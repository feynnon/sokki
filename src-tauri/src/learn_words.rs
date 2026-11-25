//! Learning names and terms from what "Keep what I write" keeps.
//!
//! Once a day, while the Mac has been idle for 5 minutes and no meeting
//! records or is processed:
//! 1. Candidates from the journal (`sokki_journal::learn`): words the spell
//!    checker doesn't know, typed in 2+ messages, plus words the user fixed
//!    by hand after a dictation ("Fassel" → "Vercel", learned as a pair).
//! 2. The local language model (built in or the user's own server, never a
//!    cloud provider: typed text stays on this Mac) answers whether each is
//!    a name or term worth spelling right.
//! 3. Hear test: macOS's voices say the word in a sentence (to a file, not
//!    out loud) and the speech model transcribes it. Heard right: the word
//!    is only learned. Heard wrong: "heard as" pairs too, unless the heard
//!    form is all normal words ("get up" for GitHub), which would overwrite
//!    real speech.
//!
//! Results go to `vocabulary_learned` / `vocabulary_heard_as` (Vocabulary
//! page). Removed ones go to `vocabulary_rejected` and never come back.
//! Test mode: `SOKKI_TEST_LEARN=1` runs once right away.

use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use tauri::{AppHandle, Manager};

use sokki_journal::learn;

use crate::managers::transcription::TranscriptionManager;
use crate::settings::{self, AppSettings, HeardAs};

/// Learned words are capped here (Granola keeps 300 too).
const MAX_LEARNED: usize = 300;
/// How often word learning runs.
const EVERY: Duration = Duration::from_secs(20 * 3600);
/// The Mac has to be idle this long first.
const IDLE: Duration = Duration::from_secs(300);
/// Journal entries older than this are not read.
const LOOK_BACK_MS: u64 = 30 * 24 * 3600 * 1000;

const JUDGE: &str = "A dictation app keeps a personal dictionary so its speech recognition spells the user's words right. You get a word the user typed that is not in the normal dictionary, with the sentence it came from.

Answer \"add\" if it is a word the user would say aloud and want spelled exactly this way: a person's name, a company, product, project, place, or a technical term or acronym.
Answer \"no\" if it is a typo, a misspelling of a normal word, chat slang, an abbreviation people don't speak, a code identifier, or private data such as a number, password, key or account number.

Answer with one word: add or no.";

/// The voices and sentences of the hear test: "{}" is the word.
const HEAR: &[(&str, &str)] = &[
    ("Anna", "Ich habe heute mit {} gesprochen."),
    ("Samantha", "I talked to {} today."),
];

fn test_run() -> bool {
    crate::test_mic::active() && std::env::var_os("SOKKI_TEST_LEARN").is_some()
}

/// Starts the daily check. Call once at startup.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("learn-words".into())
        .spawn(move || {
            if test_run() {
                std::thread::sleep(Duration::from_secs(5));
                log_run(&app);
                return;
            }
            loop {
                std::thread::sleep(Duration::from_secs(600));
                let s = settings::get_settings(&app);
                let due = sokki_journal::now_ms().saturating_sub(s.learn_words_last_run)
                    >= EVERY.as_millis() as u64;
                if s.keep_what_i_write && s.learn_words && due && !busy() && idle_for() >= IDLE {
                    log_run(&app);
                }
            }
        });
    if let Err(e) = spawned {
        log::error!("Word learning could not start: {e}");
    }
}

fn log_run(app: &AppHandle) {
    let t0 = Instant::now();
    match run(app) {
        Ok(r) => log::info!(
            "Word learning: {} candidates, {} learned, {} heard-as pairs ({} from your fixes) in {:.1}s",
            r.candidates,
            r.learned,
            r.pairs,
            r.fixes,
            t0.elapsed().as_secs_f64()
        ),
        Err(e) => log::warn!("Word learning stopped: {e:#}"),
    }
}

/// A meeting records or is processed, or a dictation records.
fn busy() -> bool {
    crate::meeting::priority::recording() || crate::meeting::process::busy()
}

/// How long since the last key press or mouse move.
fn idle_for() -> Duration {
    #[cfg(target_os = "macos")]
    {
        #[link(name = "CoreGraphics", kind = "framework")]
        extern "C" {
            fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
        }
        // Combined session state, any input event.
        let s = unsafe { CGEventSourceSecondsSinceLastEventType(0, u32::MAX) };
        Duration::from_secs_f64(s.max(0.0))
    }
    #[cfg(not(target_os = "macos"))]
    Duration::ZERO
}

fn dictating(app: &AppHandle) -> bool {
    app.state::<Arc<crate::managers::audio::AudioRecordingManager>>()
        .is_recording()
}

#[derive(Default)]
struct Outcome {
    candidates: usize,
    learned: usize,
    pairs: usize,
    fixes: usize,
}

fn spell_languages(s: &AppSettings) -> Vec<String> {
    s.spoken_languages.clone()
}

fn known(word: &str, langs: &[String]) -> bool {
    #[cfg(target_os = "macos")]
    {
        sokki_journal::mac::spell_known(word, langs)
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (word, langs);
        true
    }
}

fn key(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn run(app: &AppHandle) -> Result<Outcome> {
    let s = settings::get_settings(app);
    let dir = crate::portable::app_data_dir(app)
        .context("no app data folder")?
        .join("typing_log");
    let since = sokki_journal::now_ms().saturating_sub(LOOK_BACK_MS);
    let entries: Vec<_> = sokki_journal::store::read_all(&dir)
        .into_iter()
        .filter(|e| e.written >= since)
        .collect();
    let langs = spell_languages(&s);
    let mut is_known = |w: &str| known(w, &langs);
    let rejected: Vec<String> = s
        .vocabulary_rejected
        .iter()
        .map(|w| w.to_lowercase())
        .collect();
    let mut skip: std::collections::HashSet<String> = s
        .all_vocabulary()
        .iter()
        .map(|w| w.to_lowercase())
        .collect();
    skip.extend(rejected.iter().cloned());

    let mut out = Outcome::default();
    let mut words: Vec<String> = Vec::new();
    let mut pairs: Vec<HeardAs> = Vec::new();

    // The user's own fixes first: no model needed.
    for p in learn::corrections(&entries, &mut is_known) {
        if rejected.contains(&p.heard.to_lowercase()) || rejected.contains(&p.word.to_lowercase()) {
            continue;
        }
        out.fixes += 1;
        if !skip.contains(&p.word.to_lowercase()) {
            words.push(p.word.clone());
        }
        pairs.push(HeardAs {
            heard: p.heard,
            word: p.word,
            source: "correction".into(),
        });
    }

    let candidates = learn::candidates(&entries, &mut is_known, &skip);
    out.candidates = candidates.len();
    if !candidates.is_empty() {
        let accepted = judge(app, &s, &candidates)?;
        for word in accepted {
            if busy() || dictating(app) {
                log::info!(
                    "Word learning: paused for a recording; the rest waits for the next run"
                );
                break;
            }
            match hear_test(app, &word, &langs) {
                Ok(heard) => {
                    for h in heard {
                        if !rejected.contains(&h.to_lowercase()) {
                            pairs.push(HeardAs {
                                heard: h,
                                word: word.clone(),
                                source: "hear_test".into(),
                            });
                        }
                    }
                }
                Err(e) => log::warn!("Word learning: hear test failed: {e:#}"),
            }
            words.push(word);
        }
    }

    // Save: one write of the current settings, so nothing the user changed
    // meanwhile is lost.
    let mut now = settings::get_settings(app);
    for w in words {
        if now.vocabulary_learned.len() >= MAX_LEARNED {
            break;
        }
        if !now
            .all_vocabulary()
            .iter()
            .any(|x| x.eq_ignore_ascii_case(&w))
        {
            now.vocabulary_learned.push(w);
            out.learned += 1;
        }
    }
    for p in pairs {
        if !now
            .vocabulary_heard_as
            .iter()
            .any(|h| key(&h.heard) == key(&p.heard))
        {
            now.vocabulary_heard_as.push(p);
            out.pairs += 1;
        }
    }
    now.learn_words_last_run = sokki_journal::now_ms();
    settings::write_settings(app, now);
    Ok(out)
}

/// Asks the local language model about each candidate; returns the words to learn.
fn judge(app: &AppHandle, s: &AppSettings, candidates: &[learn::Candidate]) -> Result<Vec<String>> {
    let llm_settings = crate::meeting::process::engine_settings(s).llm;
    if sokki_engine::llm::CLOUD_PROVIDERS.contains(&llm_settings.provider.trim()) {
        anyhow::bail!("the language model is a cloud provider; typed text stays on this Mac");
    }
    let t0 = Instant::now();
    let model = sokki_engine::llm::open(&llm_settings, &s.meetings.user_name)?;
    log::info!(
        "Word learning: language model ready in {:.1}s",
        t0.elapsed().as_secs_f64()
    );
    let llm = meeting_core::summary::LlmSummarizer::new(model.config());
    let mut accepted = Vec::new();
    for c in candidates {
        if busy() || dictating(app) {
            break;
        }
        let user = format!("Word: {}\nSentence: {}", c.word, c.sentence);
        match llm.chat_plain(JUDGE, &user) {
            Ok(answer) => {
                let yes = answer.trim().to_lowercase().starts_with("add");
                log::debug!(
                    "Word learning: model says {} for a candidate",
                    if yes { "add" } else { "no" }
                );
                if yes {
                    accepted.push(c.word.clone());
                }
            }
            Err(e) => log::warn!("Word learning: the language model failed: {e:#}"),
        }
    }
    drop(model);
    Ok(accepted)
}

/// The words the speech model wrote where `sentence` (with "{}") had the word.
fn slot(transcript: &str, sentence: &str) -> Vec<String> {
    let clean = |s: &str| key(s);
    let (before, after) = sentence.split_once("{}").unwrap_or((sentence, ""));
    let pre: Vec<String> = before
        .split_whitespace()
        .map(clean)
        .filter(|w| !w.is_empty())
        .collect();
    let post: Vec<String> = after
        .split_whitespace()
        .map(clean)
        .filter(|w| !w.is_empty())
        .collect();
    let mut words: Vec<&str> = transcript.split_whitespace().collect();
    let mut i = 0;
    while i < pre.len() && !words.is_empty() && clean(words[0]) == pre[i] {
        words.remove(0);
        i += 1;
    }
    let mut j = post.len();
    while j > 0 && !words.is_empty() && clean(words[words.len() - 1]) == post[j - 1] {
        words.pop();
        j -= 1;
    }
    words
        .iter()
        .map(|w| {
            w.trim_matches(|c: char| !c.is_alphanumeric() && c != '-')
                .to_string()
        })
        .filter(|w| !w.is_empty())
        .collect()
}

/// Says `word` with each voice and returns the heard forms worth fixing.
fn hear_test(app: &AppHandle, word: &str, langs: &[String]) -> Result<Vec<String>> {
    let tm = Arc::clone(&app.state::<Arc<TranscriptionManager>>());
    let mut heard_forms = Vec::new();
    for (i, (voice, sentence)) in HEAR.iter().enumerate() {
        let wav = std::env::temp_dir().join(format!("sokki-hear-{}-{i}.wav", std::process::id()));
        let status = std::process::Command::new("/usr/bin/say")
            .arg("-v")
            .arg(voice)
            .arg("--data-format=LEI16@16000")
            .arg("-o")
            .arg(&wav)
            .arg(sentence.replace("{}", word))
            .status()
            .context("say")?;
        if !status.success() {
            continue;
        }
        let samples = crate::audio_toolkit::read_wav_samples(&wav);
        let _ = std::fs::remove_file(&wav);
        let text = tm.transcribe(samples?)?;
        let heard = slot(&text, sentence);
        if heard.is_empty() || heard.len() > 3 {
            continue;
        }
        let joined = heard.join(" ");
        if key(&joined) == key(word) {
            continue;
        }
        // All normal words ("get up", "Lied"): fixing them would overwrite speech.
        if heard.iter().all(|w| known(w, langs)) {
            continue;
        }
        if !heard_forms.iter().any(|h: &String| key(h) == key(&joined)) {
            heard_forms.push(joined);
        }
    }
    Ok(heard_forms)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_slot_of_a_hear_test() {
        assert_eq!(
            slot("Ich habe heute mit Fassel gesprochen.", HEAR[0].1),
            ["Fassel"]
        );
        assert_eq!(slot("I talked to get up today.", HEAR[1].1), ["get", "up"]);
        assert_eq!(slot("I talked to Off-Dar today", HEAR[1].1), ["Off-Dar"]);
        // The model wrote the sentence differently: what's left is the slot.
        assert_eq!(
            slot("Ich habe heute Vercel gesprochen.", HEAR[0].1),
            ["Vercel"]
        );
    }
}

/// Vocabulary › Learned: remove a word; it is never learned again.
#[tauri::command]
#[specta::specta]
pub fn vocabulary_forget_learned(app: AppHandle, word: String) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.vocabulary_learned
        .retain(|w| !w.eq_ignore_ascii_case(&word));
    s.vocabulary_heard_as
        .retain(|h| !h.word.eq_ignore_ascii_case(&word));
    let lower = word.to_lowercase();
    if !s.vocabulary_rejected.contains(&lower) {
        s.vocabulary_rejected.push(lower);
    }
    settings::write_settings(&app, s);
    Ok(())
}

/// Vocabulary › Heard as: remove one pair; that heard form is never learned again.
#[tauri::command]
#[specta::specta]
pub fn vocabulary_forget_heard_as(app: AppHandle, heard: String) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.vocabulary_heard_as
        .retain(|h| key(&h.heard) != key(&heard));
    let lower = heard.to_lowercase();
    if !s.vocabulary_rejected.contains(&lower) {
        s.vocabulary_rejected.push(lower);
    }
    settings::write_settings(&app, s);
    Ok(())
}

#[tauri::command]
#[specta::specta]
pub fn change_learn_words_setting(app: AppHandle, enabled: bool) -> Result<(), String> {
    let mut s = settings::get_settings(&app);
    s.learn_words = enabled;
    settings::write_settings(&app, s);
    Ok(())
}
