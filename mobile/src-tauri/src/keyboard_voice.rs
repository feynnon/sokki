//! Dictation for the Sokki keyboard (`mobile/keyboard`,
//! `dev.verbatim.sokki.SokkiVoice`). The keyboard records the microphone
//! itself and calls these functions over JNI: start, push audio, stop, and
//! poll for text.
//!
//! Through the Mac first, when it's reachable: the keyboard keeps a link to the
//! paired Mac open (`sokki-link`, opened when the keyboard opens) and
//! dictates through it. The Mac reads the audio with its Parakeet on the GPU:
//! live text every 0.5 s ("\0live:", may still change), the whole dictation
//! read once at the stop. Faster and more accurate than the phone (13 test
//! clips: first words 0.66 s vs 1.8 s, 19 vs 29 word errors;
//! `docs/research/phone-dictation-latency-2026-11.md`). When the Mac stops
//! answering (2.5 s without text, or an error), the phone takes over with
//! the audio so far; nothing is lost.
//!
//! On the phone (no Mac): Parakeet in pieces (`pieces.rs`): the growing
//! piece is read every half second and stays underlined; a piece that ended
//! at a pause is read whole and typed for good ("\0parts:"). Numbers there.
//!
//! Before (two passes on Parakeet Ultra, 1–1.5 s snippets read without
//! context plus 6–15 s pieces read again at pauses), measured on Robin's
//! Pixel 7 on 2026-11-24: first words 2.9 s after the tap, live text frozen
//! ~3 s at every pause (both passes fought over the CPU), 3.6 s from stop
//! to done. Numbers for this version: `mobile/asr-bench/README.md`.

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use jni::objects::{JClass, JShortArray, JString};
use jni::sys::jstring;
use jni::JNIEnv;
use transcribe_cpp::Session;

use crate::dictation::{engine_at, threads, warm};
use crate::pieces::Live;

/// What `poll` returns once the final text is out, or for an error.
const DONE: &str = "\u{0}done";
const ERROR: &str = "\u{0}error:";
/// "\0text:<text>": everything heard so far in this dictation.
const TEXT: &str = "\u{0}text:";
/// "\0final:<text>": the whole dictation, final.
const FINAL: &str = "\u{0}final:";
/// "\0live:<text>": everything heard so far, through the Mac; any of it
/// may still change until the final text.
const LIVE: &str = "\u{0}live:";
/// "\0parts:<kept>\u{1}<growing>": on the phone, the text of the pieces
/// that ended (typed for good) and the growing piece's latest reading.
const PARTS: &str = "\u{0}parts:";
/// "\0draft:<text>": through the Mac with the cleanup on, the whole
/// dictation at the stop, ready to send (typed for good); "\0final:" then
/// brings the cleaned text, swapped in where it changed.
const DRAFT: &str = "\u{0}draft:";

const RATE: usize = 16_000;
enum Msg {
    Audio(Vec<f32>),
    End,
}

/// The model's session, made once per process (the keyboard has its own).
static SESSION: Mutex<Option<Arc<Mutex<Session>>>> = Mutex::new(None);
/// Audio for the current dictation's stream.
static FEED: Mutex<Option<Sender<Msg>>> = Mutex::new(None);
/// Text for the keyboard, oldest first.
static OUT: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The current dictation; a thread of an earlier one must not write into
/// the next one.
static DICTATION: AtomicUsize = AtomicUsize::new(0);

/// Hands `msg` to the keyboard if dictation `mine` is still the current one.
fn send(mine: usize, msg: String) {
    let mut out = OUT.lock().unwrap();
    if DICTATION.load(Ordering::SeqCst) == mine {
        out.push(msg);
    }
}

fn to_jstring(env: &mut JNIEnv, s: &str) -> jstring {
    env.new_string(s)
        .map(|j| j.into_raw())
        .unwrap_or(std::ptr::null_mut())
}

/// Logcat (tag "SokkiVoice"). Lengths and timings only: what people say
/// stays out of the log.
#[cfg(target_os = "android")]
fn log(msg: &str) {
    #[link(name = "log")]
    unsafe extern "C" {
        fn __android_log_write(prio: i32, tag: *const u8, text: *const u8) -> i32;
    }
    if let Ok(text) = std::ffi::CString::new(msg) {
        // 4 = ANDROID_LOG_INFO
        unsafe { __android_log_write(4, c"SokkiVoice".as_ptr().cast(), text.as_ptr().cast()) };
    }
}
#[cfg(not(target_os = "android"))]
fn log(msg: &str) {
    eprintln!("SokkiVoice: {msg}");
}

/// Asks the kernel to read the model file back into memory in the
/// background (MADV_WILLNEED on its mapping). Android drops the pages of an
/// idle keyboard: on Robin's Pixel none of the 723 MB was left after a day,
/// and the first words then waited for the flash.
fn prefetch_model() {
    #[cfg(target_os = "android")]
    {
        unsafe extern "C" {
            fn madvise(addr: *mut std::ffi::c_void, len: usize, advice: i32) -> i32;
        }
        const MADV_WILLNEED: i32 = 3;
        let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
        for line in maps.lines().filter(|l| l.ends_with(".gguf")) {
            let Some((a, b)) = line
                .split_whitespace()
                .next()
                .and_then(|r| r.split_once('-'))
            else {
                continue;
            };
            if let (Ok(a), Ok(b)) = (usize::from_str_radix(a, 16), usize::from_str_radix(b, 16)) {
                unsafe { madvise(a as *mut _, b - a, MADV_WILLNEED) };
            }
        }
    }
}

/// Loads the model (once per process) and makes the session, warmed up.
fn session(path: &str) -> Result<Arc<Mutex<Session>>, String> {
    let mut slot = SESSION.lock().unwrap();
    if let Some(s) = slot.as_ref() {
        return Ok(s.clone());
    }
    let t = Instant::now();
    let engine = engine_at(Path::new(path))?;
    let loaded = t.elapsed().as_millis();
    let mut s = engine.new_session(threads())?;
    warm(&mut s);
    log(&format!(
        "ready: model {loaded} ms, warm {} ms, {} threads",
        t.elapsed().as_millis() - loaded,
        threads()
    ));
    let s = Arc::new(Mutex::new(s));
    *slot = Some(s.clone());
    Ok(s)
}

/// The keyboard opened: load the model and warm up, or, when it is loaded
/// already, bring it back into memory, so the first words don't wait.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativeWarmUp(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
) {
    let Ok(path) = env.get_string(&model_path).map(String::from) else {
        return;
    };
    mac::up(&path);
    let loaded = SESSION.lock().unwrap().clone();
    match loaded {
        Some(s) => {
            let t = Instant::now();
            prefetch_model();
            // A dictation holds the session: it's warm then anyway.
            if let Ok(mut s) = s.try_lock() {
                warm(&mut s);
                log(&format!("rewarmed in {} ms", t.elapsed().as_millis()));
            }
        }
        None => {
            if let Err(e) = session(&path) {
                log(&format!("warm-up failed: {e}"));
            }
        }
    }
}

/// One dictation on the phone: reads the audio in pieces as it comes
/// (`pieces.rs`) and hands the keyboard the text whenever it changes.
fn transcribe(mine: usize, path: &str, rx: Receiver<Msg>, started: Instant) {
    use std::sync::mpsc::{RecvTimeoutError, TryRecvError};
    let s = match session(path) {
        Ok(s) => s,
        Err(e) => {
            log(&format!("start failed: {e}"));
            send(mine, format!("{ERROR}{e}"));
            return;
        }
    };
    let mut guard = s.lock().unwrap();
    let mut live = Live::default();
    let (mut shown, mut first, mut slowest, mut reads) = (String::new(), None, 0u128, 0usize);
    let stopped = 'listening: loop {
        // Everything that has come in, then one read if one is due.
        loop {
            match rx.try_recv() {
                Ok(Msg::Audio(a)) => live.push(&a),
                Ok(Msg::End) | Err(TryRecvError::Disconnected) => break 'listening Instant::now(),
                Err(TryRecvError::Empty) => break,
            }
        }
        let t = Instant::now();
        match live.step(&mut guard) {
            Some(update) => {
                reads += 1;
                slowest = slowest.max(t.elapsed().as_millis());
                let text = update.text();
                if text == shown {
                    continue;
                }
                if first.is_none() && !text.is_empty() {
                    first = Some(());
                    log(&format!(
                        "first words {} ms after start ({:.1} s heard)",
                        started.elapsed().as_millis(),
                        live.heard as f64 / RATE as f64
                    ));
                }
                log(&format!(
                    "text {} chars at {} ms, {:.2} s heard, read {} ms",
                    text.chars().count(),
                    started.elapsed().as_millis(),
                    live.heard as f64 / RATE as f64,
                    t.elapsed().as_millis()
                ));
                send(
                    mine,
                    format!("{PARTS}{}\u{1}{}", update.committed, update.live),
                );
                shown = text;
            }
            None => match rx.recv_timeout(std::time::Duration::from_millis(20)) {
                Ok(Msg::Audio(a)) => live.push(&a),
                Ok(Msg::End) | Err(RecvTimeoutError::Disconnected) => break Instant::now(),
                Err(RecvTimeoutError::Timeout) => {}
            },
        }
    };
    let t = Instant::now();
    let heard = live.heard;
    let text = live.finish(&mut guard);
    send(mine, format!("{FINAL}{text}"));
    send(mine, DONE.to_string());
    log(&format!(
        "done {} ms after stop (last piece {} ms): {:.1} s heard, {} chars, {reads} reads, slowest {slowest} ms",
        stopped.elapsed().as_millis(),
        t.elapsed().as_millis(),
        heard as f64 / RATE as f64,
        text.chars().count()
    ));
}

/// Starts listening right away; the thread loads the model if needed
/// (audio waits meanwhile). A failure arrives through `poll` as
/// "\0error:<message>".
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativeStart(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
) {
    let path: String = env
        .get_string(&model_path)
        .map(String::from)
        .unwrap_or_default();
    let mine = {
        let mut out = OUT.lock().unwrap();
        out.clear();
        DICTATION.fetch_add(1, Ordering::SeqCst) + 1
    };
    let (tx, rx) = channel::<Msg>();
    // An earlier dictation's stream ends now, if it hadn't.
    if let Some(old) = FEED.lock().unwrap().replace(tx) {
        let _ = old.send(Msg::End);
    }
    let started = Instant::now();
    match mac::current() {
        Some(link) => {
            std::thread::spawn(move || mac::dictate(mine, path, rx, started, link));
        }
        None => {
            mac::up(&path);
            std::thread::spawn(move || transcribe(mine, &path, rx, started));
        }
    }
}

/// The keyboard opened: open the link to the paired Mac in the background,
/// if it isn't open, so the next dictation can go through the Mac.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativeLinkUp(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
) {
    if let Ok(path) = env.get_string(&model_path).map(String::from) {
        mac::up(&path);
    }
}

/// The phone takes over a dictation the Mac didn't finish: the audio so far,
/// then the rest as it comes.
fn take_over(
    mine: usize,
    path: String,
    kept: Vec<Vec<f32>>,
    rest: Option<Receiver<Msg>>,
    started: Instant,
) {
    let (tx, rx) = channel::<Msg>();
    for a in kept {
        let _ = tx.send(Msg::Audio(a));
    }
    let worker = std::thread::spawn(move || transcribe(mine, &path, rx, started));
    match rest {
        Some(rest) => {
            for m in rest.iter() {
                let end = matches!(m, Msg::End);
                let _ = tx.send(m);
                if end {
                    break;
                }
            }
        }
        None => {
            let _ = tx.send(Msg::End);
        }
    }
    let _ = worker.join();
}

/// Dictation through the paired Mac.
mod mac {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicU8};
    use std::sync::OnceLock;
    use std::time::Duration;

    use sokki_link::iroh::endpoint::Connection;
    use sokki_link::iroh::{Endpoint, EndpointId};

    /// The Mac didn't answer this long while audio was flowing: the phone
    /// takes over. The Mac sends text every 0.5 s.
    const SILENT: Duration = Duration::from_millis(2_500);
    /// After a refusal (not paired, an old Sokki), don't ask again so soon.
    const REFUSED_PAUSE: Duration = Duration::from_secs(600);

    static RT: OnceLock<tokio::runtime::Runtime> = OnceLock::new();
    static ENDPOINT: Mutex<Option<Endpoint>> = Mutex::new(None);
    static LINK: Mutex<Option<Connection>> = Mutex::new(None);
    static CONNECTING: AtomicBool = AtomicBool::new(false);
    static REFUSED: Mutex<Option<Instant>> = Mutex::new(None);

    fn rt() -> &'static tokio::runtime::Runtime {
        RT.get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("runtime")
        })
    }

    /// The app's data folder (where `link.rs` keeps the pairing), from the
    /// model's path `<data>/models/<file>`.
    fn data_dir(model_path: &str) -> Option<std::path::PathBuf> {
        Path::new(model_path)
            .parent()?
            .parent()
            .map(Path::to_path_buf)
    }

    /// This phone's key and the paired Mac, if any (`link.rs`).
    fn pairing(model_path: &str) -> Option<(sokki_link::iroh::SecretKey, EndpointId)> {
        let dir = data_dir(model_path)?;
        let key = std::fs::read_to_string(dir.join("link_key.txt")).ok()?;
        let key = sokki_link::secret_from_hex(key.trim()).ok()?;
        let p: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("pairing.json")).ok()?).ok()?;
        let mac = p["mac"].as_str()?.parse().ok()?;
        Some((key, mac))
    }

    /// The open link, if the Mac is reachable.
    pub(super) fn current() -> Option<Connection> {
        let mut link = LINK.lock().unwrap();
        if link.as_ref().is_some_and(|c| c.close_reason().is_some()) {
            *link = None;
        }
        link.clone()
    }

    /// The path the link uses now: direct or through a relay, and its round trip.
    fn path(conn: &Connection) -> String {
        conn.paths()
            .iter()
            .find(|p| p.is_selected())
            .map(|p| {
                format!(
                    "{}, {} ms",
                    if p.is_ip() { "direct" } else { "relay" },
                    p.rtt().as_millis()
                )
            })
            .unwrap_or_else(|| "no path".into())
    }

    fn drop_link() {
        *LINK.lock().unwrap() = None;
    }

    /// Opens the link in the background unless it's open or being opened.
    /// When it is open, pings the Mac so it loads its speech model.
    pub(super) fn up(model_path: &str) {
        if REFUSED
            .lock()
            .unwrap()
            .is_some_and(|t| t.elapsed() < REFUSED_PAUSE)
        {
            return;
        }
        if let Some(conn) = current() {
            rt().spawn(async move {
                let _ = sokki_link::call_on(&conn, &sokki_link::Request::Ping, &[]).await;
            });
            return;
        }
        let Some((key, mac)) = pairing(model_path) else {
            return;
        };
        if CONNECTING.swap(true, Ordering::SeqCst) {
            return;
        }
        // Through n0's relay only (still encrypted end to end; the relay
        // passes bytes on). When the link moved from the relay to a direct
        // path, the audio of the next dictation reached the Mac ~20 s late:
        // Robin's Pixel and the emulator, 2026-11-26 (KNOWN_ISSUES K12).
        // Relay only: first words 0.7–1.3 s, no stall. Debug builds:
        // `<data>/debug_direct` allows the direct path, to work on K12.
        let relay_only = !(cfg!(debug_assertions)
            && data_dir(model_path).is_some_and(|d| d.join("debug_direct").exists()));
        rt().spawn(async move {
            let t = Instant::now();
            let ep = ENDPOINT.lock().unwrap().clone();
            let ep = match ep {
                Some(ep) => Ok(ep),
                None => {
                    let ep = if relay_only {
                        Endpoint::builder(sokki_link::iroh::endpoint::presets::N0)
                            .secret_key(key)
                            .alpns(vec![sokki_link::ALPN.to_vec()])
                            .clear_ip_transports()
                            .bind()
                            .await
                            .map_err(|e| e.to_string())
                    } else {
                        sokki_link::bind(key).await.map_err(|e| format!("{e:#}"))
                    };
                    ep.inspect(|ep| {
                        *ENDPOINT.lock().unwrap() = Some(ep.clone());
                    })
                }
            };
            match ep {
                Ok(ep) => match sokki_link::connect(&ep, mac.into()).await {
                    Ok(conn) => {
                        log(&format!(
                            "Mac link open in {} ms ({})",
                            t.elapsed().as_millis(),
                            path(&conn)
                        ));
                        let _ = sokki_link::call_on(&conn, &sokki_link::Request::Ping, &[]).await;
                        *LINK.lock().unwrap() = Some(conn);
                    }
                    Err(e) => log(&format!(
                        "Mac not reachable ({} ms): {e:#}",
                        t.elapsed().as_millis()
                    )),
                },
                Err(e) => log(&format!("Mac link failed: {e}")),
            }
            CONNECTING.store(false, Ordering::SeqCst);
        });
    }

    const RUNNING: u8 = 0;
    const FINISHED: u8 = 1;
    const FAILED: u8 = 2;

    /// One dictation through the Mac; the phone takes over if the Mac
    /// stops answering.
    pub(super) fn dictate(
        mine: usize,
        model: String,
        rx: Receiver<Msg>,
        started: Instant,
        conn: Connection,
    ) {
        let conn_for_log = conn.clone();
        log(&format!("Mac: dictating ({})", path(&conn)));
        let (atx, arx) = tokio::sync::mpsc::unbounded_channel::<Vec<i16>>();
        let state = Arc::new(AtomicU8::new(RUNNING));
        let last = Arc::new(Mutex::new(Instant::now()));
        let abandoned = Arc::new(AtomicBool::new(false));
        let keep = Arc::new(AtomicBool::new(false));
        // The draft is typed: the phone never takes over after it.
        let drafted = Arc::new(AtomicBool::new(false));
        let stopped: Arc<Mutex<Option<Instant>>> = Arc::new(Mutex::new(None));
        {
            let (state, last, abandoned, stopped, keep, drafted) = (
                state.clone(),
                last.clone(),
                abandoned.clone(),
                stopped.clone(),
                keep.clone(),
                drafted.clone(),
            );
            rt().spawn(async move {
                let mut first = true;
                let r = sokki_link::dictate(&conn, arx, |t| {
                    *last.lock().unwrap() = Instant::now();
                    if abandoned.load(Ordering::SeqCst) {
                        return;
                    }
                    let heard = t.heard as f64 / RATE as f64;
                    if t.is_final {
                        // Before FINISHED, so the take-over can't start after it.
                        if abandoned.load(Ordering::SeqCst) {
                            return;
                        }
                        state.store(FINISHED, Ordering::SeqCst);
                        send(mine, format!("{FINAL}{}", t.text));
                        send(mine, DONE.to_string());
                        log(&format!(
                            "Mac: done {} ms after stop: {heard:.1} s heard, {} chars",
                            stopped
                                .lock()
                                .unwrap()
                                .map_or(0, |s| s.elapsed().as_millis()),
                            t.text.chars().count()
                        ));
                    } else if t.draft {
                        // Once: the keep-alive repeats it.
                        if !drafted.swap(true, Ordering::SeqCst) {
                            send(mine, format!("{DRAFT}{}", t.text));
                            log(&format!(
                                "Mac: draft {} ms after stop, {} chars",
                                stopped
                                    .lock()
                                    .unwrap()
                                    .map_or(0, |s| s.elapsed().as_millis()),
                                t.text.chars().count()
                            ));
                        }
                    } else {
                        if first && !t.text.is_empty() {
                            first = false;
                            log(&format!(
                                "Mac: first words {} ms after start ({heard:.1} s heard)",
                                started.elapsed().as_millis()
                            ));
                        }
                        send(mine, format!("{LIVE}{}", t.text));
                    }
                })
                .await;
                if let Err(e) = r {
                    let e = format!("{e:#}");
                    log(&format!("Mac: dictation failed: {e}"));
                    if e.contains("paired")
                        || e.contains("can't take")
                        || e.contains("unknown request")
                    {
                        *REFUSED.lock().unwrap() = Some(Instant::now());
                    }
                    // Reachable, only not ready: keep the link for the next one.
                    keep.store(
                        e.contains("loading") || e.contains("busy"),
                        Ordering::SeqCst,
                    );
                    state.store(FAILED, Ordering::SeqCst);
                }
            });
        }
        let mut kept: Vec<Vec<f32>> = Vec::new();
        let audio_started = Instant::now();
        let take_over_now = |why: &str, kept: Vec<Vec<f32>>, rest: Option<Receiver<Msg>>| {
            abandoned.store(true, Ordering::SeqCst);
            if !keep.load(Ordering::SeqCst) {
                drop_link();
            }
            log(&format!(
                "Mac: {why}; the phone takes over ({:.1} s of audio so far)",
                kept.iter().map(Vec::len).sum::<usize>() as f64 / RATE as f64
            ));
            log(&format!("Mac: link path then: {}", path(&conn_for_log)));
            take_over(mine, model.clone(), kept, rest, started);
        };
        // While listening: send each block, keep a copy.
        loop {
            match rx.recv_timeout(Duration::from_millis(100)) {
                Ok(Msg::Audio(a)) => {
                    let pcm = a
                        .iter()
                        .map(|s| (s * 32768.0).clamp(-32768.0, 32767.0) as i16)
                        .collect();
                    let _ = atx.send(pcm);
                    kept.push(a);
                }
                Ok(Msg::End) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
            if state.load(Ordering::SeqCst) == FAILED {
                return take_over_now("failed", kept, Some(rx));
            }
            if audio_started.elapsed() > SILENT && last.lock().unwrap().elapsed() > SILENT {
                return take_over_now("no answer", kept, Some(rx));
            }
        }
        *stopped.lock().unwrap() = Some(Instant::now());
        drop(atx);
        // Stopped: the Mac reads the whole dictation (0.3 s at the median,
        // ~3 s for 3 minutes); give it more before taking over.
        let audio_s = kept.iter().map(Vec::len).sum::<usize>() as f64 / RATE as f64;
        // Reading it all again here takes longer than the Mac ever does, so
        // wait while it says it's at it (a Mac that reads sends something
        // every second; older ones don't, for them the deadline).
        let deadline = Duration::from_secs_f64(5.0 + audio_s * 0.05);
        let since = Instant::now();
        loop {
            match state.load(Ordering::SeqCst) {
                FINISHED => return,
                // The draft is in the field, ready to send: it stays.
                FAILED if drafted.load(Ordering::SeqCst) => {
                    log("Mac: no cleaned text after the draft; the draft stays");
                    send(mine, DONE.to_string());
                    return;
                }
                FAILED => return take_over_now("failed at the stop", kept, None),
                _ if since.elapsed() > deadline && last.lock().unwrap().elapsed() > SILENT => {
                    if drafted.load(Ordering::SeqCst) {
                        abandoned.store(true, Ordering::SeqCst);
                        log("Mac: no cleaned text in time; the draft stays");
                        send(mine, DONE.to_string());
                        return;
                    }
                    return take_over_now("no final text in time", kept, None);
                }
                _ => std::thread::sleep(Duration::from_millis(20)),
            }
        }
    }
}

/// Microphone audio: 16 kHz mono, 16-bit.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativePush(
    env: JNIEnv,
    _class: JClass,
    pcm: JShortArray,
) {
    let Ok(len) = env.get_array_length(&pcm) else {
        return;
    };
    let mut buf = vec![0i16; len as usize];
    if env.get_short_array_region(&pcm, 0, &mut buf).is_err() {
        return;
    }
    let samples: Vec<f32> = buf.iter().map(|&v| v as f32 / 32768.0).collect();
    if let Some(tx) = FEED.lock().unwrap().as_ref() {
        let _ = tx.send(Msg::Audio(samples));
    }
}

/// The person stopped: the stream reads the rest and `poll` returns the
/// final text, then the done marker.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativeStop(_env: JNIEnv, _class: JClass) {
    if let Some(tx) = FEED.lock().unwrap().take() {
        let _ = tx.send(Msg::End);
    }
}

/// The next message: "\0text:<text>", "\0live:<text>", "\0final:<text>", "\0done",
/// "\0error:<message>", or null. Of several "\0text:" in a row only the
/// newest counts, so the older ones are skipped.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativePoll(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    let mut out = OUT.lock().unwrap();
    if out.is_empty() {
        return std::ptr::null_mut();
    }
    while out.len() > 1
        && ((out[0].starts_with(TEXT) && out[1].starts_with(TEXT))
            || (out[0].starts_with(LIVE) && out[1].starts_with(LIVE))
            || (out[0].starts_with(PARTS) && out[1].starts_with(PARTS)))
    {
        out.remove(0);
    }
    let s = out.remove(0);
    to_jstring(&mut env, &s)
}
