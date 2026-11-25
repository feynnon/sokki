//! Dictation for the Sokki keyboard (`mobile/keyboard`,
//! `dev.verbatim.sokki.SokkiVoice`). The keyboard records the microphone
//! itself and calls these functions over JNI: start, push audio, stop, and
//! poll for text.
//!
//! One pass, as the Mac does it: the audio goes into the streaming model
//! (Nemotron Streaming) as it arrives, 200 ms at a time. The model keeps its
//! context and adds words about every half second; words it has moved past
//! never change, so the keyboard types them for good right away. At the stop
//! only the last half second is left.
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
use transcribe_cpp::{ParakeetStreamOptions, RunOptions, Session, StreamExtension, StreamOptions};

use crate::dictation::{engine_at, threads};

/// What `poll` returns once the final text is out, or for an error.
const DONE: &str = "\u{0}done";
const ERROR: &str = "\u{0}error:";
/// "\0text:<text>": everything heard so far in this dictation.
const TEXT: &str = "\u{0}text:";
/// "\0final:<text>": the whole dictation, final.
const FINAL: &str = "\u{0}final:";

const RATE: usize = 16_000;
/// How far the model listens ahead before it writes a word, in 80 ms
/// frames: 6 adds words every ~0.56 s, as accurate as 13 (~1.1 s) on our
/// clips and 0.5 s sooner; 0 was less accurate (asr-bench `--stream`).
const LOOKAHEAD: i32 = 6;

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

fn stream_options() -> StreamOptions {
    StreamOptions {
        family: Some(StreamExtension::ParakeetStream(ParakeetStreamOptions {
            att_context_right: Some(LOOKAHEAD),
        })),
        ..Default::default()
    }
}

/// Runs half a second of silence through a stream: touches the model and
/// the session's buffers, so the next dictation finds them in memory.
fn warm(session: &mut Session) {
    let Ok(mut st) = session.stream(&RunOptions::default(), &stream_options()) else {
        return;
    };
    let _ = st.feed(&vec![0.0f32; RATE / 2]);
    let _ = st.finalize();
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

/// The stream of one dictation: feeds the audio as it comes and hands the
/// keyboard the text whenever it grows.
fn transcribe(mine: usize, path: &str, rx: Receiver<Msg>, started: Instant) {
    let s = match session(path) {
        Ok(s) => s,
        Err(e) => {
            log(&format!("start failed: {e}"));
            send(mine, format!("{ERROR}{e}"));
            return;
        }
    };
    let mut guard = s.lock().unwrap();
    let mut st = match guard.stream(&RunOptions::default(), &stream_options()) {
        Ok(st) => st,
        Err(e) => {
            log(&format!("stream failed: {e}"));
            send(mine, format!("{ERROR}{e}"));
            return;
        }
    };
    let (mut shown, mut heard, mut slowest, mut first) = (String::new(), 0usize, 0u128, None);
    let mut stopped = None;
    // Block by block (200 ms each, as the keyboard sends them), also when a
    // backlog waits (audio recorded while the model loaded): one big feed is
    // much slower than small ones (2 s at once: 3.9 s; asr-bench
    // VB_BLOCK_MS), so it would only fall further behind.
    for msg in rx.iter() {
        let audio = match msg {
            Msg::Audio(a) => a,
            Msg::End => {
                stopped = Some(Instant::now());
                break;
            }
        };
        heard += audio.len();
        let t = Instant::now();
        if let Err(e) = st.feed(&audio) {
            log(&format!("feed failed: {e}"));
            send(mine, format!("{ERROR}{e}"));
            return;
        }
        slowest = slowest.max(t.elapsed().as_millis());
        let text = st.text().display().trim().to_string();
        if text != shown {
            if first.is_none() {
                first = Some(started.elapsed().as_millis());
                log(&format!(
                    "first words {} ms after start ({:.1} s heard)",
                    started.elapsed().as_millis(),
                    heard as f64 / RATE as f64
                ));
            }
            log(&format!(
                "text {} chars at {} ms, {:.2} s heard, feed {} ms",
                text.chars().count(),
                started.elapsed().as_millis(),
                heard as f64 / RATE as f64,
                t.elapsed().as_millis()
            ));
            send(mine, format!("{TEXT}{text}"));
            shown = text;
        }
    }
    let t = Instant::now();
    if let Err(e) = st.finalize() {
        log(&format!("finalize failed: {e}"));
    }
    let text = st.text().display().trim().to_string();
    send(mine, format!("{FINAL}{text}"));
    send(mine, DONE.to_string());
    log(&format!(
        "done {} ms after stop (finalize {} ms): {:.1} s heard, {} chars, slowest feed {slowest} ms",
        stopped.map_or(0, |s| s.elapsed().as_millis()),
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
    std::thread::spawn(move || transcribe(mine, &path, rx, started));
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

/// The next message: "\0text:<text>", "\0final:<text>", "\0done",
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
    while out.len() > 1 && out[0].starts_with(TEXT) && out[1].starts_with(TEXT) {
        out.remove(0);
    }
    let s = out.remove(0);
    to_jstring(&mut env, &s)
}
