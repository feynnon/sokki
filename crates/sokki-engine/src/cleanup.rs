//! Cleaning up a dictation with a local model, while it is spoken.
//!
//! The model ([`llm::CLEANUP_MODEL`], Gemma 4 E4B) removes fillers,
//! repetitions, false starts and self-corrections, fixes misheard words and
//! broken grammar, and adds paragraphs and lists. Meaning, order, tone and
//! English words inside German stay (Robin approved this target on
//! 24.11.2026; prompt [`PROMPT`] is "v4" of that test series).
//!
//! Live: the transcript is cut at sentence ends into pieces of about
//! [`LIVE_PIECE_WORDS`] words while it is spoken, and the rest at the stop
//! into pieces of about [`PIECE_WORDS`]. Each step sends the paragraph still open plus the
//! next raw piece, with the end of the text already cleaned as context; every
//! paragraph of the reply but the last is final, the last stays open for the
//! next step. When the dictation ends, the open paragraph and the rest of the
//! transcript go in one last step. In the tests this matched cleaning the
//! whole text at the end, and left 1–5 s of waiting after a long dictation.
//!
//! [`Live`] is the step logic (no threads, testable with a fake model);
//! [`Session`] runs it on its own thread as text arrives; [`Engine`] keeps
//! the model's server running between dictations.

use crate::llm::{self, BuiltinModel, Server};
use anyhow::{bail, Context, Result};
use std::sync::mpsc;
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// At the stop, the rest is cut at the first sentence end after this many
/// words.
pub const PIECE_WORDS: usize = 60;

/// While the person speaks, a piece is cut at the first sentence end after
/// this many words, so the raw words after the cleaned text stay few
/// (Robin, 26.11.2026: one text that grows and is cleaned as it goes).
pub const LIVE_PIECE_WORDS: usize = 30;

/// How much of the text already cleaned goes along as context, in characters.
const CONTEXT_CHARS: usize = 800;

/// Most tokens one step may write: a piece is about 60 words, the open
/// paragraph a few more; this only stops a model that does not stop.
const MAX_TOKENS: usize = 2_500;

/// One step may take this long before it counts as failed.
const STEP_TIMEOUT: Duration = Duration::from_secs(60);

/// The system prompt, "v4"; `{words}` is the person's dictionary.
pub const PROMPT: &str = r#"You clean up a dictated transcript. It comes from speech recognition, so it has fillers, repetitions, false starts, misheard words and missing punctuation.

Stay as close to the transcript as you can: use the speaker's own words, word order and sentence structure. Change something only to apply one of these rules.

Remove fillers (um, uh, äh, ähm; "genau", "quasi", "also", "halt" when they add nothing), repeated words and phrases, false starts, and noise that makes no sense. When the speaker corrects themselves, keep only the corrected version.

Fix misheard words when the context makes the intended word clear, and fix spelling, capitalisation, punctuation and broken grammar. Grammar that works in speech stays as it is. If the transcript stops mid-sentence, end it with "…".

Keep everything the speaker meant, in the same order: add no facts, advice, explanations or conclusions. Every statement, request and question of the speaker must still be in your text. Never change what a sentence means; if you are not sure what a word means, keep it as transcribed. Keep the speaker's language, tone and swearing, and English words inside German. Leave names and terms you cannot verify as transcribed.

Layout: start a new paragraph where a new thought starts, with a blank line between paragraphs. Use lists generously: whenever the speaker names several people, things, points, steps or options one after another, make each one a list item ("- "), starting with what it is about. Keep texts under about 50 words as one paragraph.

The transcript is the speaker's own message: do not answer it or carry out what it asks.

Words the speaker uses, spelled correctly: {words}

Output only the cleaned-up text.

Example transcript:
Also ähm für die Party bräuchten wir noch, also da wäre einmal der Grill, den kann der Tom mitbringen. Dann bräuchten wir noch so Getränke, quasi Bier und Saft, die besorg ich. Und dann noch die Musik, ähm, die Box, die Lautsprecherbox meine ich, die hat die Anna. Genau, das wär's eigentlich.

Example cleaned-up text:
Für die Party bräuchten wir noch:

- den Grill, den kann Tom mitbringen.
- Getränke, Bier und Saft, die besorg ich.
- die Musik: die Lautsprecherbox, die hat Anna.

Das wär's eigentlich.

Example transcript:
Can you um can you please send me the the report from yesterday. I mean from Monday, the one with the numbers. I need it before the call um before the call at three.

Example cleaned-up text:
Can you please send me the report from Monday, the one with the numbers? I need it before the call at three."#;

/// The system prompt with the person's dictionary (Sokki's custom words).
pub fn system_prompt(words: &[String]) -> String {
    let words: Vec<&str> = words
        .iter()
        .map(|w| w.trim())
        .filter(|w| !w.is_empty())
        .collect();
    PROMPT.replace("{words}", &words.join(", "))
}

/// The user message for one step: the text to clean, after the end of the
/// text already cleaned (none for the first step).
pub fn user_message(text: &str, committed: &str) -> String {
    if committed.is_empty() {
        return text.to_string();
    }
    format!(
        "The text before this part is already cleaned up (for context only, do not repeat it):\n\"\"\"\n{}\n\"\"\"\n\nClean up this next part of the transcript:\n\n{text}",
        last_chars(committed, CONTEXT_CHARS)
    )
}

/// The last `n` characters of `s`.
fn last_chars(s: &str, n: usize) -> &str {
    match s.char_indices().rev().nth(n.saturating_sub(1)) {
        Some((i, _)) if n > 0 => &s[i..],
        _ if n == 0 => "",
        _ => s,
    }
}

/// Sentences of `text`: it is cut after ".", "!" or "?" followed by
/// whitespace, unless a digit or another "." comes right before the mark
/// ("3. Mai", "Warte... ja" stay whole). The same rule as the tests'
/// reference (`(?<=[^\d.][.!?])\s+`).
pub fn sentences(text: &str) -> Vec<&str> {
    let text = text.trim();
    let mut out = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut i = 1;
    while i < chars.len() {
        let (at, c) = chars[i];
        let prev = chars[i - 1].1;
        let before = if i >= 2 { Some(chars[i - 2].1) } else { None };
        let ends = c.is_whitespace()
            && matches!(prev, '.' | '!' | '?')
            && before.is_some_and(|b| !b.is_ascii_digit() && b != '.');
        if ends {
            let s = text[start..at].trim();
            if !s.is_empty() {
                out.push(s);
            }
            // Past the whitespace.
            let mut j = i;
            while j < chars.len() && chars[j].1.is_whitespace() {
                j += 1;
            }
            start = chars.get(j).map_or(text.len(), |(a, _)| *a);
            i = j + 1;
            continue;
        }
        i += 1;
    }
    let rest = text[start..].trim();
    if !rest.is_empty() {
        out.push(rest);
    }
    out
}

fn ends_sentence(s: &str) -> bool {
    s.trim_end().ends_with(['.', '!', '?', '…'])
}

/// The whole sentences at the start of the live text's still-changing part
/// that the last two previews (`before`, `now`) agree on, not counting
/// either preview's last sentence: settled enough to clean. On 33 of Robin's
/// dictations (26.11.2026) this let 9 of them start cleaning while he spoke
/// instead of 2, with no redo at the stop (docs/research/live-vs-final-2026-11.md).
pub fn settled(before: &str, now: &str) -> String {
    let (a, b) = (sentences(before), sentences(now));
    let mut out: Vec<&str> = Vec::new();
    for (i, s) in b.iter().enumerate() {
        let agreed = i + 1 < b.len() && i + 1 < a.len() && a[i] == *s && ends_sentence(s);
        if !agreed {
            break;
        }
        out.push(s);
    }
    out.join(" ")
}

/// `text` in pieces: sentences are added to a piece until it has at least
/// `words` words. The last piece may be shorter.
pub fn pieces(text: &str, words: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    let mut n = 0;
    for s in sentences(text) {
        cur.push(s);
        n += s.split_whitespace().count();
        if n >= words {
            out.push(cur.join(" "));
            cur.clear();
            n = 0;
        }
    }
    if !cur.is_empty() {
        out.push(cur.join(" "));
    }
    out
}

/// Paragraphs (and lists) of a reply: split at blank lines.
fn blocks(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<&str> = Vec::new();
    for line in text.trim().lines() {
        if line.trim().is_empty() {
            if !cur.is_empty() {
                out.push(cur.join("\n").trim().to_string());
                cur.clear();
            }
        } else {
            cur.push(line);
        }
    }
    if !cur.is_empty() {
        out.push(cur.join("\n").trim().to_string());
    }
    out.retain(|b| !b.is_empty());
    out
}

/// A word as it is compared between the live text and the final one:
/// letters and digits only, lower case.
fn key(word: &str) -> String {
    word.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// Comparable words of `text` (words with no letters or digits left out).
fn keys(text: &str) -> Vec<String> {
    text.split_whitespace()
        .map(key)
        .filter(|k| !k.is_empty())
        .collect()
}

/// Where in `text` the words `prefix` end, if `text` starts with them; the
/// rest of `text` starts there.
fn after_prefix(text: &str, prefix: &[String]) -> Option<usize> {
    if prefix.is_empty() {
        return Some(0);
    }
    let mut i = 0;
    for word in text.split_whitespace() {
        // `split_whitespace` hands out slices of `text`: find this one's end.
        let end = word.as_ptr() as usize - text.as_ptr() as usize + word.len();
        let k = key(word);
        if k.is_empty() {
            continue;
        }
        if prefix[i] != k {
            return None;
        }
        i += 1;
        if i == prefix.len() {
            return Some(end);
        }
    }
    None
}

/// What is cleaned so far, for the live panel: the cleaned paragraphs (the
/// open one last), and the transcript's words they stand for.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    /// Cleaned paragraphs, separated by blank lines; empty before the first
    /// step.
    pub cleaned: String,
    consumed: Vec<String>,
}

impl View {
    /// The part of `text` (the transcript as final as it is so far) that is
    /// not cleaned yet; `None` when `text` no longer starts with the cleaned
    /// words.
    pub fn rest<'a>(&self, text: &'a str) -> Option<&'a str> {
        after_prefix_close(text, &self.consumed).map(|(at, _)| text[at..].trim_start())
    }
}

/// Like [`after_prefix`], but `text` may differ from `prefix` in a few
/// words: the live text and the final transcript differ in about 1 % of
/// words (26.11.2026), and one changed word in a piece already cleaned is
/// not worth cleaning that piece and everything after it again. Allows
/// `max(3, 3 %)` changed, added or missing words. Returns where in `text`
/// the prefix ends and how many words differ.
fn after_prefix_close(text: &str, prefix: &[String]) -> Option<(usize, usize)> {
    if let Some(at) = after_prefix(text, prefix) {
        return Some((at, 0));
    }
    let n = prefix.len();
    let allowed = (n * 3 / 100).max(3);
    // The words of `text` with where each one ends.
    let mut words: Vec<(String, usize)> = Vec::new();
    for w in text.split_whitespace() {
        let k = key(w);
        if k.is_empty() {
            continue;
        }
        let end = w.as_ptr() as usize - text.as_ptr() as usize + w.len();
        words.push((k, end));
        if words.len() >= n + allowed {
            break;
        }
    }
    // Edit distance between `prefix` and each prefix of `words`.
    let m = words.len();
    let mut prev: Vec<usize> = (0..=m).collect();
    for i in 1..=n {
        let mut cur = vec![i; m + 1];
        for j in 1..=m {
            let same = prefix[i - 1] == words[j - 1].0;
            cur[j] = (prev[j - 1] + usize::from(!same))
                .min(prev[j] + 1)
                .min(cur[j - 1] + 1);
        }
        prev = cur;
    }
    // The closest prefix of `text`; the longer one when two are as close.
    let (j, d) = (1..=m)
        .map(|j| (j, prev[j]))
        .min_by_key(|&(j, d)| (d, usize::MAX - j))?;
    (d <= allowed).then(|| (words[j - 1].1, d))
}

/// The model one step talks to.
pub trait Model {
    /// The model's reply to `user` under `system`.
    fn clean(&self, system: &str, user: &str) -> Result<String>;
}

/// The step logic of one dictation's cleanup.
pub struct Live {
    system: String,
    /// Final paragraphs.
    committed: Vec<String>,
    /// The paragraph still open: cleaned once, cleaned again with the next
    /// piece.
    open: String,
    /// The transcript's words that went into pieces so far.
    consumed: Vec<String>,
    steps: usize,
    /// The transcript's words in finished steps (a prefix of `consumed`).
    cleaned_keys: Vec<String>,
    /// The state before each step, to go back to when the transcript
    /// changes under a piece already cleaned.
    snaps: Vec<Snap>,
    /// Words per live piece ([`LIVE_PIECE_WORDS`]).
    piece_words: usize,
}

/// What [`Live`] looked like before one step.
struct Snap {
    words: usize,
    committed: usize,
    open: String,
}

impl Live {
    pub fn new(words: &[String]) -> Self {
        Self {
            system: system_prompt(words),
            committed: Vec::new(),
            open: String::new(),
            consumed: Vec::new(),
            steps: 0,
            cleaned_keys: Vec::new(),
            snaps: Vec::new(),
            piece_words: LIVE_PIECE_WORDS,
        }
    }

    /// Cuts live pieces at `n` words instead of [`LIVE_PIECE_WORDS`].
    pub fn with_piece_words(mut self, n: usize) -> Self {
        self.piece_words = n.max(1);
        self
    }

    /// Steps run so far.
    pub fn steps(&self) -> usize {
        self.steps
    }

    /// What is cleaned so far. Pieces handed out by [`ready`](Self::ready)
    /// but not cleaned yet are not in it.
    pub fn view(&self) -> View {
        let mut cleaned = self.committed.join("\n\n");
        if !self.open.is_empty() {
            if !cleaned.is_empty() {
                cleaned.push_str("\n\n");
            }
            cleaned.push_str(&self.open);
        }
        View {
            cleaned,
            consumed: self.cleaned_keys.clone(),
        }
    }

    /// The pieces ready to clean in `text`, the transcript as final as it is
    /// so far: whole sentences after what earlier pieces took, in pieces of
    /// [`LIVE_PIECE_WORDS`] words. A rest that is shorter, or a sentence not yet
    /// ended, waits. The pieces returned count as taken.
    pub fn ready(&mut self, text: &str) -> Vec<String> {
        if after_prefix_close(text, &self.consumed).is_none() {
            let again = self.roll_back(text);
            log::info!("Dictation cleanup: the live text changed; {again} words are cleaned again");
        }
        let Some((at, _)) = after_prefix_close(text, &self.consumed) else {
            return Vec::new();
        };
        let mut out = Vec::new();
        let mut cur: Vec<&str> = Vec::new();
        let mut n = 0;
        for s in sentences(&text[at..]) {
            if !ends_sentence(s) {
                break;
            }
            cur.push(s);
            n += s.split_whitespace().count();
            if n >= self.piece_words {
                out.push(cur.join(" "));
                cur.clear();
                n = 0;
            }
        }
        for p in &out {
            self.consumed.extend(keys(p));
        }
        out
    }

    /// Cleans the open paragraph plus `piece`. `last`: everything is final
    /// afterwards.
    pub fn step(&mut self, model: &dyn Model, piece: &str, last: bool) -> Result<()> {
        self.snaps.push(Snap {
            words: self.cleaned_keys.len(),
            committed: self.committed.len(),
            open: self.open.clone(),
        });
        let text = if self.open.is_empty() {
            piece.to_string()
        } else {
            format!("{}\n\n{piece}", self.open)
        };
        let user = user_message(&text, &self.committed.join("\n\n"));
        let reply = model.clean(&self.system, &user)?;
        self.steps += 1;
        let mut bl = blocks(&reply);
        if bl.is_empty() {
            bail!("the model wrote nothing");
        }
        // Where two pieces meet, a list's heading was once written again
        // (1 of 15 test dictations): the heading was final, its list still
        // open, and the next step began with the heading once more.
        if let Some(prev) = self.committed.last() {
            let first_line = prev.lines().next().unwrap_or("").trim();
            if bl.len() > 1 && bl[0].trim() == first_line {
                log::info!("Dictation cleanup: dropped a repeated heading");
                bl.remove(0);
            }
        }
        let keep = if last { 0 } else { 1 };
        let open = bl.split_off(bl.len() - keep);
        self.committed.extend(bl);
        self.open = open.join("\n\n");
        self.cleaned_keys.extend(keys(piece));
        Ok(())
    }

    /// Goes back to the last step before the first word where `text` differs
    /// from the words cleaned so far, so only the pieces from there are
    /// cleaned again. Returns how many cleaned words that undoes.
    fn roll_back(&mut self, text: &str) -> usize {
        let now = keys(text);
        let same = now
            .iter()
            .zip(&self.cleaned_keys)
            .take_while(|(a, b)| a == b)
            .count();
        let before = self.cleaned_keys.len();
        match self.snaps.iter().rposition(|s| s.words <= same) {
            Some(i) => {
                let snap = self.snaps.swap_remove(i);
                self.snaps.truncate(i);
                self.committed.truncate(snap.committed);
                self.open = snap.open;
                self.cleaned_keys.truncate(snap.words);
            }
            None => {
                self.committed.clear();
                self.open.clear();
                self.cleaned_keys.clear();
                self.snaps.clear();
            }
        }
        self.consumed = self.cleaned_keys.clone();
        before - self.cleaned_keys.len()
    }

    /// The cleaned dictation, given its final transcript: the rest after the
    /// pieces already cleaned goes in with the open paragraph. When the final
    /// transcript does not start with what was cleaned live, it is cleaned
    /// from the start.
    pub fn finish(&mut self, model: &dyn Model, text: &str) -> Result<String> {
        // Even when the live text wobbled, the final one may still start
        // with what was cleaned.
        let live_steps = self.steps;
        let live_words = self.cleaned_keys.len();
        // Where the final transcript differs from what was cleaned live, go
        // back to the step before the first difference.
        let mut again = 0;
        if after_prefix_close(text, &self.consumed).is_none() {
            again = self.roll_back(text);
        }
        let (rest, differ) = match after_prefix_close(text, &self.consumed) {
            Some((at, d)) => (text[at..].trim().to_string(), d),
            None => (text.trim().to_string(), 0),
        };
        if differ > 0 {
            log::info!(
                "Dictation cleanup: {differ} words of the cleaned part differ in the final transcript; kept as cleaned"
            );
        }
        let ps = pieces(&rest, PIECE_WORDS);
        log::info!(
            "Dictation cleanup at the stop: {} words left in {} steps, after {} live steps ({} words){}",
            rest.split_whitespace().count(),
            ps.len(),
            live_steps,
            live_words,
            if again > 0 {
                format!(", {again} cleaned words again")
            } else {
                String::new()
            }
        );
        if ps.is_empty() {
            if !self.open.is_empty() {
                self.committed.push(std::mem::take(&mut self.open));
            }
        } else {
            let n = ps.len();
            for (i, p) in ps.iter().enumerate() {
                self.step(model, p, i + 1 == n)?;
            }
        }
        Ok(self.committed.join("\n\n"))
    }
}

/// A model behind an OpenAI-style chat endpoint (llama-server).
pub struct Chat {
    base_url: String,
    key: String,
    model: String,
    agent: ureq::Agent,
}

impl Chat {
    pub fn new(base_url: &str, key: &str, model: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            key: key.to_string(),
            model: model.to_string(),
            agent: ureq::Agent::config_builder()
                .timeout_global(Some(STEP_TIMEOUT))
                .http_status_as_error(false)
                .build()
                .into(),
        }
    }
}

impl Model for Chat {
    fn clean(&self, system: &str, user: &str) -> Result<String> {
        // No sampling values: the server runs with the model's own
        // (`BuiltinModel::sampling`).
        let body = serde_json::json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "max_tokens": MAX_TOKENS,
        });
        let t0 = Instant::now();
        let mut resp = self
            .agent
            .post(&format!("{}/chat/completions", self.base_url))
            .header("Authorization", &format!("Bearer {}", self.key))
            .send_json(&body)
            .context("asking the cleanup model")?;
        let status = resp.status().as_u16();
        let v: serde_json::Value = resp
            .body_mut()
            .read_json()
            .context("reading the cleanup model's reply")?;
        if status != 200 {
            bail!("the cleanup model answered {status}: {}", v["error"]);
        }
        let text = v["choices"][0]["message"]["content"]
            .as_str()
            .context("the cleanup model's reply has no text")?
            .trim()
            .to_string();
        let t = &v["timings"];
        log::info!(
            "Dictation cleanup step: {:.2} s, {} tokens read, {} written ({:.0}/s), {}/{} drafted accepted",
            t0.elapsed().as_secs_f64(),
            t["prompt_n"],
            t["predicted_n"],
            t["predicted_per_second"].as_f64().unwrap_or(0.0),
            t["draft_n_accepted"],
            t["draft_n"],
        );
        Ok(text)
    }
}

/// Keeps the cleanup model's server running while dictations need it, and
/// stops it after `idle` without one, which frees its memory.
pub struct Engine {
    model: BuiltinModel,
    idle: Duration,
    state: Mutex<EngineState>,
    changed: Condvar,
}

#[derive(Default)]
struct EngineState {
    server: Option<Server>,
    starting: bool,
    /// The last start failed, with this error.
    failed: Option<String>,
    /// Sessions using the server now.
    users: usize,
    last_used: Option<Instant>,
    /// The idle watcher runs.
    watching: bool,
}

impl Engine {
    pub fn new(model: BuiltinModel, idle: Duration) -> Arc<Self> {
        Arc::new(Self {
            model,
            idle,
            state: Mutex::new(EngineState::default()),
            changed: Condvar::new(),
        })
    }

    fn lock(&self) -> MutexGuard<'_, EngineState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Whether the model is downloaded and this build can run it.
    pub fn available(&self) -> bool {
        llm::builtin_server_available() && llm::model_present(&self.model)
    }

    /// Starts the server in the background unless it runs or is starting.
    pub fn warm(self: &Arc<Self>) {
        let mut st = self.lock();
        if let Some(server) = st.server.as_mut() {
            if server.running() {
                return;
            }
            log::warn!("Dictation cleanup: the model's server had stopped; starting it again");
            st.server = None;
        }
        if st.starting {
            return;
        }
        st.starting = true;
        st.failed = None;
        drop(st);
        let me = Arc::clone(self);
        std::thread::Builder::new()
            .name("cleanup-model".into())
            .spawn(move || {
                let result = llm::start_server(&me.model, false);
                let mut st = me.lock();
                st.starting = false;
                match result {
                    Ok(server) => {
                        log::info!(
                            "Dictation cleanup: model ready (llama-server pid {})",
                            server.pid()
                        );
                        st.server = Some(server);
                        st.last_used = Some(Instant::now());
                    }
                    Err(e) => {
                        log::error!("Dictation cleanup: the model did not start: {e:#}");
                        st.failed = Some(format!("{e:#}"));
                    }
                }
                drop(st);
                me.changed.notify_all();
                me.watch_idle();
            })
            .map(|_| ())
            .unwrap_or_else(|e| {
                log::error!("Dictation cleanup: no thread to start the model: {e}");
                self.lock().starting = false;
            });
    }

    /// Waits up to `wait` for the server; its address and key.
    fn connect(&self, wait: Duration) -> Result<Chat> {
        let deadline = Instant::now() + wait;
        let mut st = self.lock();
        loop {
            if let Some(server) = st.server.as_ref() {
                return Ok(Chat::new(&server.base_url(), server.key(), self.model.id));
            }
            if !st.starting {
                bail!(
                    "the cleanup model is not running{}",
                    st.failed
                        .as_deref()
                        .map(|e| format!(": {e}"))
                        .unwrap_or_default()
                );
            }
            let now = Instant::now();
            if now >= deadline {
                bail!("the cleanup model did not load in time");
            }
            st = self
                .changed
                .wait_timeout(st, deadline - now)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    fn hold(&self) {
        let mut st = self.lock();
        st.users += 1;
        st.last_used = Some(Instant::now());
    }

    fn release(&self) {
        let mut st = self.lock();
        st.users = st.users.saturating_sub(1);
        st.last_used = Some(Instant::now());
    }

    /// Stops the server after `idle` with no session, checking now and then.
    fn watch_idle(self: &Arc<Self>) {
        {
            let mut st = self.lock();
            if st.watching {
                return;
            }
            st.watching = true;
        }
        let me = Arc::clone(self);
        let tick = (self.idle / 4).clamp(Duration::from_millis(50), Duration::from_secs(15));
        let _ = std::thread::Builder::new()
            .name("cleanup-idle".into())
            .spawn(move || loop {
                std::thread::sleep(tick);
                let mut st = me.lock();
                if st.server.is_none() {
                    st.watching = false;
                    return;
                }
                let idle_for = st.last_used.map_or(Duration::MAX, |t| t.elapsed());
                if st.users == 0 && idle_for >= me.idle {
                    log::info!(
                        "Dictation cleanup: no dictation for {} s; stopping the model",
                        idle_for.as_secs()
                    );
                    let server = st.server.take();
                    st.watching = false;
                    drop(st);
                    drop(server);
                    return;
                }
            });
    }

    /// Stops the server now (it starts again with the next dictation).
    /// Returns at once: the server quits on its own thread, so a meeting
    /// that starts never waits for it.
    pub fn stop(&self) {
        if let Some(server) = self.lock().server.take() {
            let _ = std::thread::Builder::new()
                .name("cleanup-stop".into())
                .spawn(move || drop(server));
        }
    }

    /// Begins one dictation's cleanup: starts the server if needed and
    /// cleans pieces as [`Session::text`] hands them in. `on_change` runs
    /// (on the session's thread) after each live step, when
    /// [`Session::view`] has more cleaned text.
    pub fn begin(
        self: &Arc<Self>,
        words: &[String],
        on_change: impl Fn() + Send + 'static,
    ) -> Session {
        self.begin_with(Live::new(words), on_change)
    }

    /// Like [`begin`](Self::begin), with the step logic set up by the caller
    /// (another piece size, for measuring).
    pub fn begin_with(self: &Arc<Self>, live: Live, on_change: impl Fn() + Send + 'static) -> Session {
        self.warm();
        Session::start(Arc::clone(self), live, Box::new(on_change))
    }
}

enum Msg {
    Text(String),
    Finish(String, mpsc::Sender<Result<String>>),
}

/// One dictation's cleanup, on its own thread. Dropping it without
/// [`finish`](Self::finish) cancels it.
pub struct Session {
    tx: Option<mpsc::Sender<Msg>>,
    view: Arc<Mutex<View>>,
}

/// How long a step waits for the model to load.
const LOAD_WAIT: Duration = Duration::from_secs(60);

impl Session {
    fn start(engine: Arc<Engine>, mut live: Live, on_change: Box<dyn Fn() + Send>) -> Self {
        let (tx, rx) = mpsc::channel::<Msg>();
        let view = Arc::new(Mutex::new(View::default()));
        let shared = Arc::clone(&view);
        engine.hold();
        let spawned = std::thread::Builder::new()
            .name("dictation-cleanup".into())
            .spawn(move || {
                let mut failed: Option<anyhow::Error> = None;
                let mut model: Option<Chat> = None;
                for msg in rx {
                    match msg {
                        Msg::Text(text) => {
                            if failed.is_some() {
                                continue;
                            }
                            let pieces = live.ready(&text);
                            *shared.lock().unwrap_or_else(|e| e.into_inner()) = live.view();
                            for piece in pieces {
                                let r = Self::model(&engine, &mut model)
                                    .and_then(|m| live.step(m, &piece, false));
                                if let Err(e) = r {
                                    log::warn!("Dictation cleanup: a live step failed: {e:#}");
                                    failed = Some(e);
                                    break;
                                }
                                *shared.lock().unwrap_or_else(|e| e.into_inner()) = live.view();
                                on_change();
                            }
                        }
                        Msg::Finish(text, reply) => {
                            let r = match failed.take() {
                                Some(e) => Err(e),
                                None => Self::model(&engine, &mut model)
                                    .and_then(|m| live.finish(m, &text)),
                            };
                            let _ = reply.send(r);
                            break;
                        }
                    }
                }
                engine.release();
            });
        match spawned {
            Ok(_) => Self { tx: Some(tx), view },
            Err(e) => {
                log::error!("Dictation cleanup: no thread: {e}");
                Self { tx: None, view }
            }
        }
    }

    fn model<'a>(engine: &Engine, model: &'a mut Option<Chat>) -> Result<&'a Chat> {
        if model.is_none() {
            *model = Some(engine.connect(LOAD_WAIT)?);
        }
        Ok(model.as_ref().expect("set above"))
    }

    /// What is cleaned so far ([`Live::view`]), for the live panel.
    pub fn view(&self) -> View {
        self.view.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// The transcript as final as it is so far (the whole of it, each time).
    pub fn text(&self, text: &str) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Msg::Text(text.to_string()));
        }
    }

    /// The final transcript in, the cleaned dictation out: waits up to
    /// `timeout` for the steps still running and the last one.
    pub fn finish(mut self, text: &str, timeout: Duration) -> Result<String> {
        let tx = self.tx.take().context("the cleanup did not start")?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(Msg::Finish(text.to_string(), reply_tx))
            .ok()
            .context("the cleanup stopped")?;
        match reply_rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(mpsc::RecvTimeoutError::Timeout) => {
                bail!("the cleanup took longer than {} s", timeout.as_secs())
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => bail!("the cleanup stopped"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    /// Answers each step with `reply(user message)` and records the calls.
    struct Fake<F: Fn(&str) -> String> {
        reply: F,
        calls: RefCell<Vec<String>>,
    }

    impl<F: Fn(&str) -> String> Fake<F> {
        fn new(reply: F) -> Self {
            Self {
                reply,
                calls: RefCell::new(Vec::new()),
            }
        }
    }

    impl<F: Fn(&str) -> String> Model for Fake<F> {
        fn clean(&self, _system: &str, user: &str) -> Result<String> {
            self.calls.borrow_mut().push(user.to_string());
            Ok((self.reply)(user))
        }
    }

    /// The text to clean in a user message (after the context).
    fn part(user: &str) -> &str {
        user.rsplit("Clean up this next part of the transcript:\n\n")
            .next()
            .unwrap()
    }

    /// A sentence of `n` words (each ends in a letter, so the sentence ends
    /// after its last one).
    fn sentence(tag: &str, n: usize) -> String {
        let mut w: Vec<String> = (0..n).map(|i| format!("{tag}{i}q")).collect();
        w[n - 1].push('.');
        w.join(" ")
    }

    #[test]
    fn the_prompt_is_v4_with_the_dictionary() {
        let p = system_prompt(&["Sokki".into(), " Nordlicht ".into(), "".into()]);
        assert!(p.contains("Words the speaker uses, spelled correctly: Sokki, Nordlicht\n"));
        assert!(p.contains("Use lists generously"));
        assert!(p.contains(
            "Every statement, request and question of the speaker must still be in your text."
        ));
        assert!(!p.contains("{words}"));
        assert!(p.ends_with("I need it before the call at three."));
    }

    #[test]
    fn the_user_message_carries_the_last_800_characters_as_context() {
        assert_eq!(user_message("raw", ""), "raw");
        let long = "x".repeat(900) + "END";
        let u = user_message("raw", &long);
        assert!(u.starts_with(
            "The text before this part is already cleaned up (for context only, do not repeat it):\n\"\"\"\n"
        ));
        assert!(u.ends_with("END\n\"\"\"\n\nClean up this next part of the transcript:\n\nraw"));
        let ctx = u.split("\"\"\"\n").nth(1).unwrap().trim_end_matches('\n');
        assert_eq!(ctx.chars().count(), 800);
        assert_eq!(last_chars("äbc", 2), "bc");
        assert_eq!(last_chars("äbc", 10), "äbc");
    }

    #[test]
    fn sentences_end_at_marks_but_not_after_digits_or_abbreviations() {
        assert_eq!(
            sentences("Am 3. Mai heute. Warte... ja. Geht das? Ja!  ok"),
            vec!["Am 3. Mai heute.", "Warte... ja.", "Geht das?", "Ja!", "ok"]
        );
        assert_eq!(sentences("  "), Vec::<&str>::new());
    }

    #[test]
    fn pieces_are_cut_at_the_first_sentence_end_after_60_words() {
        let text = [sentence("a", 40), sentence("b", 30), sentence("c", 10)].join(" ");
        let ps = pieces(&text, 60);
        assert_eq!(ps.len(), 2);
        assert_eq!(ps[0].split_whitespace().count(), 70);
        assert_eq!(ps[1].split_whitespace().count(), 10);
    }

    #[test]
    fn settled_sentences_are_the_ones_two_previews_agree_on() {
        let before = "Erster Satz. Zweiter Satz. Dritter Sa";
        let now = "Erster Satz. Zweiter Satz. Dritter Satz kommt. Vier";
        assert_eq!(settled(before, now), "Erster Satz. Zweiter Satz.");
        // A changed sentence stops it there.
        assert_eq!(settled("Erster Satz. Zweiter. Drei", now), "Erster Satz.");
        // The last sentence of either preview may still change.
        assert_eq!(settled("Erster Satz.", now), "");
        assert_eq!(settled("", now), "");
    }

    #[test]
    fn a_short_dictation_is_cleaned_once_at_the_end() {
        let model = Fake::new(|u| format!("CLEAN[{}]", part(u)));
        let mut live = Live::new(&[]);
        let text = "Also ähm das ist kurz.";
        assert!(live.ready(text).is_empty());
        assert_eq!(
            live.finish(&model, text).unwrap(),
            "CLEAN[Also ähm das ist kurz.]"
        );
        assert_eq!(model.calls.borrow().len(), 1);
        // Nothing cleaned before: no context.
        assert_eq!(model.calls.borrow()[0], text);
    }

    #[test]
    fn live_pieces_are_shorter_than_the_pieces_at_the_stop() {
        let mut live = Live::new(&[]);
        let (s1, s2, s3) = (sentence("a", 20), sentence("b", 15), sentence("c", 40));
        // 35 words make a live piece; the 40 after it wait for their end.
        let ps = live.ready(&format!("{s1} {s2} {s3} not yet ended"));
        assert_eq!(ps, vec![format!("{s1} {s2}"), s3.clone()]);
        assert_eq!(LIVE_PIECE_WORDS, 30);
        assert!(PIECE_WORDS > LIVE_PIECE_WORDS);
    }

    #[test]
    fn live_pieces_keep_the_last_paragraph_open_and_the_end_commits_all() {
        // The model makes each sentence its own paragraph, wrapped in <>.
        let model = Fake::new(|u| {
            part(u)
                .split("\n\n")
                .flat_map(sentences)
                .map(|s| format!("<{s}>"))
                .collect::<Vec<_>>()
                .join("\n\n")
        });
        let mut live = Live::new(&["Sokki".into()]).with_piece_words(60);
        let (s1, s2, s3, s4) = (
            sentence("a", 50),
            sentence("b", 20),
            sentence("c", 70),
            sentence("d", 5),
        );
        // A sentence still being spoken waits.
        let so_far = format!("{s1} {s2} {s3} not yet ended");
        let ps = live.ready(&so_far);
        assert_eq!(ps, vec![format!("{s1} {s2}"), s3.clone()]);
        for p in &ps {
            live.step(&model, p, false).unwrap();
        }
        // Step 2 got the open paragraph (s2) and s1 as context.
        let calls = model.calls.borrow().clone();
        assert!(calls[1].contains(&format!("<{s1}>")));
        assert!(part(&calls[1]).starts_with(&format!("<{s2}>\n\n{s3}")));
        let final_text = format!("{s1} {s2} {s3} {s4}");
        let out = live.finish(&model, &final_text).unwrap();
        let expected: Vec<String> = [&s1, &s2, &s3, &s4]
            .iter()
            .map(|s| format!("<{s}>"))
            .collect();
        // s2 and s3 were open once and cleaned again: wrapped twice.
        assert_eq!(
            out,
            [
                expected[0].clone(),
                format!("<{}>", expected[1]),
                format!("<{}>", expected[2]),
                expected[3].clone()
            ]
            .join("\n\n")
        );
        assert_eq!(model.calls.borrow().len(), 3);
    }

    #[test]
    fn the_view_shows_cleaned_paragraphs_and_the_raw_rest() {
        let model = Fake::new(|u| format!("<{}>", part(u)));
        let mut live = Live::new(&[]);
        assert_eq!(live.view(), View::default());
        let s1 = sentence("a", 60);
        let ps = live.ready(&s1);
        // Handed out but not cleaned yet: still raw.
        let so_far = format!("{s1} Noch roh.");
        assert_eq!(live.view().rest(&so_far), Some(so_far.as_str()));
        live.step(&model, &ps[0], false).unwrap();
        let v = live.view();
        assert_eq!(v.cleaned, format!("<{s1}>"));
        assert_eq!(v.rest(&so_far), Some("Noch roh."));
        assert_eq!(v.rest("Etwas ganz anderes."), None);
    }

    #[test]
    fn a_repeated_heading_where_pieces_meet_is_dropped() {
        // First step: heading + list; the list stays open. Second step:
        // the model writes the heading again before the list.
        let model = Fake::new(|u| {
            if u.contains("already cleaned up") {
                "Wir brauchen:\n\n- Grill\n- Bier\n- Musik\n\nDas war's.".to_string()
            } else {
                "Wir brauchen:\n\n- Grill\n- Bier".to_string()
            }
        });
        let mut live = Live::new(&[]);
        let s1 = sentence("a", 60);
        assert_eq!(live.ready(&s1), vec![s1.clone()]);
        live.step(&model, &s1, false).unwrap();
        let all = format!("{s1} Das wars.");
        let out = live.finish(&model, &all).unwrap();
        assert_eq!(
            out,
            "Wir brauchen:\n\n- Grill\n- Bier\n- Musik\n\nDas war's."
        );
    }

    #[test]
    fn a_changed_piece_is_cleaned_again_from_that_piece_only() {
        let model = Fake::new(|u| format!("<{}>", part(u).replace("\n\n", " ")));
        let mut live = Live::new(&[]);
        let (s1, s2, s3) = (sentence("a", 60), sentence("b", 60), sentence("c", 10));
        for p in live.ready(&format!("{s1} {s2} Weiter.")) {
            live.step(&model, &p, false).unwrap();
        }
        assert_eq!(model.calls.borrow().len(), 2);
        // The transcript changed inside the second piece, more than a few
        // words.
        let mut s2b = s2.replace("b30q", "B-dreissig");
        for i in 40..45 {
            s2b = s2b.replace(&format!("b{i}q"), "anders");
        }
        let out = live.finish(&model, &format!("{s1} {s2b} {s3}")).unwrap();
        // From the second piece on: two more steps (60 + 10 words), the first
        // piece is not cleaned again.
        let calls = model.calls.borrow().clone();
        assert_eq!(calls.len(), 4);
        assert!(part(&calls[2]).contains("B-dreissig"));
        assert!(part(&calls[3]).contains("c0q"));
        // The first piece goes in only as cleaned text (its open paragraph,
        // "<…"), never raw again.
        assert!(
            calls[2..].iter().all(|c| !part(c).starts_with("a0q")),
            "the first piece stays as cleaned"
        );
        assert!(out.contains("B-dreissig"));
    }

    #[test]
    fn a_live_text_that_changes_is_followed_not_given_up() {
        let model = Fake::new(|u| format!("<{}>", part(u)));
        let mut live = Live::new(&[]);
        let s1 = sentence("a", 60);
        for p in live.ready(&s1) {
            live.step(&model, &p, false).unwrap();
        }
        // The live text revises the first piece (more than a few words),
        // then grows by another one.
        let mut s1b = s1.replace("a5q", "A-fuenf");
        for i in 10..15 {
            s1b = s1b.replace(&format!("a{i}q"), "anders");
        }
        let s2 = sentence("b", 60);
        let ps = live.ready(&format!("{s1b} {s2}"));
        assert_eq!(ps.len(), 2, "both pieces again, while speaking");
        assert!(ps[0].contains("A-fuenf"));
    }

    #[test]
    fn a_final_transcript_that_differs_is_cleaned_from_the_start() {
        let model = Fake::new(|u| format!("<{}>", part(u)));
        let mut live = Live::new(&[]);
        let s1 = sentence("a", 60);
        let ps = live.ready(&s1);
        live.step(&model, &ps[0], false).unwrap();
        let other = format!("{} Ende.", sentence("z", 10));
        let out = live.finish(&model, &other).unwrap();
        assert_eq!(out, format!("<{other}>"));
    }

    #[test]
    fn the_final_transcript_may_differ_in_case_and_punctuation() {
        let model = Fake::new(|u| format!("<{}>", part(u)));
        let mut live = Live::new(&[]);
        let s1 = sentence("a", 60);
        let ps = live.ready(&s1);
        live.step(&model, &ps[0], false).unwrap();
        // The live text said "a59." and the final one "A59!": the same words.
        let final_text = format!(
            "{}! Rest hier.",
            s1.trim_end_matches('.').replace("a0q", "A0q")
        );
        let out = live.finish(&model, &final_text).unwrap();
        assert_eq!(model.calls.borrow().len(), 2);
        assert!(out.ends_with("Rest hier.>"), "{out}");
    }

    #[test]
    fn a_few_changed_words_keep_what_is_cleaned() {
        let pre = keys(&sentence("a", 100));
        // One word changed, one missing: within 3.
        let text = format!(
            "{} Weiter so.",
            sentence("a", 100).replace("a7q", "x").replace("a50q ", "")
        );
        let (at, d) = after_prefix_close(&text, &pre).unwrap();
        assert_eq!(d, 2);
        assert_eq!(text[at..].trim(), "Weiter so.");
        // Ten changed words are too many: cleaned again.
        let mut t = sentence("a", 100);
        for i in 0..10 {
            t = t.replace(&format!("a{i}q"), "anders");
        }
        assert!(after_prefix_close(&t, &pre).is_none());
    }

    #[test]
    fn words_are_matched_ignoring_punctuation_only_tokens() {
        let pre = keys("Hallo, du – da.");
        assert_eq!(pre, vec!["hallo", "du", "da"]);
        let text = "hallo du - DA! und weiter";
        let at = after_prefix(text, &pre).unwrap();
        assert_eq!(&text[at..], " und weiter");
        assert_eq!(after_prefix("hallo ich", &pre), None);
        assert_eq!(after_prefix("hallo", &pre), None);
    }

    #[test]
    fn a_failing_model_fails_the_session_and_nothing_hangs() {
        struct Down;
        impl Model for Down {
            fn clean(&self, _: &str, _: &str) -> Result<String> {
                bail!("down")
            }
        }
        let mut live = Live::new(&[]);
        assert!(live.finish(&Down, "Hallo.").is_err());
    }

    #[test]
    fn a_session_without_a_model_reports_an_error_in_time() {
        // No server or model in tests: the engine cannot start one.
        let engine = Engine::new(llm::CLEANUP_MODEL, Duration::from_secs(1));
        let session = engine.begin(&[], || {});
        session.text("Hallo.");
        let t0 = Instant::now();
        let r = session.finish("Hallo.", Duration::from_secs(5));
        assert!(r.is_err());
        assert!(t0.elapsed() < Duration::from_secs(5));
    }
}
