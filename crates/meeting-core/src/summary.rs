//! LLM meeting summariser for local models via Ollama and for any
//! OpenAI-compatible chat-completions endpoint.
//!
//! JSON strategy:
//! - Ollama: native `/api/chat` with `format` set to a JSON schema (grammar-
//!   constrained output), `"think": false` (Qwen 3.x would otherwise emit a
//!   long reasoning block first) and a fixed `num_ctx` (changing `num_ctx`
//!   between calls makes Ollama reload the model). If the server refuses
//!   structured output (Ollama 0.40's MLX runner answers HTTP 501
//!   "structured output is unavailable"), the request is repeated without
//!   `format` and the prompt alone asks for JSON; this is remembered.
//! - OpenAI-compatible: `response_format` `json_schema`; if the server rejects
//!   that (HTTP 400/422), `json_object`; if that is rejected too, no
//!   `response_format` and only the prompt instructions. The working mode is
//!   remembered for later calls.
//! - Parsing is lenient: `<think>` blocks and code fences are stripped, the
//!   first balanced `{…}` is taken, trailing commas are repaired, and common
//!   key variants are accepted. If parsing still fails, the model is asked
//!   once more with the parse error; a second failure is an error.
//!
//! Long meetings: tokens are estimated as chars / 3.5. If the transcript does
//! not fit the context budget, it is split into chunks on line boundaries
//! (with a few lines of overlap), each chunk is summarised into partial notes
//! ("map"), and the partial notes are combined into the final summary
//! ("reduce"; repeated in groups if the partial notes are still too long).
//!
//! The output language is decided in code (German/English majority of the
//! transcript words, see [`crate::lang`]) and stated explicitly in the prompt,
//! which is more reliable than asking a small model to "use the meeting's
//! language".

use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde_json::{json, Value};

use crate::lang::{detect_language, estimate_tokens, Language};
use crate::{ActionItem, MeetingSummary, Summarizer, Transcript};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backend {
    /// Ollama's native API (`{base_url}/api/chat`).
    Ollama,
    /// OpenAI-compatible API (`{base_url}/chat/completions`; `base_url`
    /// usually ends in `/v1`). OpenAI itself, OpenRouter, LM Studio, …
    OpenAiCompatible,
    /// Anthropic Messages API (`{base_url}/messages`, `x-api-key`).
    Anthropic,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmConfig {
    pub backend: Backend,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    /// Timeout for one HTTP request (connect + full response). A long
    /// prompt gets more: see [`LlmSummarizer::request_timeout`].
    pub timeout: Duration,
    /// Time allowed per prompt token, for long prompts (0: always
    /// `timeout`).
    pub prompt_ms_per_token: u64,
    /// Context window to plan for, in tokens. Sent to Ollama as `num_ctx`.
    pub context_tokens: usize,
    /// Upper bound for the model's answer, in tokens (Ollama `num_predict`,
    /// OpenAI `max_tokens`). Reserved out of `context_tokens`.
    pub max_output_tokens: usize,
    /// Cap for one map chunk, in tokens, even when the context is larger.
    /// Smaller chunks keep more detail in the partial notes.
    pub map_chunk_tokens: usize,
    /// A longer transcript is summarised in parts even when it would fit
    /// the context; each part gets its own timeout. Unlimited by default:
    /// measured 20.11.2026 with qwen3.5:9b on Ollama (MLX), a 41-minute
    /// meeting (~10k tokens) took 147 s in one request and 282 s in two
    /// parts plus merging, with fewer points and tasks. Prompt processing
    /// (~80 tokens/s) dominates either way.
    pub single_pass_tokens: usize,

    /// Sampling temperature. `None` (the default): not sent, so every model
    /// runs with its own default (qwen3.5's is 1.0; current Claude and
    /// OpenAI models do not accept one at all).
    pub temperature: Option<f32>,
    /// Name the transcript uses for the person who recorded the meeting
    /// (the pipeline's `user_name`, default "Ich"). Mentioned in the prompt.
    pub user_name: Option<String>,
    /// Force the output language instead of detecting it.
    pub language: Option<Language>,
    /// How long Ollama keeps the model loaded after a request (Ollama's
    /// `keep_alive`, e.g. "1m"). A 9–12B model holds 8–15 GB while loaded;
    /// Ollama's default is 5 minutes. `None` leaves Ollama's default.
    pub keep_alive: Option<String>,
    /// The person's dictionary: names and terms, correctly spelled. Listed
    /// in the prompt so the notes spell them right even where speech
    /// recognition misheard them.
    pub glossary: Vec<String>,
}

impl LlmConfig {
    /// Local Ollama at `http://localhost:11434`.
    pub fn ollama(model: impl Into<String>) -> Self {
        Self {
            backend: Backend::Ollama,
            base_url: "http://localhost:11434".into(),
            model: model.into(),
            api_key: None,
            timeout: Duration::from_secs(300),
            prompt_ms_per_token: 30,
            context_tokens: 16_384,
            max_output_tokens: 3_072,
            map_chunk_tokens: 6_000,
            single_pass_tokens: usize::MAX,
            temperature: None,
            user_name: Some("Ich".into()),
            language: None,
            keep_alive: Some("1m".into()),
            glossary: Vec::new(),
        }
    }

    pub fn openai_compatible(
        base_url: impl Into<String>,
        model: impl Into<String>,
        api_key: Option<String>,
    ) -> Self {
        Self {
            backend: Backend::OpenAiCompatible,
            base_url: base_url.into(),
            api_key,
            context_tokens: 32_768,
            keep_alive: None,
            ..Self::ollama(model)
        }
    }

    /// Anthropic's Messages API. Current Claude models think on their own,
    /// reject `temperature`, and have a 1M-token context, so a meeting goes
    /// in as one request.
    pub fn anthropic(model: impl Into<String>, api_key: Option<String>) -> Self {
        Self {
            backend: Backend::Anthropic,
            base_url: "https://api.anthropic.com/v1".into(),
            api_key,
            context_tokens: 200_000,
            max_output_tokens: 16_000,
            keep_alive: None,
            ..Self::ollama(model)
        }
    }
}

/// Numbers about one summarise run, for logs and tests.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SummaryStats {
    pub language: Option<Language>,
    pub estimated_tokens: usize,
    pub map_reduce: bool,
    /// Number of map chunks (0 for single pass).
    pub chunks: usize,
    /// HTTP requests made, including retries.
    pub requests: usize,
    /// Requests repeated because the reply was not valid JSON.
    pub parse_retries: usize,
    pub elapsed: Duration,
}

pub struct LlmSummarizer {
    cfg: LlmConfig,
    agent: ureq::Agent,
    /// OpenAI response_format mode: 0 json_schema, 1 json_object, 2 none.
    oa_mode: AtomicU8,
    /// OpenAI parameter quirks: bit 1 leave out `temperature`, bit 2 send
    /// `max_completion_tokens` instead of `max_tokens` (OpenAI's current
    /// models take neither `temperature` nor `max_tokens`).
    oa_params: AtomicU8,
    /// Anthropic: 0 structured output (`output_config.format`), 1 none.
    anth_format: AtomicU8,
    /// Anthropic: 1 once the server refused the `fallbacks` parameter.
    anth_no_fallback: AtomicU8,
    /// Ollama: 1 once the server said the model does not support `think`.
    ollama_no_think: AtomicU8,
    /// Ollama `format`: 0 JSON schema, 1 `"json"`, 2 none (prompt only).
    /// Some runners (e.g. Ollama's MLX runner in 0.40) answer HTTP 501
    /// "structured output is unavailable".
    ollama_format: AtomicU8,
}

#[derive(Debug, Clone)]
struct Msg {
    role: &'static str,
    content: String,
}

/// The reply shape shown to the model. `summary` is a list because models
/// asked for "one string with bullet lines" tend to break out of the string
/// into a list anyway (seen with qwen3.5:9b without structured output); the
/// list is joined into "- " lines in code.
const JSON_SHAPE: &str = r#"{"title": "...", "summary": ["...", "..."], "decisions": ["..."], "action_items": [{"owner": "..." or null, "text": "..."}]}"#;

/// Prompt overhead estimate (system prompt + wrapper), in tokens.
const PROMPT_OVERHEAD_TOKENS: usize = 900;
/// Lines repeated at the start of the next chunk for context.
const CHUNK_OVERLAP_LINES: usize = 3;

impl LlmSummarizer {
    pub fn new(cfg: LlmConfig) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(cfg.timeout))
            .http_status_as_error(false)
            .build()
            .into();
        let oa_params = if cfg.base_url.contains("api.openai.com") {
            OA_NO_TEMPERATURE | OA_COMPLETION_TOKENS
        } else {
            0
        };
        Self {
            cfg,
            agent,
            oa_mode: AtomicU8::new(0),
            oa_params: AtomicU8::new(oa_params),
            anth_format: AtomicU8::new(0),
            anth_no_fallback: AtomicU8::new(0),
            ollama_no_think: AtomicU8::new(0),
            ollama_format: AtomicU8::new(0),
        }
    }

    /// A client for free-form answers (no summary schema), used by the
    /// connection check. Sends `system` + `user`, returns the reply text
    /// with `<think>` blocks and code fences removed.
    pub fn chat_plain(&self, system: &str, user: &str) -> Result<String> {
        self.ollama_format.store(2, Ordering::Relaxed);
        self.oa_mode.store(2, Ordering::Relaxed);
        self.anth_format.store(1, Ordering::Relaxed);
        let msgs = vec![
            Msg {
                role: "system",
                content: system.to_string(),
            },
            Msg {
                role: "user",
                content: user.to_string(),
            },
        ];
        let mut stats = SummaryStats::default();
        let reply = self.chat(&msgs, &mut stats)?;
        Ok(strip_fences(&strip_think(&reply)))
    }

    pub fn config(&self) -> &LlmConfig {
        &self.cfg
    }

    /// Like [`Summarizer::summarize`], plus statistics.
    pub fn summarize_with_stats(
        &self,
        transcript: &Transcript,
    ) -> Result<(MeetingSummary, SummaryStats)> {
        let started = Instant::now();
        let mut stats = SummaryStats::default();
        if transcript.segments.iter().all(|s| s.text.trim().is_empty()) {
            bail!("transcript is empty; nothing to summarise");
        }
        let lang = self.cfg.language.unwrap_or_else(|| {
            let words: String = transcript
                .segments
                .iter()
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join(" ");
            detect_language(&words).unwrap_or(Language::German)
        });
        stats.language = Some(lang);
        let speakers = transcript.speakers();
        let text = transcript.to_plain_text();
        stats.estimated_tokens = estimate_tokens(&text);

        let budget = self.transcript_budget();
        let summary = if stats.estimated_tokens <= budget.min(self.cfg.single_pass_tokens) {
            let msgs = vec![
                Msg {
                    role: "system",
                    content: self.system_prompt(lang, &speakers, Mode::Final),
                },
                Msg {
                    role: "user",
                    content: format!("Transcript:\n\n{text}"),
                },
            ];
            self.ask_json(&msgs, &mut stats)?
        } else {
            stats.map_reduce = true;
            let chunks = chunk_lines(
                &text,
                part_tokens(
                    stats.estimated_tokens,
                    budget.min(self.cfg.map_chunk_tokens),
                ),
            );
            stats.chunks = chunks.len();
            let mut partials = Vec::with_capacity(chunks.len());
            for (i, chunk) in chunks.iter().enumerate() {
                let msgs = vec![
                    Msg {
                        role: "system",
                        content: self.system_prompt(lang, &speakers, Mode::Map),
                    },
                    Msg {
                        role: "user",
                        content: format!(
                            "Part {} of {} of the transcript ({}):\n\n{}",
                            i + 1,
                            chunks.len(),
                            span_label(chunk),
                            chunk
                        ),
                    },
                ];
                let part = self
                    .ask_json(&msgs, &mut stats)
                    .with_context(|| format!("summarising part {} of {}", i + 1, chunks.len()))?;
                partials.push((span_label(chunk), part));
            }
            self.reduce(partials, lang, &speakers, &mut stats)?
        };
        stats.elapsed = started.elapsed();
        Ok((clean_summary(summary, &speakers), stats))
    }

    /// Tokens available for transcript text in one request.
    fn transcript_budget(&self) -> usize {
        self.cfg
            .context_tokens
            .saturating_sub(self.cfg.max_output_tokens + PROMPT_OVERHEAD_TOKENS)
            .max(500)
    }

    fn reduce(
        &self,
        mut partials: Vec<(String, MeetingSummary)>,
        lang: Language,
        speakers: &[String],
        stats: &mut SummaryStats,
    ) -> Result<MeetingSummary> {
        let budget = self.transcript_budget();
        loop {
            let rendered: Vec<String> = partials
                .iter()
                .enumerate()
                .map(|(i, (span, p))| render_partial(i + 1, span, p))
                .collect();
            let all = rendered.join("\n");
            if estimate_tokens(&all) <= budget || partials.len() <= 1 {
                let msgs = vec![
                    Msg {
                        role: "system",
                        content: self.system_prompt(lang, speakers, Mode::Reduce),
                    },
                    Msg {
                        role: "user",
                        content: format!("Notes on consecutive parts of the meeting:\n\n{all}"),
                    },
                ];
                return self
                    .ask_json(&msgs, stats)
                    .context("combining partial notes");
            }
            // Too long: combine neighbouring partials in groups first.
            let mut groups: Vec<Vec<usize>> = vec![vec![]];
            let mut used = 0;
            for (i, r) in rendered.iter().enumerate() {
                let t = estimate_tokens(r);
                if used + t > budget && !groups.last().unwrap().is_empty() {
                    groups.push(vec![]);
                    used = 0;
                }
                groups.last_mut().unwrap().push(i);
                used += t;
            }
            if groups.len() >= partials.len() {
                // No two neighbours fit together: combine pairs anyway. Each
                // round halves the count, so this always terminates.
                groups = (0..partials.len())
                    .collect::<Vec<_>>()
                    .chunks(2)
                    .map(<[usize]>::to_vec)
                    .collect();
            }
            let mut next = Vec::with_capacity(groups.len());
            for g in groups {
                let span = format!(
                    "{} – {}",
                    partials[g[0]].0.split(" – ").next().unwrap_or(""),
                    partials[*g.last().unwrap()]
                        .0
                        .rsplit(" – ")
                        .next()
                        .unwrap_or("")
                );
                let text = g
                    .iter()
                    .map(|&i| rendered[i].as_str())
                    .collect::<Vec<_>>()
                    .join("\n");
                let msgs = vec![
                    Msg {
                        role: "system",
                        content: self.system_prompt(lang, speakers, Mode::ReduceIntermediate),
                    },
                    Msg {
                        role: "user",
                        content: format!("Notes on consecutive parts of the meeting:\n\n{text}"),
                    },
                ];
                next.push((
                    span,
                    self.ask_json(&msgs, stats)
                        .context("combining partial notes")?,
                ));
            }
            partials = next;
        }
    }

    fn system_prompt(&self, lang: Language, speakers: &[String], mode: Mode) -> String {
        let mut p = String::new();
        p.push_str(match mode {
            Mode::Final => "You write meeting notes from the transcript of a recorded meeting.\n",
            Mode::Map => "You take notes on one part of a long meeting transcript. Your notes will later be combined with the notes of the other parts.\n",
            Mode::Reduce => "You write the final meeting notes from notes that were taken on consecutive parts of one meeting.\n",
            Mode::ReduceIntermediate => "You combine notes that were taken on consecutive parts of one meeting into one set of notes for that stretch of the meeting.\n",
        });
        p.push_str("\nRules:\n");
        p.push_str("- Use only information that is explicitly stated. Never invent facts, names, numbers, dates, owners, decisions or tasks. If something is unclear, leave it out.\n");
        p.push_str("- Keep concrete facts exactly as stated: amounts, prices, percentages, numbers, dates, weekdays and deadlines.\n");
        p.push_str(&format!(
            "- Write all text values in {}.{}\n",
            lang.name(),
            match lang {
                Language::German => " Schreibe alles auf Deutsch.",
                Language::English => "",
            }
        ));
        if !speakers.is_empty() {
            p.push_str(&format!(
                "- Speakers: {}. Refer to people exactly by these labels or by names that are actually said in the transcript. Do not guess real names for generic labels like \"Speaker 2\".\n",
                speakers.join(", ")
            ));
        }
        p.push_str("- Attribute statements and commitments to the speaker who made them (the label on that line). When a speaker assigns a task to \"du\"/\"you\", the owner is the person addressed, usually the previous speaker. Asking a question about something does not make a speaker its owner.\n");
        p.push_str("- Keep people, customers and projects apart. Every detail (a time, a date, a weekday, an availability, a number, a task) belongs to the person or topic it was said about; never move it to another one.\n");
        p.push_str("- A task is something someone will still do after the meeting. If the speaker says it is already done, it is not a task. Name the customer or project each task is about.\n");
        p.push_str("- Leave out greetings and pure small talk. Personal news (someone is ill, on holiday) may stay in one short point.\n");
        if let Some(u) = self
            .cfg
            .user_name
            .as_deref()
            .filter(|u| speakers.iter().any(|s| s == u))
        {
            p.push_str(&format!(
                "- \"{u}\" is the person who recorded the meeting.\n"
            ));
        }
        if matches!(mode, Mode::Final | Mode::Map) {
            p.push_str("- The transcript comes from speech recognition and may contain misheard words. Read past obvious errors; do not quote them.\n");
        }
        let glossary: Vec<&str> = self
            .cfg
            .glossary
            .iter()
            .map(|w| w.trim())
            .filter(|w| !w.is_empty())
            .collect();
        if !glossary.is_empty() {
            p.push_str(&format!(
                "- Names and terms, correctly spelled: {}. Speech recognition often mishears them; where the text clearly means one of them, write it exactly as listed.\n",
                glossary.join(", ")
            ));
        }
        if matches!(mode, Mode::Reduce | Mode::ReduceIntermediate) {
            p.push_str("- Merge duplicates. Keep every decision and task unless a later part explicitly changed or cancelled it; then keep only the final state.\n");
        }
        p.push_str("\nAnswer with one JSON object in exactly this shape:\n");
        p.push_str(JSON_SHAPE);
        p.push_str("\n\n");
        match mode {
            Mode::Final => {
                p.push_str(
                    "- \"title\": a short title for the meeting, at most 8 words, no date.\n",
                );
                p.push_str("- \"summary\": list of 3 to 8 short bullet points (strings) covering the main topics and outcomes.\n");
            }
            Mode::Reduce => {
                p.push_str(
                    "- \"title\": a short title for the meeting, at most 8 words, no date.\n",
                );
                p.push_str("- \"summary\": list of 4 to 10 short bullet points (strings), one or more per topic, keeping the key numbers of each topic.\n");
            }
            Mode::Map | Mode::ReduceIntermediate => {
                p.push_str("- \"title\": a few words naming the main topic of this part.\n");
                p.push_str("- \"summary\": list of up to 12 bullet points (strings) with all topics, facts, numbers, dates and open questions of this part.\n");
            }
        }
        p.push_str("- \"decisions\": list of strings, what the participants agreed to do or not to do, including agreeing to wait for something. [] if nothing was agreed.\n");
        p.push_str("- \"action_items\": list of objects; \"owner\" is the person who committed to or was assigned the task (speaker label or name), or null if not clear; \"text\" is the task, including its deadline if one was stated. [] if none.\n");
        p.push_str("\nReturn only the JSON object, no other text.");
        p
    }

    /// Sends the messages, parses the reply; on a parse failure asks once more.
    fn ask_json(&self, msgs: &[Msg], stats: &mut SummaryStats) -> Result<MeetingSummary> {
        let reply = self.chat(msgs, stats)?;
        debug_reply(&reply);
        match parse_summary_json(&reply) {
            Ok(s) => Ok(s),
            Err(first) => {
                stats.parse_retries += 1;
                // Ask again from scratch with the error, without replaying
                // the broken reply (models tend to copy it verbatim).
                let mut retry = msgs.to_vec();
                if let Some(last) = retry.iter_mut().rev().find(|m| m.role == "user") {
                    last.content.push_str(&format!(
                        "\n\nNote: a previous answer to this was not valid JSON ({first}). Answer with only one valid JSON object in exactly this shape: {JSON_SHAPE}"
                    ));
                }
                let reply2 = self.chat(&retry, stats)?;
                debug_reply(&reply2);
                parse_summary_json(&reply2).map_err(|second| {
                    anyhow!(
                        "model did not return valid JSON after a retry: {second}; first error: {first}; reply: {}",
                        excerpt(&reply2)
                    )
                })
            }
        }
    }

    fn chat(&self, msgs: &[Msg], stats: &mut SummaryStats) -> Result<String> {
        match self.cfg.backend {
            Backend::Ollama => self.chat_ollama(msgs, stats),
            Backend::OpenAiCompatible => self.chat_openai(msgs, stats),
            Backend::Anthropic => self.chat_anthropic(msgs, stats),
        }
    }

    fn post(&self, url: &str, body: &Value) -> Result<(u16, String)> {
        let mut req = self
            .agent
            .post(url)
            .header("Content-Type", "application/json");
        if let Some(k) = self.cfg.api_key.as_deref().filter(|k| !k.is_empty()) {
            if self.cfg.backend == Backend::Anthropic {
                req = req.header("x-api-key", k);
            } else {
                req = req.header("Authorization", &format!("Bearer {k}"));
            }
        }
        if self.cfg.backend == Backend::Anthropic {
            req = req.header("anthropic-version", "2023-06-01");
            if self.anth_no_fallback.load(Ordering::Relaxed) == 0 {
                req = req.header("anthropic-beta", "server-side-fallback-2026-07-01");
            }
        }
        let body = serde_json::to_string(body)?;
        let req = req
            .config()
            .timeout_global(Some(self.request_timeout(estimate_tokens(&body))))
            .build();
        let mut resp = req
            .send(body)
            .with_context(|| format!("request to {url} failed"))?;
        let status = resp.status().as_u16();
        let text = resp
            .body_mut()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_string()
            .with_context(|| format!("reading response from {url}"))?;
        Ok((status, text))
    }

    /// The timeout for one request with a prompt of `prompt_tokens`: the
    /// configured one, or more for a long prompt. A local model processes the
    /// prompt first: qwen3.5:9b on Ollama (MLX) did ~76 tokens/s on
    /// 20.11.2026, so a 10k-token meeting took 2.5 minutes on a free Mac and
    /// the fixed 5 minutes left little room. The default 30 ms per token
    /// allows ~33 tokens/s, plus a minute for the answer.
    pub fn request_timeout(&self, prompt_tokens: usize) -> Duration {
        if self.cfg.prompt_ms_per_token == 0 {
            return self.cfg.timeout;
        }
        let prompt = self
            .cfg
            .prompt_ms_per_token
            .saturating_mul(prompt_tokens as u64);
        self.cfg
            .timeout
            .max(Duration::from_secs(60) + Duration::from_millis(prompt))
    }

    fn chat_ollama(&self, msgs: &[Msg], stats: &mut SummaryStats) -> Result<String> {
        let url = format!("{}/api/chat", self.cfg.base_url.trim_end_matches('/'));
        loop {
            let mut body = json!({
                "model": self.cfg.model,
                "messages": msgs_json(msgs),
                "stream": false,
                "options": {
                    "num_ctx": self.cfg.context_tokens,
                    "num_predict": self.cfg.max_output_tokens,
                },
            });
            if let Some(t) = self.cfg.temperature {
                body["options"]["temperature"] = json!(t);
            }
            if let Some(k) = &self.cfg.keep_alive {
                body["keep_alive"] = json!(k);
            }
            let think = self.ollama_no_think.load(Ordering::Relaxed) == 0;
            if think {
                body["think"] = json!(false);
            }
            let format = self.ollama_format.load(Ordering::Relaxed);
            match format {
                0 => body["format"] = summary_schema(),
                1 => body["format"] = json!("json"),
                _ => {}
            }
            stats.requests += 1;
            let (status, text) = self.post(&url, &body)?;
            if status == 400 && think && text.contains("does not support thinking") {
                self.ollama_no_think.store(1, Ordering::Relaxed);
                continue;
            }
            if (status == 400 || status == 501) && format < 2 {
                if text.contains("structured output is unavailable") {
                    // Neither schema nor "json" will work on this runner.
                    self.ollama_format.store(2, Ordering::Relaxed);
                    continue;
                }
                if text.contains("format") || text.contains("schema") || text.contains("structured")
                {
                    self.ollama_format.store(format + 1, Ordering::Relaxed);
                    continue;
                }
            }
            if !(200..300).contains(&status) {
                bail!("Ollama returned HTTP {status}: {}", excerpt(&text));
            }
            let v: Value = serde_json::from_str(&text)
                .with_context(|| format!("Ollama response is not JSON: {}", excerpt(&text)))?;
            if let Some(e) = v.get("error").and_then(Value::as_str) {
                bail!("Ollama error: {e}");
            }
            return v
                .pointer("/message/content")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| {
                    anyhow!("Ollama response has no message content: {}", excerpt(&text))
                });
        }
    }

    fn chat_openai(&self, msgs: &[Msg], stats: &mut SummaryStats) -> Result<String> {
        let url = format!(
            "{}/chat/completions",
            self.cfg.base_url.trim_end_matches('/')
        );
        loop {
            let mode = self.oa_mode.load(Ordering::Relaxed);
            let params = self.oa_params.load(Ordering::Relaxed);
            let mut body = json!({
                "model": self.cfg.model,
                "messages": msgs_json(msgs),
                "stream": false,
            });
            if let Some(t) = self
                .cfg
                .temperature
                .filter(|_| params & OA_NO_TEMPERATURE == 0)
            {
                body["temperature"] = json!(t);
            }
            if params & OA_COMPLETION_TOKENS == 0 {
                body["max_tokens"] = json!(self.cfg.max_output_tokens);
            } else {
                body["max_completion_tokens"] = json!(self.cfg.max_output_tokens);
            }
            match mode {
                0 => {
                    body["response_format"] = json!({
                        "type": "json_schema",
                        "json_schema": { "name": "meeting_summary", "schema": summary_schema() },
                    })
                }
                1 => body["response_format"] = json!({ "type": "json_object" }),
                _ => {}
            }
            stats.requests += 1;
            let (status, text) = self.post(&url, &body)?;
            if status == 400 || status == 422 {
                let more = if text.contains("temperature") && params & OA_NO_TEMPERATURE == 0 {
                    OA_NO_TEMPERATURE
                } else if text.contains("max_tokens") && params & OA_COMPLETION_TOKENS == 0 {
                    OA_COMPLETION_TOKENS
                } else {
                    0
                };
                if more != 0 {
                    self.oa_params.store(params | more, Ordering::Relaxed);
                    continue;
                }
            }
            if (status == 400 || status == 422) && mode < 2 {
                // Most likely the server does not support this response_format.
                self.oa_mode.store(mode + 1, Ordering::Relaxed);
                continue;
            }
            if !(200..300).contains(&status) {
                bail!("LLM endpoint returned HTTP {status}: {}", excerpt(&text));
            }
            let v: Value = serde_json::from_str(&text)
                .with_context(|| format!("LLM response is not JSON: {}", excerpt(&text)))?;
            if let Some(e) = v.get("error") {
                bail!("LLM error: {e}");
            }
            return v
                .pointer("/choices/0/message/content")
                .and_then(Value::as_str)
                .map(str::to_string)
                .ok_or_else(|| anyhow!("LLM response has no message content: {}", excerpt(&text)));
        }
    }
}

impl LlmSummarizer {
    fn chat_anthropic(&self, msgs: &[Msg], stats: &mut SummaryStats) -> Result<String> {
        let url = format!("{}/messages", self.cfg.base_url.trim_end_matches('/'));
        let system: Vec<&str> = msgs
            .iter()
            .filter(|m| m.role == "system")
            .map(|m| m.content.as_str())
            .collect();
        let turns: Vec<Value> = msgs
            .iter()
            .filter(|m| m.role != "system")
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect();
        loop {
            let format = self.anth_format.load(Ordering::Relaxed);
            let fallback = self.anth_no_fallback.load(Ordering::Relaxed) == 0;
            // No `temperature`: current Claude models reject it.
            let mut body = json!({
                "model": self.cfg.model,
                "max_tokens": self.cfg.max_output_tokens,
                "messages": turns,
            });
            if !system.is_empty() {
                body["system"] = json!(system.join("\n\n"));
            }
            if format == 0 {
                body["output_config"] =
                    json!({ "format": { "type": "json_schema", "schema": summary_schema() } });
            }
            if fallback {
                // If a safety check declines, the API retries on another model.
                body["fallbacks"] = json!("default");
            }
            stats.requests += 1;
            let (status, text) = self.post(&url, &body)?;
            if status == 400 && fallback && text.contains("fallback") {
                self.anth_no_fallback.store(1, Ordering::Relaxed);
                continue;
            }
            if status == 400
                && format == 0
                && (text.contains("output_config") || text.contains("schema"))
            {
                self.anth_format.store(1, Ordering::Relaxed);
                continue;
            }
            if !(200..300).contains(&status) {
                bail!("Anthropic returned HTTP {status}: {}", excerpt(&text));
            }
            let v: Value = serde_json::from_str(&text)
                .with_context(|| format!("Anthropic response is not JSON: {}", excerpt(&text)))?;
            if v["stop_reason"] == "refusal" {
                bail!(
                    "The model declined to answer: {}",
                    excerpt(&v["stop_details"].to_string())
                );
            }
            let out: String = v["content"]
                .as_array()
                .map(|blocks| {
                    blocks
                        .iter()
                        .filter(|b| b["type"] == "text")
                        .filter_map(|b| b["text"].as_str())
                        .collect()
                })
                .unwrap_or_default();
            if out.is_empty() {
                bail!("Anthropic response has no text: {}", excerpt(&text));
            }
            return Ok(out);
        }
    }
}

const OA_NO_TEMPERATURE: u8 = 1;
const OA_COMPLETION_TOKENS: u8 = 2;

impl Summarizer for LlmSummarizer {
    fn summarize(&self, transcript: &Transcript) -> Result<MeetingSummary> {
        self.summarize_with_stats(transcript).map(|(s, _)| s)
    }
}

#[derive(Clone, Copy)]
enum Mode {
    Final,
    Map,
    Reduce,
    ReduceIntermediate,
}

fn msgs_json(msgs: &[Msg]) -> Value {
    Value::Array(
        msgs.iter()
            .map(|m| json!({ "role": m.role, "content": m.content }))
            .collect(),
    )
}

/// JSON schema of [`MeetingSummary`] as requested from the model.
pub fn summary_schema() -> Value {
    json!({
        "type": "object",
        "properties": {
            "title": { "type": "string" },
            "summary": { "type": "array", "items": { "type": "string" } },
            "decisions": { "type": "array", "items": { "type": "string" } },
            "action_items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "owner": { "type": ["string", "null"] },
                        "text": { "type": "string" }
                    },
                    "required": ["owner", "text"],
                    "additionalProperties": false
                }
            }
        },
        "required": ["title", "summary", "decisions", "action_items"],
        "additionalProperties": false
    })
}

/// With `MEETING_LLM_DEBUG=1`, raw model replies go to stderr.
fn debug_reply(reply: &str) {
    if std::env::var("MEETING_LLM_DEBUG").as_deref() == Ok("1") {
        eprintln!("--- LLM reply ---\n{reply}\n--- end ---");
    }
}

fn excerpt(s: &str) -> String {
    let t: String = s.chars().take(300).collect();
    if t.len() < s.len() {
        format!("{t}…")
    } else {
        t
    }
}

/// "[00:00] … [12:34]" → "00:00 – 12:34", from the first and last line stamps.
fn span_label(chunk: &str) -> String {
    let stamp = |l: &str| -> Option<String> {
        let l = l.trim_start();
        let rest = l.strip_prefix('[')?;
        Some(rest[..rest.find(']')?].to_string())
    };
    let first = chunk.lines().find_map(stamp);
    let last = chunk.lines().rev().find_map(stamp);
    match (first, last) {
        (Some(a), Some(b)) => format!("{a} – {b}"),
        _ => "?".into(),
    }
}

/// Tokens per part for a transcript of `total` tokens with parts of at
/// most `cap`: as many parts as needed, of about equal size, so the last
/// part is not a short remainder.
fn part_tokens(total: usize, cap: usize) -> usize {
    let cap = cap.max(500);
    let parts = total.div_ceil(cap).max(1);
    // A little slack: the parts are cut on line boundaries.
    (total.div_ceil(parts) * 11 / 10).clamp(500, cap)
}

/// Splits transcript text into chunks of at most `max_tokens` (estimated) on
/// line boundaries, repeating the last few lines of a chunk at the start of
/// the next. A single line longer than the limit is split at whitespace.
pub fn chunk_lines(text: &str, max_tokens: usize) -> Vec<String> {
    let max_chars = (max_tokens as f64 * 3.5) as usize;
    let mut lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.chars().count() <= max_chars {
            lines.push(line.to_string());
            continue;
        }
        let mut cur = String::new();
        for w in line.split_whitespace() {
            if !cur.is_empty() && cur.chars().count() + 1 + w.chars().count() > max_chars {
                lines.push(std::mem::take(&mut cur));
            }
            if !cur.is_empty() {
                cur.push(' ');
            }
            cur.push_str(w);
        }
        if !cur.is_empty() {
            lines.push(cur);
        }
    }
    let mut chunks = Vec::new();
    let mut start = 0;
    while start < lines.len() {
        let mut end = start;
        let mut chars = 0;
        while end < lines.len() {
            let add = lines[end].chars().count() + 1;
            if chars + add > max_chars && end > start {
                break;
            }
            chars += add;
            end += 1;
        }
        chunks.push(lines[start..end].join("\n"));
        if end >= lines.len() {
            break;
        }
        // Overlap, but always make progress.
        let back = CHUNK_OVERLAP_LINES.min((end - start).saturating_sub(1));
        start = end - back;
    }
    chunks
}

fn render_partial(n: usize, span: &str, p: &MeetingSummary) -> String {
    let mut s = format!("Part {n} ({span}): {}\n", p.title.trim());
    if !p.summary.trim().is_empty() {
        s.push_str(&format!("Notes:\n{}\n", p.summary.trim()));
    }
    if !p.decisions.is_empty() {
        s.push_str("Decisions:\n");
        for d in &p.decisions {
            s.push_str(&format!("- {d}\n"));
        }
    }
    if !p.action_items.is_empty() {
        s.push_str("Tasks:\n");
        for a in &p.action_items {
            match &a.owner {
                Some(o) => s.push_str(&format!("- [{o}] {}\n", a.text)),
                None => s.push_str(&format!("- [owner unclear] {}\n", a.text)),
            }
        }
    }
    s
}

// ---------------------------------------------------------------------------
// Lenient JSON parsing
// ---------------------------------------------------------------------------

/// Parses a model reply into a [`MeetingSummary`], tolerating thinking
/// blocks, code fences, text around the JSON, trailing commas and common key
/// variants.
pub fn parse_summary_json(reply: &str) -> Result<MeetingSummary> {
    let text = strip_think(reply);
    let text = strip_fences(&text);
    let obj = extract_object(&text).ok_or_else(|| anyhow!("no JSON object found in reply"))?;
    let v: Value = serde_json::from_str(&obj)
        .or_else(|e| serde_json::from_str(&remove_trailing_commas(&obj)).map_err(|_| e))
        .map_err(|e| anyhow!("invalid JSON: {e}"))?;
    summary_from_value(&v)
}

fn strip_think(s: &str) -> String {
    let mut out = s.to_string();
    while let Some(a) = out.find("<think>") {
        match out[a..].find("</think>") {
            Some(b) => out.replace_range(a..a + b + "</think>".len(), ""),
            None => {
                out.truncate(a);
                break;
            }
        }
    }
    out
}

fn strip_fences(s: &str) -> String {
    let t = s.trim();
    if let Some(a) = t.find("```") {
        let after = &t[a + 3..];
        // Skip a language tag like "json".
        let body_start = after.find('\n').map(|i| i + 1).unwrap_or(0);
        let body = &after[body_start..];
        let body = match body.find("```") {
            Some(b) => &body[..b],
            None => body,
        };
        if body.contains('{') {
            return body.to_string();
        }
    }
    t.to_string()
}

/// The first balanced `{…}` (string-aware). If the object is cut off, the
/// missing closing brackets are added.
pub(crate) fn extract_object(s: &str) -> Option<String> {
    let start = s.find('{')?;
    let mut stack: Vec<char> = Vec::new();
    let mut in_str = false;
    let mut esc = false;
    for (i, c) in s[start..].char_indices() {
        if in_str {
            match c {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => stack.push('}'),
            '[' => stack.push(']'),
            '}' | ']' => {
                stack.pop();
                if stack.is_empty() {
                    return Some(s[start..start + i + 1].to_string());
                }
            }
            _ => {}
        }
    }
    // Truncated: close what is open.
    let mut out = s[start..].trim_end().trim_end_matches(',').to_string();
    if in_str {
        out.push('"');
    }
    while let Some(c) = stack.pop() {
        out.push(c);
    }
    Some(out)
}

pub(crate) fn remove_trailing_commas(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let chars: Vec<char> = s.chars().collect();
    let mut in_str = false;
    let mut esc = false;
    for (i, &c) in chars.iter().enumerate() {
        if in_str {
            out.push(c);
            match c {
                _ if esc => esc = false,
                '\\' => esc = true,
                '"' => in_str = false,
                _ => {}
            }
            continue;
        }
        if c == '"' {
            in_str = true;
        }
        if c == ',' {
            let next = chars[i + 1..].iter().find(|c| !c.is_whitespace());
            if matches!(next, Some('}') | Some(']')) {
                continue;
            }
        }
        out.push(c);
    }
    out
}

fn get<'a>(v: &'a Value, keys: &[&str]) -> Option<&'a Value> {
    let obj = v.as_object()?;
    for k in keys {
        if let Some(x) = obj.get(*k) {
            return Some(x);
        }
    }
    // Case-insensitive fallback.
    obj.iter()
        .find(|(k, _)| keys.iter().any(|w| k.eq_ignore_ascii_case(w)))
        .map(|(_, x)| x)
}

fn value_to_text(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.trim().to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::Array(items) => {
            let lines: Vec<String> = items
                .iter()
                .filter_map(value_to_text)
                .filter(|s| !s.is_empty())
                .map(|s| {
                    if s.starts_with("- ") {
                        s
                    } else {
                        format!("- {s}")
                    }
                })
                .collect();
            Some(lines.join("\n"))
        }
        Value::Object(_) => {
            get(v, &["text", "content", "description", "summary"]).and_then(value_to_text)
        }
        _ => None,
    }
}

fn list_of_strings(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| match i {
                Value::Object(_) => get(
                    i,
                    &["text", "decision", "description", "content", "entscheidung"],
                )
                .and_then(value_to_text),
                other => value_to_text(other),
            })
            .map(|s| s.trim_start_matches("- ").trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        Some(Value::String(s)) => s
            .lines()
            .map(|l| {
                l.trim()
                    .trim_start_matches(['-', '*', '•'])
                    .trim()
                    .to_string()
            })
            .filter(|l| !l.is_empty())
            .collect(),
        _ => vec![],
    }
}

fn summary_from_value(v: &Value) -> Result<MeetingSummary> {
    let mut v = v;
    // Unwrap {"meeting_summary": {...}} and similar single-key wrappers.
    if let Some(obj) = v.as_object() {
        if obj.len() == 1 {
            if let Some(inner) = obj.values().next().filter(|x| x.is_object()) {
                v = inner;
            }
        }
    }
    if !v.is_object() {
        bail!("reply is not a JSON object");
    }
    let title = get(v, &["title", "titel", "heading"])
        .and_then(value_to_text)
        .unwrap_or_default();
    let summary = get(v, &["summary", "zusammenfassung", "notes", "overview"])
        .and_then(value_to_text)
        .unwrap_or_default();
    let decisions = list_of_strings(get(v, &["decisions", "entscheidungen", "decision"]));
    let action_items = match get(
        v,
        &[
            "action_items",
            "actionItems",
            "action items",
            "tasks",
            "todos",
            "aufgaben",
        ],
    ) {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|i| match i {
                Value::String(s) => Some(ActionItem {
                    owner: None,
                    text: s.trim().to_string(),
                }),
                Value::Object(_) => {
                    let text = get(
                        i,
                        &["text", "task", "item", "description", "action", "aufgabe"],
                    )
                    .and_then(value_to_text)?;
                    let owner = get(
                        i,
                        &[
                            "owner",
                            "assignee",
                            "responsible",
                            "who",
                            "verantwortlich",
                            "person",
                        ],
                    )
                    .and_then(value_to_text);
                    Some(ActionItem { owner, text })
                }
                _ => None,
            })
            .collect(),
        Some(Value::String(s)) => list_of_strings(Some(&Value::String(s.clone())))
            .into_iter()
            .map(|text| ActionItem { owner: None, text })
            .collect(),
        _ => vec![],
    };
    if title.is_empty() && summary.is_empty() {
        bail!("JSON object has neither \"title\" nor \"summary\"");
    }
    Ok(MeetingSummary {
        title,
        summary,
        decisions,
        action_items,
    })
}

/// Final tidy-up: trims, removes empty and duplicate entries, normalises
/// owners ("null", "unknown", "-" → none; case-insensitive match to a speaker
/// label → that label), strips a trailing period from the title.
fn clean_summary(mut s: MeetingSummary, speakers: &[String]) -> MeetingSummary {
    s.title = s.title.split_whitespace().collect::<Vec<_>>().join(" ");
    s.title = s
        .title
        .trim_matches(|c| c == '"' || c == '.' || c == '#' || c == ' ')
        .to_string();
    s.summary = s.summary.trim().to_string();
    let mut seen = std::collections::HashSet::new();
    s.decisions
        .retain(|d| !d.trim().is_empty() && seen.insert(d.trim().to_lowercase()));
    for d in &mut s.decisions {
        *d = d.trim().to_string();
    }
    let mut seen = std::collections::HashSet::new();
    s.action_items = s
        .action_items
        .into_iter()
        .filter_map(|mut a| {
            a.text = a.text.trim().to_string();
            if a.text.is_empty() || !seen.insert(a.text.to_lowercase()) {
                return None;
            }
            a.owner = a.owner.map(|o| o.trim().to_string()).filter(|o| {
                !o.is_empty()
                    && !matches!(
                        o.to_lowercase().as_str(),
                        "null"
                            | "none"
                            | "unknown"
                            | "unklar"
                            | "unbekannt"
                            | "n/a"
                            | "-"
                            | "?"
                            | "niemand"
                            | "nobody"
                    )
            });
            if let Some(o) = &a.owner {
                if let Some(canon) = speakers.iter().find(|sp| sp.eq_ignore_ascii_case(o)) {
                    a.owner = Some(canon.clone());
                }
            }
            Some(a)
        })
        .collect();
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn glossary_goes_into_every_prompt() {
        let llm = LlmSummarizer::new(LlmConfig {
            glossary: vec!["Kelasur".into(), " ".into(), "Claude Code".into()],
            ..LlmConfig::ollama("m")
        });
        for mode in [
            Mode::Final,
            Mode::Map,
            Mode::Reduce,
            Mode::ReduceIntermediate,
        ] {
            let p = llm.system_prompt(Language::German, &[], mode);
            assert!(
                p.contains("correctly spelled: Kelasur, Claude Code."),
                "{p}"
            );
        }
        let p = LlmSummarizer::new(LlmConfig::ollama("m")).system_prompt(
            Language::German,
            &[],
            Mode::Final,
        );
        assert!(!p.contains("correctly spelled"), "{p}");
    }

    #[test]
    fn a_long_prompt_gets_a_longer_timeout() {
        let llm = LlmSummarizer::new(LlmConfig::ollama("m"));
        assert_eq!(llm.request_timeout(500), Duration::from_secs(300));
        // The 41-minute meeting of 20.11.2026: ~10.8k tokens.
        assert_eq!(llm.request_timeout(10_800), Duration::from_secs(384));
        // The largest single request with the default 16k context.
        assert_eq!(llm.request_timeout(12_400), Duration::from_secs(432));
    }

    #[test]
    fn parses_plain_json() {
        let s = parse_summary_json(r#"{"title":"Kampagne","summary":"- a\n- b","decisions":["x"],"action_items":[{"owner":"Ich","text":"t"}]}"#).unwrap();
        assert_eq!(s.title, "Kampagne");
        assert_eq!(s.decisions, vec!["x"]);
        assert_eq!(s.action_items[0].owner.as_deref(), Some("Ich"));
    }

    #[test]
    fn parses_fenced_json_with_text_around() {
        let s = parse_summary_json("Hier ist das Ergebnis:\n```json\n{\"title\": \"T\", \"summary\": \"S\", \"decisions\": [], \"action_items\": []}\n```\nViel Erfolg!").unwrap();
        assert_eq!((s.title.as_str(), s.summary.as_str()), ("T", "S"));
    }

    #[test]
    fn strips_think_blocks_and_trailing_commas() {
        let s = parse_summary_json("<think>Hmm {not json}</think>{\"title\":\"T\",\"summary\":\"S\",\"decisions\":[\"a\",],}").unwrap();
        assert_eq!(s.decisions, vec!["a"]);
    }

    #[test]
    fn tolerates_key_and_shape_variants() {
        let s = parse_summary_json(r#"{"meeting": {"Title": "T", "summary": ["eins", "zwei"], "decisions": "- d1\n- d2", "tasks": [{"task": "x", "assignee": "Speaker 2"}, "y"]}}"#).unwrap();
        assert_eq!(s.summary, "- eins\n- zwei");
        assert_eq!(s.decisions, vec!["d1", "d2"]);
        assert_eq!(s.action_items.len(), 2);
        assert_eq!(s.action_items[0].owner.as_deref(), Some("Speaker 2"));
        assert_eq!(s.action_items[1].owner, None);
    }

    #[test]
    fn closes_truncated_json() {
        let s =
            parse_summary_json(r#"{"title": "T", "summary": "S", "decisions": ["a", "b"#).unwrap();
        assert_eq!(s.title, "T");
        assert_eq!(s.decisions, vec!["a", "b"]);
    }

    #[test]
    fn rejects_garbage() {
        assert!(parse_summary_json("Sorry, I cannot help with that.").is_err());
        assert!(parse_summary_json("{\"foo\": 1, \"bar\": 2}").is_err());
    }

    #[test]
    fn braces_inside_strings_do_not_confuse_extraction() {
        let s = parse_summary_json(
            r#"{"title":"A {b}","summary":"x } y","decisions":[],"action_items":[]} trailing {"#,
        )
        .unwrap();
        assert_eq!(s.title, "A {b}");
        assert_eq!(s.summary, "x } y");
    }

    #[test]
    fn cleans_owners_and_duplicates() {
        let s = clean_summary(
            MeetingSummary {
                title: " \"Planung.\" ".into(),
                summary: "x".into(),
                decisions: vec!["A".into(), "a".into(), " ".into()],
                action_items: vec![
                    ActionItem {
                        owner: Some("null".into()),
                        text: "t1".into(),
                    },
                    ActionItem {
                        owner: Some("speaker 2".into()),
                        text: "t2".into(),
                    },
                    ActionItem {
                        owner: None,
                        text: "T1".into(),
                    },
                ],
            },
            &["Ich".into(), "Speaker 2".into()],
        );
        assert_eq!(s.title, "Planung");
        assert_eq!(s.decisions, vec!["A"]);
        assert_eq!(s.action_items.len(), 2);
        assert_eq!(s.action_items[0].owner, None);
        assert_eq!(s.action_items[1].owner.as_deref(), Some("Speaker 2"));
    }

    #[test]
    fn chunking_respects_budget_and_overlaps() {
        let text: String = (0..100)
            .map(|i| format!("[{:02}:00] Ich: Satz Nummer {i} mit etwas Text.\n", i % 60))
            .collect();
        let chunks = chunk_lines(&text, 200); // 700 chars
        assert!(chunks.len() > 3);
        for c in &chunks {
            assert!(c.chars().count() <= 700, "{}", c.len());
        }
        // Overlap: the last line of chunk 0 appears in chunk 1.
        let last = chunks[0].lines().last().unwrap();
        assert!(chunks[1].contains(last));
        // Everything is covered.
        for i in 0..100 {
            assert!(chunks.iter().any(|c| c.contains(&format!("Nummer {i} "))));
        }
    }

    #[test]
    fn chunking_splits_a_huge_line() {
        let line = "wort ".repeat(1000);
        let chunks = chunk_lines(&line, 100);
        assert!(chunks.len() >= 10);
        assert!(chunks.iter().all(|c| c.chars().count() <= 350));
    }

    #[test]
    fn span_labels() {
        assert_eq!(span_label("[00:05] A: x\n[12:30] B: y"), "00:05 – 12:30");
        assert_eq!(span_label("no stamps"), "?");
    }

    /// A one-shot HTTP server: answers each request with the next canned
    /// (status, body) and hands back what it received (headers + body).
    fn fake_server(replies: Vec<(u16, String)>) -> (String, std::sync::mpsc::Receiver<String>) {
        use std::io::{BufRead, BufReader, Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            for (status, body) in replies {
                let (mut stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut head = String::new();
                let mut len = 0;
                loop {
                    let mut l = String::new();
                    reader.read_line(&mut l).unwrap();
                    if let Some(v) = l.to_ascii_lowercase().strip_prefix("content-length:") {
                        len = v.trim().parse().unwrap();
                    }
                    if l == "\r\n" {
                        break;
                    }
                    head.push_str(&l);
                }
                let mut buf = vec![0; len];
                reader.read_exact(&mut buf).unwrap();
                tx.send(format!("{head}\n{}", String::from_utf8_lossy(&buf)))
                    .unwrap();
                let resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                stream.write_all(resp.as_bytes()).unwrap();
            }
        });
        (url, rx)
    }

    #[test]
    fn anthropic_request_shape_and_reply() {
        let summary = r#"{"title":"T","summary":["a"],"decisions":[],"action_items":[{"owner":"Ben","text":"x"}]}"#;
        let ok = json!({
            "stop_reason": "end_turn",
            "content": [{"type": "thinking", "thinking": ""}, {"type": "text", "text": summary}],
        })
        .to_string();
        // First reply refuses `fallbacks`; the client must retry without it.
        let (url, rx) = fake_server(vec![
            (
                400,
                r#"{"error":{"message":"fallbacks: unknown parameter"}}"#.into(),
            ),
            (200, ok),
        ]);
        let mut cfg = LlmConfig::anthropic("claude-opus-5-5", Some("k-123".into()));
        cfg.base_url = url;
        let llm = LlmSummarizer::new(cfg);
        let t = Transcript {
            segments: vec![crate::Segment {
                start_s: 0.0,
                end_s: 1.0,
                track: crate::TrackKind::System,
                speaker: "Ben".into(),
                text: "Ich schicke das morgen.".into(),
            }],
        };
        let s = llm.summarize(&t).unwrap();
        assert_eq!(s.title, "T");
        assert_eq!(s.action_items[0].owner.as_deref(), Some("Ben"));
        let first = rx.recv().unwrap().to_ascii_lowercase();
        assert!(first.starts_with("post /messages"), "{first}");
        assert!(first.contains("x-api-key: k-123"));
        assert!(first.contains("anthropic-version: 2023-06-01"));
        assert!(first.contains("\"fallbacks\":\"default\""));
        let second = rx.recv().unwrap();
        let body: Value = serde_json::from_str(second.split_once("\n\n").unwrap().1).unwrap();
        assert!(body.get("fallbacks").is_none());
        assert!(body.get("temperature").is_none());
        assert_eq!(body["max_tokens"], 16_000);
        assert!(body["system"].as_str().unwrap().len() > 100);
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["output_config"]["format"]["type"], "json_schema");
        assert!(!second.to_ascii_lowercase().contains("anthropic-beta"));
    }

    #[test]
    fn openai_drops_parameters_the_model_rejects() {
        let ok = json!({"choices": [{"message": {"content": "OK"}}]}).to_string();
        let (url, rx) = fake_server(vec![
            (
                400,
                r#"{"error":{"message":"Unsupported parameter: 'temperature'"}}"#.into(),
            ),
            (
                400,
                r#"{"error":{"message":"Unsupported parameter: 'max_tokens'"}}"#.into(),
            ),
            (200, ok),
        ]);
        let llm = LlmSummarizer::new(LlmConfig {
            temperature: Some(0.5),
            ..LlmConfig::openai_compatible(url, "m", Some("k".into()))
        });
        assert_eq!(llm.chat_plain("s", "u").unwrap(), "OK");
        let bodies: Vec<Value> = (0..3)
            .map(|_| {
                let r = rx.recv().unwrap();
                serde_json::from_str(r.split_once("\n\n").unwrap().1).unwrap()
            })
            .collect();
        assert!(bodies[0].get("temperature").is_some());
        assert!(bodies[1].get("temperature").is_none());
        assert!(bodies[2].get("max_tokens").is_none());
        assert_eq!(bodies[2]["max_completion_tokens"], 3_072);
        // chat_plain asks for free text: no response_format.
        assert!(bodies[2].get("response_format").is_none());
    }

    #[test]
    fn openai_itself_gets_its_parameters_from_the_start() {
        let llm = LlmSummarizer::new(LlmConfig::openai_compatible(
            "https://api.openai.com/v1",
            "m",
            None,
        ));
        assert_eq!(
            llm.oa_params.load(Ordering::Relaxed),
            OA_NO_TEMPERATURE | OA_COMPLETION_TOKENS
        );
    }

    /// Real request to a cloud model with a made-up two-line meeting. Needs
    /// MEETING_CLOUD=anthropic|openai|openrouter, MEETING_CLOUD_MODEL and
    /// the provider's API key in MEETING_CLOUD_KEY (costs a fraction of a cent):
    ///   cargo test -p meeting-core cloud_summary_smoke -- --ignored --nocapture
    #[test]
    #[ignore]
    fn cloud_summary_smoke() {
        use crate::{Segment, TrackKind};
        let provider = std::env::var("MEETING_CLOUD").unwrap();
        let model = std::env::var("MEETING_CLOUD_MODEL").unwrap();
        let key = std::env::var("MEETING_CLOUD_KEY").ok();
        let mut cfg = match provider.as_str() {
            "anthropic" => LlmConfig::anthropic(model, key),
            "openai" => LlmConfig::openai_compatible("https://api.openai.com/v1", model, key),
            _ => LlmConfig::openai_compatible("https://openrouter.ai/api/v1", model, key),
        };
        cfg.max_output_tokens = 4_000;
        let line = |t: f64, sp: &str, text: &str| Segment {
            start_s: t,
            end_s: t + 5.0,
            track: TrackKind::System,
            speaker: sp.into(),
            text: text.into(),
        };
        let transcript = Transcript {
            segments: vec![
                line(
                    0.0,
                    "Anna",
                    "Wir verschieben den Launch der neuen Website auf Montag.",
                ),
                line(
                    6.0,
                    "Ben",
                    "Okay, dann schicke ich dem Kunden bis Freitag die Texte.",
                ),
            ],
        };
        let llm = LlmSummarizer::new(cfg);
        let plain = llm
            .chat_plain("Answer with the single word OK.", "Ping")
            .unwrap();
        println!("plain: {plain}");
        let llm = LlmSummarizer::new(llm.config().clone());
        let (s, stats) = llm.summarize_with_stats(&transcript).unwrap();
        println!("{s:#?}\n{stats:?}");
        assert!(!s.summary.is_empty());
        assert!(!s.action_items.is_empty());
    }
}
