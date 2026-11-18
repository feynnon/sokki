//! Live tests against a local Ollama. Skipped unless `MEETING_LIVE_LLM=1`.
//!
//! ```sh
//! MEETING_LIVE_LLM=1 cargo test --test live_llm -- --nocapture --test-threads=1
//! ```
//!
//! Env: `MEETING_LLM_MODEL` (default `qwen3.5:9b`), `OLLAMA_URL` (default
//! `http://localhost:11434`). Each test prints timings and the summary, and
//! writes the rendered note to `$CARGO_TARGET_TMPDIR/live-notes/`.

mod common;

use std::time::Duration;

use common::*;
use meeting_core::lang::{detect_language, Language};
use meeting_core::note::write_note;
use meeting_core::summary::{LlmConfig, LlmSummarizer, SummaryStats};
use meeting_core::{MeetingMeta, MeetingNote, MeetingSummary, Transcript};

fn live() -> Option<LlmSummarizer> {
    if std::env::var("MEETING_LIVE_LLM").as_deref() != Ok("1") {
        eprintln!("skipped: set MEETING_LIVE_LLM=1 to run live LLM tests");
        return None;
    }
    let model = std::env::var("MEETING_LLM_MODEL").unwrap_or_else(|_| "qwen3.5:9b".into());
    let mut cfg = LlmConfig::ollama(model);
    if let Ok(url) = std::env::var("OLLAMA_URL") {
        cfg.base_url = url;
    }
    cfg.timeout = Duration::from_secs(600);
    Some(LlmSummarizer::new(cfg))
}

/// Everything the summary says, lower-cased, with digit grouping removed
/// ("12.500 €" → "12500 €") so amounts can be matched in any format.
fn all_text(s: &MeetingSummary) -> String {
    let mut t = format!("{}\n{}\n", s.title, s.summary);
    for d in &s.decisions {
        t.push_str(d);
        t.push('\n');
    }
    for a in &s.action_items {
        t.push_str(&format!(
            "{}: {}\n",
            a.owner.as_deref().unwrap_or("-"),
            a.text
        ));
    }
    let chars: Vec<char> = t.to_lowercase().chars().collect();
    let mut out = String::new();
    for (i, &c) in chars.iter().enumerate() {
        let between_digits = i > 0
            && i + 1 < chars.len()
            && chars[i - 1].is_ascii_digit()
            && chars[i + 1].is_ascii_digit();
        if between_digits && matches!(c, '.' | ' ' | '\u{a0}' | '\u{202f}' | '\'') {
            continue;
        }
        out.push(c);
    }
    out
}

fn summary_language(s: &MeetingSummary) -> Option<Language> {
    detect_language(&format!(
        "{} {} {}",
        s.title,
        s.summary,
        s.decisions.join(" ")
    ))
}

fn report(name: &str, t: &Transcript, s: &MeetingSummary, stats: &SummaryStats, duration_s: f64) {
    eprintln!(
        "\n===== {name}: {:.1}s, ~{} tokens in, map_reduce={} chunks={} requests={} parse_retries={} lang={:?}",
        stats.elapsed.as_secs_f64(),
        stats.estimated_tokens,
        stats.map_reduce,
        stats.chunks,
        stats.requests,
        stats.parse_retries,
        stats.language
    );
    eprintln!("{}", serde_json::to_string_pretty(s).unwrap());
    let dir = std::path::PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("live-notes");
    let note = MeetingNote {
        meta: MeetingMeta {
            started_at: "2026-11-17T10:00:00+01:00".into(),
            duration_s,
            source_app: None,
        },
        transcript: t.clone(),
        summary: Some(s.clone()),
    };
    match write_note(&dir, &note) {
        Ok(p) => eprintln!("note: {}", p.display()),
        Err(e) => eprintln!("could not write note: {e:#}"),
    }
}

fn has_item(s: &MeetingSummary, owners: &[&str], words: &[&str]) -> bool {
    s.action_items.iter().any(|a| {
        let owner_ok = owners.is_empty()
            || a.owner
                .as_deref()
                .is_some_and(|o| owners.iter().any(|w| o.eq_ignore_ascii_case(w)));
        let text = a.text.to_lowercase();
        owner_ok && words.iter().any(|w| text.contains(w))
    })
}

/// Checks that are reported but do not fail the test (model wording varies).
fn soft(label: &str, ok: bool) {
    eprintln!("  [{}] {label}", if ok { "ok" } else { "MISS" });
}

#[test]
fn live_german_team_meeting() {
    let Some(llm) = live() else { return };
    let t = load_fixture("de_team_meeting.txt");
    let (s, stats) = llm.summarize_with_stats(&t).expect("summarise");
    report("de_team_meeting", &t, &s, &stats, 298.0);
    let all = all_text(&s);

    assert!(!s.title.is_empty() && !s.summary.is_empty());
    assert!(!s.decisions.is_empty(), "no decisions");
    assert!(s.action_items.len() >= 3, "too few action items");
    assert_eq!(
        summary_language(&s),
        Some(Language::German),
        "summary not German"
    );
    assert!(
        all.contains("12500") || all.contains("zwölftausendfünfhundert") || all.contains("12,5"),
        "budget 12.500 missing"
    );
    assert!(all.contains("freitag"), "deadline Friday missing");
    assert!(
        has_item(&s, &["Speaker 2"], &["landing"]),
        "landing page task not owned by Speaker 2: {:?}",
        s.action_items
    );
    soft(
        "CPL 18 / target 15",
        all.contains("18") && all.contains("15"),
    );
    soft(
        "webinar moved to 16 Dec",
        all.contains("16") && all.contains("dezember"),
    );
    soft(
        "agency by Wednesday owned by Ich",
        has_item(&s, &["Ich"], &["agentur"]) && all.contains("mittwoch"),
    );
    soft(
        "dashboard by Speaker 3",
        has_item(&s, &["Speaker 3"], &["dashboard", "reporting"]),
    );
    soft("no stray English", !all.contains(" the "));
    assert!(!stats.map_reduce);
}

#[test]
fn live_mixed_language_call() {
    let Some(llm) = live() else { return };
    let t = load_fixture("de_en_mixed_call.txt");
    let (s, stats) = llm.summarize_with_stats(&t).expect("summarise");
    report("de_en_mixed_call", &t, &s, &stats, 158.0);
    let all = all_text(&s);

    assert_eq!(stats.language, Some(Language::German));
    assert_eq!(
        summary_language(&s),
        Some(Language::German),
        "summary not German (majority)"
    );
    assert!(
        all.contains("40") || all.contains("vierzig"),
        "40 customers missing"
    );
    assert!(
        all.contains("dienstag") || all.contains("tuesday"),
        "Tuesday missing"
    );
    assert!(
        has_item(
            &s,
            &["Speaker 2", "Daniel"],
            &["einladung", "invite", "review", "termin"]
        ),
        "invite task not owned by Speaker 2/Daniel: {:?}",
        s.action_items
    );
    soft(
        "49 Euro per seat",
        all.contains("49") || all.contains("neunundvierzig"),
    );
    soft(
        "churn analysis by Thursday, Speaker 3",
        has_item(&s, &["Speaker 3"], &["churn"]) && all.contains("donnerstag"),
    );
}

#[test]
fn live_long_meeting_map_reduce() {
    let Some(llm) = live() else { return };
    let t = load_fixture("long_meeting_60min.txt");
    let (s, stats) = llm.summarize_with_stats(&t).expect("summarise");
    report("long_meeting_60min", &t, &s, &stats, 3647.0);
    let all = all_text(&s);

    assert!(stats.map_reduce, "60 min meeting should use map-reduce");
    assert!(stats.chunks >= 2);
    assert_eq!(summary_language(&s), Some(Language::German));
    // Facts from the start, middle and end of the meeting.
    assert!(
        all.contains("480000")
            || all.contains("480 000")
            || all.contains("vierhundertachtzigtausend"),
        "budget 480.000 missing"
    );
    assert!(all.contains("märz"), "CRM go-live in March missing");
    assert!(
        all.contains("köln") || all.contains("messe"),
        "trade fair missing"
    );
    soft(
        "go-live 15 March",
        all.contains("15. märz")
            || all.contains("15 märz")
            || all.contains("15.03")
            || all.contains("fünfzehnten märz"),
    );
    soft("60 % performance", all.contains("60"));
    soft(
        "churn 3,1 -> 4,2",
        all.contains("4,2") || all.contains("4.2"),
    );
    soft(
        "fair 22 April",
        all.contains("22. april") || all.contains("22 april"),
    );
    soft("booth 18.000", all.contains("18000"));
    soft(
        "migration owned by Speaker 4",
        has_item(&s, &["Speaker 4"], &["migration", "daten"]),
    );
    soft(
        "job ads by Speaker 5 by Thursday",
        has_item(&s, &["Speaker 5"], &["stellen", "anzeige"]) && all.contains("donnerstag"),
    );
    soft("office move July", all.contains("juli"));
}
