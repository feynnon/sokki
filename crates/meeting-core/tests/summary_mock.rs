//! Summariser tests against a scripted local HTTP server (no real LLM).

mod common;

use std::time::{Duration, Instant};

use common::*;
use meeting_core::lang::Language;
use meeting_core::summary::{LlmConfig, LlmSummarizer};
use meeting_core::Summarizer;
use serde_json::json;

const GOOD: &str = r#"{"title":"Herbstkampagne und Budget","summary":"- Kosten pro Lead bei 18 Euro\n- Budget Dezember auf 12.500 Euro erhöht","decisions":["Dezember-Budget auf 12.500 Euro erhöht"],"action_items":[{"owner":"Speaker 2","text":"Relaunch der Landingpage bis Freitag"},{"owner":null,"text":"Teilnehmer informieren"}]}"#;

fn ollama_cfg(server: &MockServer) -> LlmConfig {
    LlmConfig {
        base_url: server.base_url.clone(),
        timeout: Duration::from_secs(10),
        ..LlmConfig::ollama("qwen-test")
    }
}

#[test]
fn ollama_well_formed_json() {
    let server = MockServer::scripted(vec![(200, ollama_reply(GOOD))]);
    let s = LlmSummarizer::new(ollama_cfg(&server));
    let t = load_fixture("de_team_meeting.txt");
    let (sum, stats) = s.summarize_with_stats(&t).unwrap();

    assert_eq!(sum.title, "Herbstkampagne und Budget");
    assert_eq!(sum.decisions.len(), 1);
    assert_eq!(sum.action_items[0].owner.as_deref(), Some("Speaker 2"));
    assert_eq!(sum.action_items[1].owner, None);
    assert_eq!(stats.requests, 1);
    assert!(!stats.map_reduce);
    assert_eq!(stats.language, Some(Language::German));

    let reqs = server.requests();
    assert_eq!(reqs.len(), 1);
    let r = &reqs[0];
    assert_eq!(r.path, "/api/chat");
    assert_eq!(r.body["model"], "qwen-test");
    assert_eq!(r.body["stream"], false);
    assert_eq!(r.body["think"], false);
    assert_eq!(r.body["options"]["num_ctx"], 16384);
    assert_eq!(r.body["format"]["type"], "object");
    assert!(r.body["format"]["required"]
        .as_array()
        .unwrap()
        .contains(&json!("action_items")));
    let sys = r.system();
    assert!(sys.contains("Write all text values in German"), "{sys}");
    assert!(sys.contains("Never invent"), "{sys}");
    assert!(sys.contains("Ich, Speaker 2, Speaker 3"), "{sys}");
    assert!(
        sys.contains("\"Ich\" is the person who recorded the meeting"),
        "{sys}"
    );
    assert!(r
        .user()
        .contains("[01:49] Speaker 2: Ich habe das durchgerechnet."));
    assert!(r.header("authorization").is_none());
}

#[test]
fn openai_fenced_json_with_auth_and_schema() {
    let fenced = format!("Sure! Here are the notes:\n```json\n{GOOD}\n```");
    let server = MockServer::scripted(vec![(200, openai_reply(&fenced))]);
    let cfg = LlmConfig {
        timeout: Duration::from_secs(10),
        ..LlmConfig::openai_compatible(
            format!("{}/v1/", server.base_url),
            "gpt-test",
            Some("sk-test".into()),
        )
    };
    let sum = LlmSummarizer::new(cfg)
        .summarize(&load_fixture("de_team_meeting.txt"))
        .unwrap();
    assert_eq!(sum.title, "Herbstkampagne und Budget");

    let r = &server.requests()[0];
    assert_eq!(r.path, "/v1/chat/completions");
    assert_eq!(r.header("authorization"), Some("Bearer sk-test"));
    assert_eq!(r.body["response_format"]["type"], "json_schema");
    assert_eq!(r.body["model"], "gpt-test");
    assert!(r.body.get("think").is_none());
}

#[test]
fn openai_falls_back_when_response_format_is_rejected() {
    let server = MockServer::scripted(vec![
        (
            400,
            r#"{"error":{"message":"response_format json_schema not supported"}}"#.into(),
        ),
        (
            400,
            r#"{"error":{"message":"response_format not supported"}}"#.into(),
        ),
        (200, openai_reply(GOOD)),
        (200, openai_reply(GOOD)),
    ]);
    let cfg = LlmConfig {
        timeout: Duration::from_secs(10),
        ..LlmConfig::openai_compatible(server.base_url.clone(), "m", None)
    };
    let s = LlmSummarizer::new(cfg);
    let t = load_fixture("de_team_meeting.txt");
    s.summarize(&t).unwrap();
    // The working mode is remembered: the second summary needs one request.
    s.summarize(&t).unwrap();
    let reqs = server.requests();
    assert_eq!(reqs.len(), 4);
    assert_eq!(reqs[0].body["response_format"]["type"], "json_schema");
    assert_eq!(reqs[1].body["response_format"]["type"], "json_object");
    assert!(reqs[2].body.get("response_format").is_none());
    assert!(reqs[3].body.get("response_format").is_none());
}

#[test]
fn ollama_model_without_thinking_support_is_retried_without_think() {
    let server = MockServer::scripted(vec![
        (
            400,
            r#"{"error":"\"llama3\" does not support thinking"}"#.into(),
        ),
        (200, ollama_reply(GOOD)),
    ]);
    let sum = LlmSummarizer::new(ollama_cfg(&server))
        .summarize(&load_fixture("de_team_meeting.txt"))
        .unwrap();
    assert_eq!(sum.title, "Herbstkampagne und Budget");
    let reqs = server.requests();
    assert_eq!(reqs[0].body["think"], false);
    assert!(reqs[1].body.get("think").is_none());
}

#[test]
fn ollama_without_structured_output_falls_back_to_prompt_only_json() {
    let server = MockServer::scripted(vec![
        (
            501,
            r#"{"error":"structured output is unavailable"}"#.into(),
        ),
        (200, ollama_reply(&format!("```json\n{GOOD}\n```"))),
        (200, ollama_reply(GOOD)),
    ]);
    let s = LlmSummarizer::new(ollama_cfg(&server));
    let t = load_fixture("de_team_meeting.txt");
    assert_eq!(s.summarize(&t).unwrap().title, "Herbstkampagne und Budget");
    s.summarize(&t).unwrap();
    let reqs = server.requests();
    assert_eq!(reqs.len(), 3);
    assert!(reqs[0].body.get("format").is_some());
    assert!(reqs[1].body.get("format").is_none());
    // Remembered for the next summary.
    assert!(reqs[2].body.get("format").is_none());
}

#[test]
fn ollama_rejecting_schema_format_tries_plain_json_format() {
    let server = MockServer::scripted(vec![
        (
            400,
            r#"{"error":"invalid format: schema not supported"}"#.into(),
        ),
        (200, ollama_reply(GOOD)),
    ]);
    LlmSummarizer::new(ollama_cfg(&server))
        .summarize(&load_fixture("de_team_meeting.txt"))
        .unwrap();
    let reqs = server.requests();
    assert_eq!(reqs[0].body["format"]["type"], "object");
    assert_eq!(reqs[1].body["format"], "json");
}

#[test]
fn malformed_reply_is_fixed_on_retry() {
    let server = MockServer::scripted(vec![
        (
            200,
            ollama_reply("Hier ist die Zusammenfassung: Es ging um das Budget."),
        ),
        (200, ollama_reply(GOOD)),
    ]);
    let s = LlmSummarizer::new(ollama_cfg(&server));
    let (sum, stats) = s
        .summarize_with_stats(&load_fixture("de_team_meeting.txt"))
        .unwrap();
    assert_eq!(sum.title, "Herbstkampagne und Budget");
    assert_eq!(stats.requests, 2);
    assert_eq!(stats.parse_retries, 1);
    let reqs = server.requests();
    let retry = &reqs[1];
    let msgs = retry.body["messages"].as_array().unwrap();
    assert_eq!(msgs.len(), 2, "retry is a fresh request, not a replay");
    let u = retry.user();
    assert!(u.contains("not valid JSON"), "{u}");
    assert!(u.contains(r#""summary": ["...", "..."]"#), "{u}");
    assert!(u.starts_with(&reqs[0].user()));
}

#[test]
fn malformed_twice_is_an_error() {
    let server = MockServer::scripted(vec![
        (200, ollama_reply("kein json")),
        (200, ollama_reply("immer noch kein json")),
    ]);
    let err = LlmSummarizer::new(ollama_cfg(&server))
        .summarize(&load_fixture("de_team_meeting.txt"))
        .unwrap_err();
    assert!(format!("{err:#}").contains("valid JSON"), "{err:#}");
}

#[test]
fn error_status_is_returned() {
    let server = MockServer::scripted(vec![(
        404,
        r#"{"error":"model 'qwen-test' not found"}"#.into(),
    )]);
    let err = LlmSummarizer::new(ollama_cfg(&server))
        .summarize(&load_fixture("de_team_meeting.txt"))
        .unwrap_err();
    let msg = format!("{err:#}");
    assert!(
        msg.contains("HTTP 404") && msg.contains("not found"),
        "{msg}"
    );

    let server = MockServer::scripted(vec![(500, "internal".into())]);
    let cfg = LlmConfig {
        timeout: Duration::from_secs(10),
        ..LlmConfig::openai_compatible(server.base_url.clone(), "m", None)
    };
    let err = LlmSummarizer::new(cfg)
        .summarize(&load_fixture("de_team_meeting.txt"))
        .unwrap_err();
    assert!(format!("{err:#}").contains("HTTP 500"));
}

#[test]
fn connection_refused_is_an_error_not_a_panic() {
    // Bind and drop to get a port nobody listens on.
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let cfg = LlmConfig {
        base_url: format!("http://127.0.0.1:{port}"),
        timeout: Duration::from_secs(5),
        ..LlmConfig::ollama("m")
    };
    assert!(LlmSummarizer::new(cfg)
        .summarize(&load_fixture("de_team_meeting.txt"))
        .is_err());
}

#[test]
fn timeout_is_respected() {
    let server = MockServer::start_with_delay(
        Box::new(|_, _| (200, ollama_reply(GOOD))),
        Some(Duration::from_secs(5)),
    );
    let cfg = LlmConfig {
        timeout: Duration::from_millis(500),
        prompt_ms_per_token: 0,
        ..ollama_cfg(&server)
    };
    let started = Instant::now();
    let res = LlmSummarizer::new(cfg).summarize(&load_fixture("de_team_meeting.txt"));
    assert!(res.is_err());
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "{:?}",
        started.elapsed()
    );
}

#[test]
fn english_transcript_asks_for_english() {
    let t = parse_transcript(
        "[00:00] Ich: Hi everyone, thanks for joining. Let's go through the launch plan.\n\
         [00:05] Speaker 2: Sure. The landing page is ready and we will ship it on Friday.\n",
    );
    let server = MockServer::scripted(vec![(200, ollama_reply(GOOD))]);
    let (_, stats) = LlmSummarizer::new(ollama_cfg(&server))
        .summarize_with_stats(&t)
        .unwrap();
    assert_eq!(stats.language, Some(Language::English));
    assert!(server.requests()[0]
        .system()
        .contains("Write all text values in English."));
}

#[test]
fn empty_transcript_is_an_error_without_request() {
    let server = MockServer::scripted(vec![]);
    let res = LlmSummarizer::new(ollama_cfg(&server)).summarize(&Default::default());
    assert!(res.is_err());
    assert!(server.requests().is_empty());
}

/// Map step answers echo which part they saw; the reduce step returns the
/// final summary.
fn map_reduce_responder(partial_summary_len: usize) -> Responder {
    Box::new(move |_, req| {
        let sys = req.system();
        let user = req.user();
        if sys.contains("one part of a long meeting") {
            let part = user.split_whitespace().nth(1).unwrap_or("?").to_string(); // "Part N of M"
            let body = json!({
                "title": format!("Teil {part}"),
                "summary": format!("- Notiz zu Teil {part}: {}", "x".repeat(partial_summary_len)),
                "decisions": [format!("Entscheidung aus Teil {part}")],
                "action_items": [{"owner": "Speaker 2", "text": format!("Aufgabe aus Teil {part}")}],
            });
            (200, ollama_reply(&body.to_string()))
        } else if sys.contains("for that stretch of the meeting") {
            let body = json!({
                "title": "Zwischenstand",
                "summary": format!("- Zwischen: {}", user.lines().filter(|l| l.starts_with("Part ")).count()),
                "decisions": ["Zwischenentscheidung"],
                "action_items": [],
            });
            (200, ollama_reply(&body.to_string()))
        } else {
            (200, ollama_reply(GOOD))
        }
    })
}

fn small_budget(server: &MockServer) -> LlmConfig {
    // transcript budget = 2400 - 600 - 900 = 900 tokens (~3150 chars)
    LlmConfig {
        context_tokens: 2400,
        max_output_tokens: 600,
        ..ollama_cfg(server)
    }
}

#[test]
fn map_reduce_is_used_when_over_budget() {
    let server = MockServer::start(map_reduce_responder(10));
    let t = load_fixture("de_team_meeting.txt");
    let (sum, stats) = LlmSummarizer::new(small_budget(&server))
        .summarize_with_stats(&t)
        .unwrap();
    assert!(stats.map_reduce);
    assert!(stats.chunks >= 2, "{stats:?}");
    assert_eq!(sum.title, "Herbstkampagne und Budget");

    let reqs = server.requests();
    assert_eq!(reqs.len(), stats.chunks + 1);
    // Every map request fits the budget and names its part and time span.
    for (i, r) in reqs[..stats.chunks].iter().enumerate() {
        let u = r.user();
        assert!(
            u.starts_with(&format!("Part {} of {}", i + 1, stats.chunks)),
            "{u}"
        );
        assert!(u.len() / 3 < 1100 * 3, "chunk too long: {}", u.len());
        assert!(r.system().contains("German"));
    }
    // First chunk starts at the beginning, last chunk contains the end.
    assert!(reqs[0].user().contains("[00:00] Ich: Hallo zusammen"));
    assert!(reqs[stats.chunks - 1]
        .user()
        .contains("[04:53] Ich: Danke euch"));
    // The reduce request carries all partial notes.
    let reduce = reqs.last().unwrap();
    assert!(reduce.system().contains("final meeting notes"));
    for i in 1..=stats.chunks {
        assert!(
            reduce.user().contains(&format!("Aufgabe aus Teil {i}")),
            "{}",
            reduce.user()
        );
    }
    assert!(reduce.user().contains("[Speaker 2]"));
    // All requests use the same num_ctx (no model reloads).
    assert!(reqs.iter().all(|r| r.body["options"]["num_ctx"] == 2400));
}

#[test]
fn map_reduce_reduces_in_groups_when_partials_are_too_long() {
    // Each partial note is ~1500 chars, so all of them together exceed the
    // ~3150 char budget and must be combined in groups first.
    let server = MockServer::start(map_reduce_responder(1500));
    let t = load_fixture("long_meeting_60min.txt");
    let (sum, stats) = LlmSummarizer::new(small_budget(&server))
        .summarize_with_stats(&t)
        .unwrap();
    assert!(stats.map_reduce && stats.chunks > 10, "{stats:?}");
    assert_eq!(sum.title, "Herbstkampagne und Budget");
    let reqs = server.requests();
    let intermediate = reqs
        .iter()
        .filter(|r| r.system().contains("for that stretch of the meeting"))
        .count();
    assert!(intermediate >= 2, "intermediate reduces: {intermediate}");
    assert!(reqs
        .last()
        .unwrap()
        .system()
        .contains("final meeting notes"));
}

#[test]
fn default_budget_single_pass_for_5min_map_reduce_for_60min() {
    let server = MockServer::start(map_reduce_responder(10));
    let (_, stats) = LlmSummarizer::new(ollama_cfg(&server))
        .summarize_with_stats(&load_fixture("long_meeting_60min.txt"))
        .unwrap();
    // 60 min (~17k tokens) does not fit the default 16k context: map-reduce.
    assert!(stats.map_reduce);
    let server = MockServer::start(map_reduce_responder(10));
    let (_, stats) = LlmSummarizer::new(ollama_cfg(&server))
        .summarize_with_stats(&load_fixture("de_team_meeting.txt"))
        .unwrap();
    assert!(!stats.map_reduce);
    assert_eq!(stats.chunks, 0);
}

/// A 40-minute meeting (~10k tokens) fits the default 16k context.
fn forty_minutes() -> meeting_core::Transcript {
    let mut t = load_fixture("long_meeting_60min.txt");
    t.segments.truncate(t.segments.len() * 2 / 3);
    let tokens = meeting_core::lang::estimate_tokens(&t.to_plain_text());
    assert!((9_000..12_000).contains(&tokens), "{tokens} tokens");
    t
}

#[test]
fn a_40_minute_meeting_goes_to_a_local_model_in_one_request() {
    // Measured 20.11.2026: one request 147 s, two parts plus merging 282 s.
    let server = MockServer::start(map_reduce_responder(10));
    let (_, stats) = LlmSummarizer::new(ollama_cfg(&server))
        .summarize_with_stats(&forty_minutes())
        .unwrap();
    assert!(!stats.map_reduce, "{stats:?}");
    assert_eq!(server.requests().len(), 1);
}

#[test]
fn a_single_pass_limit_splits_into_parts_of_equal_size() {
    let server = MockServer::start(map_reduce_responder(10));
    let cfg = LlmConfig {
        single_pass_tokens: 6_000,
        ..ollama_cfg(&server)
    };
    let (sum, stats) = LlmSummarizer::new(cfg)
        .summarize_with_stats(&forty_minutes())
        .unwrap();
    assert!(stats.map_reduce);
    assert_eq!(stats.chunks, 2, "{stats:?}");
    assert_eq!(sum.title, "Herbstkampagne und Budget");
    // Two parts of about the same size, not 6k + a short remainder.
    let parts: Vec<usize> = server
        .requests()
        .iter()
        .filter(|r| r.system().contains("one part of a long meeting"))
        .map(|r| r.user().len())
        .collect();
    assert_eq!(parts.len(), 2);
    let (a, b) = (parts[0] as f64, parts[1] as f64);
    assert!((a / b - 1.0).abs() < 0.25, "part sizes {parts:?}");
}
