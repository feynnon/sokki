//! Runs the real engines on the synthetic meeting fixtures
//! (`testdata/generate.py`). Models are downloaded on first run into
//! `~/Library/Caches/handy-meetings/diarize-models` (override with
//! `DIARIZE_MODELS_DIR`). Fixtures are read from `MEETING_TESTDATA`
//! (default: the main checkout's `testdata/audio`); tests skip with a
//! message when they are missing.
//!
//! Print the comparison table:
//!   cargo test --release --test fixtures report -- --nocapture

mod common;

use common::{assert_contract, diarizer, fixture, load, run_with, score, speakers, Backend};
use diarize::{cosine, SpeakerEmbedder};

/// (scenario, track, expected speakers or None when there is no single right
/// answer, e.g. the mic track of `call_tracks` also hears the remote side).
const CASES: &[(&str, &str, Option<usize>)] = &[
    ("de_two", "mic", Some(2)),
    ("de_en_mixed", "mic", Some(2)),
    ("phone_speaker", "mic", Some(2)),
    ("call_tracks", "mic", None),
    ("call_tracks", "system", Some(2)),
    ("three_remote", "system", Some(3)),
    ("silence", "mic", Some(0)),
    ("three_distinct", "-", Some(3)),
    ("long_mix", "-", Some(3)),
];

#[test]
fn report() {
    let mut rows = Vec::new();
    // DIARIZE_BACKENDS=sortformer,sherpa limits the backends compared.
    let wanted = std::env::var("DIARIZE_BACKENDS").unwrap_or_default();
    for backend in [Backend::Sortformer, Backend::Sherpa] {
        if !wanted.is_empty() && !wanted.split(',').any(|w| w == backend.name()) {
            continue;
        }
        let d = diarizer(backend);
        for (scenario, track, expected) in CASES {
            let Some(fx) = fixture(scenario, track) else {
                continue;
            };
            let r = run_with(d.as_ref(), &fx);
            assert_contract(&r.turns, fx.duration_s());
            let s = score(&fx.truth, &r.turns, fx.duration_s());
            rows.push(format!(
                "| {:<10} | {:<14} | {:<6} | {:>8} | {:>5} | {:>8.3} | {:>6.3} | {:>6.3} | {:>8.2} | {} |",
                backend.name(),
                scenario,
                track,
                expected.map_or("-".into(), |e| e.to_string()),
                speakers(&r.turns).len(),
                s.accuracy,
                s.missed,
                s.false_alarm,
                r.seconds / fx.duration_s() * 60.0,
                {
                    let mut m: Vec<String> =
                        s.mapping.iter().map(|(t, h)| format!("{t}->{h}")).collect();
                    m.sort();
                    m.join(" ")
                },
            ));
        }
    }
    println!(
        "\n| backend    | scenario      | track  | expected | found | accuracy | missed | f.alarm | s/min audio | truth->found |"
    );
    println!("|---|---|---|---|---|---|---|---|---|---|");
    for r in rows {
        println!("{r}");
    }
}

/// Voices behind the truth labels, per (scenario, track, label).
const VOICES: &[(&str, &str, &str, &str)] = &[
    ("de_two", "mic", "A", "Anna"),
    ("de_two", "mic", "B", "Eddy"),
    ("de_en_mixed", "mic", "A", "Anna"),
    ("de_en_mixed", "mic", "B", "Daniel"),
    ("phone_speaker", "mic", "A", "Anna"),
    ("phone_speaker", "mic", "B", "Reed(phone)"),
    ("call_tracks", "system", "R1", "Eddy"),
    ("call_tracks", "system", "R2", "Flo"),
    ("three_remote", "system", "R1", "Eddy"),
    ("three_remote", "system", "R2", "Flo"),
    ("three_remote", "system", "R3", "Reed"),
];

/// Diagnostic: cosine similarity between ground-truth voices, embedded from
/// the truth spans with the configured embedding model. Shows how separable
/// the synthetic voices are, independent of segmentation.
#[test]
fn voice_similarity() {
    let cfg = common::config();
    let emb = SpeakerEmbedder::new(&cfg).expect("load embedder");
    let mut rows: Vec<(String, Vec<f32>)> = Vec::new();
    for (scenario, track, label, voice) in VOICES {
        let Some(fx) = load(scenario, track) else {
            return;
        };
        let spans: Vec<(f64, f64)> = fx
            .truth
            .iter()
            .filter(|t| t.speaker == *label)
            .map(|t| (t.start_s, t.end_s))
            .collect();
        let e = emb.embed_spans(&fx.samples, &spans).expect("embed");
        rows.push((format!("{voice}@{scenario}"), e));
    }
    println!("\nembedding model: {}", cfg.embedding_model.id());
    for (i, (a, ea)) in rows.iter().enumerate() {
        let line: Vec<String> = rows
            .iter()
            .take(i)
            .map(|(_, eb)| format!("{:5.2}", cosine(ea, eb)))
            .collect();
        println!("{a:<26} {}", line.join(" "));
    }
}

/// Runs `backend` on `cases` and asserts speaker count and accuracy.
fn check(backend: Backend, cases: &[(&str, &str, usize, f64)]) {
    let d = diarizer(backend);
    let mut failures = Vec::new();
    for (scenario, track, expected, min_acc) in cases {
        let Some(fx) = fixture(scenario, track) else {
            continue;
        };
        let r = run_with(d.as_ref(), &fx);
        assert_contract(&r.turns, fx.duration_s());
        let s = score(&fx.truth, &r.turns, fx.duration_s());
        let found = speakers(&r.turns).len();
        println!(
            "{} {scenario}/{track}: speakers {found} (expected {expected}), accuracy {:.3}, \
             {:.2} s per minute of audio",
            backend.name(),
            s.accuracy,
            r.seconds / fx.duration_s() * 60.0
        );
        if found != *expected {
            failures.push(format!(
                "{scenario}/{track}: {found} speakers, expected {expected}"
            ));
        }
        if s.accuracy < *min_acc {
            failures.push(format!(
                "{scenario}/{track}: accuracy {:.3} < {min_acc}",
                s.accuracy
            ));
        }
    }
    assert!(failures.is_empty(), "{}: {failures:#?}", backend.name());
}

#[test]
fn sortformer_fixtures() {
    check(
        Backend::Sortformer,
        &[
            ("de_two", "mic", 2, 0.9),
            ("de_en_mixed", "mic", 2, 0.9),
            ("phone_speaker", "mic", 2, 0.9),
            ("call_tracks", "system", 2, 0.9),
            ("silence", "mic", 0, 1.0),
            ("three_distinct", "-", 3, 0.9),
            ("long_mix", "-", 3, 0.9),
        ],
    );
}

#[test]
fn sherpa_fixtures() {
    check(
        Backend::Sherpa,
        &[
            ("de_two", "mic", 2, 0.9),
            ("de_en_mixed", "mic", 2, 0.9),
            ("phone_speaker", "mic", 2, 0.9),
            ("call_tracks", "system", 2, 0.85),
            ("silence", "mic", 0, 1.0),
        ],
    );
}

/// Known failures, kept visible: run with `-- --ignored`.
/// - `three_remote`: its Eddy and Reed voices are near identical for every
///   embedding model tried (cosine 0.83-0.96 vs. 0.86-0.97 for the same
///   voice), so both backends hear two speakers. A fixture problem.
/// - sherpa on `three_distinct` / `long_mix`: merges Eddy and Daniel at the
///   clustering threshold that keeps the two-speaker cases right.
#[test]
#[ignore = "known failures, see doc comment"]
fn known_failures() {
    let mut failed = 0;
    for (backend, scenario, track) in [
        (Backend::Sortformer, "three_remote", "system"),
        (Backend::Sherpa, "three_remote", "system"),
        (Backend::Sherpa, "three_distinct", "-"),
        (Backend::Sherpa, "long_mix", "-"),
    ] {
        let Some(fx) = fixture(scenario, track) else {
            return;
        };
        let r = run_with(diarizer(backend).as_ref(), &fx);
        let s = score(&fx.truth, &r.turns, fx.duration_s());
        let found = speakers(&r.turns).len();
        println!(
            "{} {scenario}: {found} speakers (expected 3), accuracy {:.3}",
            backend.name(),
            s.accuracy
        );
        if found != 3 || s.accuracy < 0.85 {
            failed += 1;
        }
    }
    assert_eq!(failed, 0, "{failed} known failures still fail");
}

/// Enroll Anna (speaker A) from `de_two`, then recognise her in
/// `de_en_mixed`, and do not mistake Eddy or Daniel for her.
fn voice_profiles(backend: Backend) {
    use diarize::VoiceProfiles;
    let (Some(two), Some(mixed), Some(phone)) = (
        load("de_two", "mic"),
        load("de_en_mixed", "mic"),
        load("phone_speaker", "mic"),
    ) else {
        return;
    };
    let d = diarizer(backend);
    let cluster_embedding = |fx: &common::Fixture, truth_label: &str| -> Vec<f32> {
        let r = run_with(d.as_ref(), fx);
        assert!(
            r.turns.iter().all(|t| t.embedding.is_some()),
            "turn without embedding"
        );
        let s = score(&fx.truth, &r.turns, fx.duration_s());
        let label = &s.mapping[truth_label];
        r.turns
            .iter()
            .find(|t| &t.speaker == label)
            .and_then(|t| t.embedding.clone())
            .unwrap()
    };

    let path = std::env::temp_dir().join(format!(
        "diarize-voices-{}-{}.json",
        backend.name(),
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    let mut voices = VoiceProfiles::open(&path).unwrap();
    voices.enroll("A", &cluster_embedding(&two, "A")).unwrap();

    let anna = cluster_embedding(&mixed, "A");
    let daniel = cluster_embedding(&mixed, "B");
    let eddy = cluster_embedding(&two, "B");
    let anna_phone_scene = cluster_embedding(&phone, "A");
    let reed_phone = cluster_embedding(&phone, "B");
    for (who, e) in [
        ("Anna (de_en_mixed)", &anna),
        ("Anna (phone_speaker)", &anna_phone_scene),
        ("Eddy (de_two)", &eddy),
        ("Daniel (de_en_mixed)", &daniel),
        ("Reed via phone (phone_speaker)", &reed_phone),
    ] {
        println!(
            "{}: {who} vs enrolled A: {:.3} -> {:?}",
            backend.name(),
            voices.scores(e)[0].1,
            voices.identify(e)
        );
    }
    assert_eq!(voices.identify(&anna).map(|m| m.0).as_deref(), Some("A"));
    assert_eq!(voices.identify(&daniel), None);
    assert_eq!(voices.identify(&eddy), None);
    assert_eq!(voices.identify(&reed_phone), None);
    // Persisted and reloadable.
    let again = VoiceProfiles::open(&path).unwrap();
    assert_eq!(again.identify(&anna).map(|m| m.0).as_deref(), Some("A"));
    let _ = std::fs::remove_file(&path);
}

#[test]
fn voice_profiles_sortformer() {
    voice_profiles(Backend::Sortformer);
}

#[test]
fn voice_profiles_sherpa() {
    voice_profiles(Backend::Sherpa);
}

/// Slow: `long_mix` repeated to ~16 minutes, to see how accuracy and runtime
/// scale toward real meeting lengths. Run with `-- --ignored long_meeting`.
#[test]
#[ignore = "slow (~16 min of audio per backend)"]
fn long_meeting() {
    let Some(one) = common::derived("long_mix") else {
        return;
    };
    let reps = 5;
    let dur = one.duration_s();
    let mut fx = common::Fixture {
        samples: Vec::new(),
        truth: Vec::new(),
    };
    for i in 0..reps {
        fx.samples.extend_from_slice(&one.samples);
        for t in &one.truth {
            let mut t = t.clone();
            t.start_s += i as f64 * dur;
            t.end_s += i as f64 * dur;
            fx.truth.push(t);
        }
    }
    for backend in [Backend::Sortformer, Backend::Sherpa] {
        let r = run_with(diarizer(backend).as_ref(), &fx);
        assert_contract(&r.turns, fx.duration_s());
        let s = score(&fx.truth, &r.turns, fx.duration_s());
        println!(
            "{} long_meeting ({:.1} min): {} speakers (expected 3), accuracy {:.3}, {:.2} s per minute",
            backend.name(),
            fx.duration_s() / 60.0,
            speakers(&r.turns).len(),
            s.accuracy,
            r.seconds / fx.duration_s() * 60.0
        );
    }
}
