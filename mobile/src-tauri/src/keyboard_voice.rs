//! Dictation for the Sokki keyboard (`mobile/keyboard`,
//! `dev.verbatim.sokki.SokkiVoice`). The keyboard records the microphone
//! itself and calls these functions over JNI: start, push audio, stop, and
//! poll for text.
//!
//! Two passes, on two sessions of the same model, in parallel:
//! - Live: every 1–1.5 s the newest audio becomes a snippet (cut at its
//!   quietest spot), transcribed once and shown right away as provisional
//!   (composing) text. Nothing is read twice, so this keeps up easily.
//! - Final: pieces are cut at pauses after 6–15 s (`dictation-core`, as in
//!   the Dictate tab) and each is transcribed whole, with full context; that
//!   text replaces the piece's snippets, so corrections show up.
//! Measured on a Pixel 7 (`asr-bench --live`, 2026-11-19): first words after
//! ~1.4 s, live text ~0.7 s behind the speech, every word visible when the
//! person stops; final text as accurate as before (6.5 % off the whole
//! recording, vs 8.3 % for 4–8 s pieces). Snippets alone are rough
//! (~38 % off) until their piece is final, 2–4 s after each pause.

use std::collections::VecDeque;
use std::path::Path;
use std::sync::mpsc::{channel, Sender};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use dictation_core::{Mode, Params, Segmenter, RATE};
use jni::objects::{JClass, JShortArray, JString};
use jni::sys::jstring;
use jni::JNIEnv;

use crate::dictation::engine_at;

/// What `poll` returns once the last piece is final, or for an error.
const DONE: &str = "\u{0}done";
const ERROR: &str = "\u{0}error:";
/// "\0snip:<piece>:<text>": live text to add to piece <piece>.
const SNIP: &str = "\u{0}snip:";
/// "\0final:<piece>:<text>": the final text of piece <piece>.
const FINAL: &str = "\u{0}final:";

/// Snippets: cut at the quietest spot between `min` and `max` of new audio.
const SNIP_MIN: usize = RATE;
const SNIP_MAX: usize = RATE * 3 / 2;
/// The first one comes sooner.
const FIRST_MIN: usize = RATE * 6 / 10;
const FIRST_MAX: usize = RATE;
/// CPU threads per session (4 + 4 was best on a Pixel 7: 2+4 lagged).
const THREADS: i32 = 4;

enum Job {
    Piece { index: usize, audio: Vec<f32>, speech_end: usize },
    End,
}

struct Live {
    segmenter: Segmenter,
    finals: Sender<Job>,
    /// Pieces cut so far (= index of the growing piece).
    cuts: usize,
    /// Samples of the growing piece already in snippets.
    snipped: usize,
    snippets: usize,
    stopped: bool,
}

/// The two sessions, made once per process (the keyboard has its own).
struct Sessions {
    live: Mutex<transcribe_cpp::Session>,
    last: Mutex<transcribe_cpp::Session>,
}

static LIVE: Mutex<Option<Live>> = Mutex::new(None);
static SESSIONS: Mutex<Option<Arc<Sessions>>> = Mutex::new(None);
/// Text for the keyboard, oldest first.
static OUT: Mutex<VecDeque<String>> = Mutex::new(VecDeque::new());
/// The current dictation; threads of an earlier one (a snippet still
/// running after it ended) must not write into the next one.
static DICTATION: AtomicUsize = AtomicUsize::new(0);

/// Hands `msg` to the keyboard if dictation `mine` is still the current one.
fn send(mine: usize, msg: String) {
    let mut out = OUT.lock().unwrap();
    if DICTATION.load(Ordering::SeqCst) == mine {
        out.push_back(msg);
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

fn secs(samples: usize) -> f64 {
    samples as f64 / RATE as f64
}

fn run(session: &Mutex<transcribe_cpp::Session>, audio: &[f32]) -> String {
    let opts = transcribe_cpp::RunOptions::default();
    session
        .lock()
        .unwrap()
        .run(audio, &opts)
        .map(|r| r.text.trim().to_string())
        .unwrap_or_default()
}

/// Loads the model (once per process) and makes the two sessions, each
/// warmed up on a second of silence.
fn sessions(path: &str) -> Result<Arc<Sessions>, String> {
    let mut slot = SESSIONS.lock().unwrap();
    if let Some(s) = slot.as_ref() {
        return Ok(s.clone());
    }
    let t = Instant::now();
    let engine = engine_at(Path::new(path))?;
    let loaded = t.elapsed().as_millis();
    let s = Arc::new(Sessions {
        live: Mutex::new(engine.new_session(THREADS)?),
        last: Mutex::new(engine.new_session(THREADS)?),
    });
    let silence = vec![0.0f32; RATE];
    run(&s.live, &silence);
    run(&s.last, &silence);
    log(&format!("ready: model {loaded} ms, sessions warm {} ms", t.elapsed().as_millis() - loaded));
    *slot = Some(s.clone());
    Ok(s)
}

/// Where to end a snippet: the quietest 20 ms in `audio[lo..]`.
fn quietest(audio: &[f32], lo: usize) -> usize {
    const F: usize = RATE / 50;
    let energy = |i: usize| audio[i * F..(i + 1) * F].iter().map(|s| s * s).sum::<f32>();
    (lo / F..audio.len() / F)
        .min_by(|&a, &b| energy(a).partial_cmp(&energy(b)).unwrap())
        .map_or(audio.len(), |i| i * F + F / 2)
}

/// The keyboard opened: load the model and warm up, so the first dictation
/// doesn't wait for it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativeWarmUp(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
) {
    let Ok(path) = env.get_string(&model_path).map(String::from) else {
        return;
    };
    if let Err(e) = sessions(&path) {
        log(&format!("warm-up failed: {e}"));
    }
}

/// Starts listening right away; the threads load the model if needed
/// (audio waits meanwhile). A failure arrives through `poll` as
/// "\0error:<message>".
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativeStart(
    mut env: JNIEnv,
    _class: JClass,
    model_path: JString,
) {
    let path: String = env.get_string(&model_path).map(String::from).unwrap_or_default();
    let mine = {
        let mut out = OUT.lock().unwrap();
        out.clear();
        DICTATION.fetch_add(1, Ordering::SeqCst) + 1
    };
    let (tx, rx) = channel::<Job>();
    let started = Instant::now();
    *LIVE.lock().unwrap() = Some(Live {
        segmenter: Segmenter::new(Mode::Pauses(Params::default())),
        finals: tx,
        cuts: 0,
        snipped: 0,
        snippets: 0,
        stopped: false,
    });

    // Final passes: each finished piece, whole.
    let p = path.clone();
    std::thread::spawn(move || {
        let s = match sessions(&p) {
            Ok(s) => s,
            Err(e) => {
                log(&format!("start failed: {e}"));
                send(mine, format!("{ERROR}{e}"));
                return;
            }
        };
        for job in rx {
            match job {
                Job::Piece { index, audio, speech_end } => {
                    let t = Instant::now();
                    let text = if speech_end == 0 { String::new() } else { run(&s.last, &audio) };
                    log(&format!(
                        "final {index}: {:.1} s -> {} chars in {} ms ({} ms after start)",
                        secs(audio.len()),
                        text.chars().count(),
                        t.elapsed().as_millis(),
                        started.elapsed().as_millis()
                    ));
                    send(mine, format!("{FINAL}{index}:{text}"));
                }
                Job::End => break,
            }
        }
        send(mine, DONE.to_string());
        log(&format!("done {} ms after start", started.elapsed().as_millis()));
    });

    // Live snippets: the newest audio, each bit once.
    std::thread::spawn(move || {
        let Ok(s) = sessions(&path) else { return };
        loop {
            // Take the next snippet (or, once stopped, the rest) under the lock.
            let job = {
                let mut guard = LIVE.lock().unwrap();
                let Some(live) = guard.as_mut() else { return };
                let pending = live.segmenter.pending();
                let (min, max) = if live.snippets == 0 { (FIRST_MIN, FIRST_MAX) } else { (SNIP_MIN, SNIP_MAX) };
                let from = live.snipped.min(pending.len());
                if live.stopped {
                    // The words not in a snippet yet, then the piece goes to its final pass.
                    let rest = (live.segmenter.speech_end(&pending[from..]) > 0).then(|| pending[from..].to_vec());
                    let index = live.cuts;
                    let mut live = guard.take().unwrap();
                    if let Some(audio) = live.segmenter.finish() {
                        let speech_end = live.segmenter.speech_end(&audio);
                        let _ = live.finals.send(Job::Piece { index, audio, speech_end });
                    }
                    let _ = live.finals.send(Job::End);
                    rest.map(|audio| (index, audio, true))
                } else if pending.len() >= from + max {
                    let cut = quietest(pending, from + min);
                    live.snipped = cut;
                    live.snippets += 1;
                    Some((live.cuts, pending[from..cut].to_vec(), false))
                } else {
                    None
                }
            };
            match job {
                Some((index, audio, last)) => {
                    let t = Instant::now();
                    let text = run(&s.live, &audio);
                    log(&format!(
                        "snippet {index}: {:.1} s -> {} chars in {} ms ({} ms after start)",
                        secs(audio.len()),
                        text.chars().count(),
                        t.elapsed().as_millis(),
                        started.elapsed().as_millis()
                    ));
                    if !text.is_empty() {
                        send(mine, format!("{SNIP}{index}:{text}"));
                    }
                    if last {
                        return;
                    }
                }
                None => std::thread::sleep(Duration::from_millis(40)),
            }
        }
    });
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
    let mut guard = LIVE.lock().unwrap();
    if let Some(live) = guard.as_mut() {
        if live.stopped {
            return;
        }
        for audio in live.segmenter.push(&samples) {
            let speech_end = live.segmenter.speech_end(&audio);
            // Snippets that ran past the cut are in the pause: not taken again.
            live.snipped = live.snipped.saturating_sub(audio.len());
            let _ = live.finals.send(Job::Piece { index: live.cuts, audio, speech_end });
            live.cuts += 1;
        }
    }
}

/// The person stopped: the snippet thread shows the last words, then the
/// last piece gets its final pass; `poll` returns the done marker after it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativeStop(_env: JNIEnv, _class: JClass) {
    if let Some(live) = LIVE.lock().unwrap().as_mut() {
        live.stopped = true;
    }
}

/// The next message: "\0snip:<piece>:<text>", "\0final:<piece>:<text>",
/// "\0done", "\0error:<message>", or null.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_verbatim_sokki_SokkiVoice_nativePoll(
    mut env: JNIEnv,
    _class: JClass,
) -> jstring {
    match OUT.lock().unwrap().pop_front() {
        Some(s) => to_jstring(&mut env, &s),
        None => std::ptr::null_mut(),
    }
}
