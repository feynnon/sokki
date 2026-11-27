//! Recall of the search modes on the made-up German set
//! `$SOKKI_EVALS/memory-search-de` (docs/evals.md). Not run by default:
//!
//!     cargo test --test recall_eval -- --ignored --nocapture
//!
//! For each question: is an expected turn among the top k transcript
//! results? Prints recall@1/@5 and MRR per mode and category, and writes
//! them to `$SOKKI_EVALS/runs/memory-search/`.

use meeting_core::note::{render_note, write_segment_lines, NoteOptions};
use meeting_core::{MeetingMeta, MeetingNote, Segment, TrackKind, Transcript};
use serde_json::{json, Value};
use sokki_index::query::{self, ListOpts, Mode, SearchOpts};
use sokki_index::{sync, Index, Paths};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn evals() -> PathBuf {
    std::env::var_os("SOKKI_EVALS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap()).join("sokki-evals"))
}

#[test]
#[ignore = "needs $SOKKI_EVALS/memory-search-de"]
fn recall_by_mode() {
    let set = evals().join("memory-search-de");
    let corpus: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(set.join("data/corpus.json")).expect("corpus.json")).unwrap();
    let questions: Vec<Value> =
        serde_json::from_str(&std::fs::read_to_string(set.join("data/questions.json")).unwrap()).unwrap();

    let root = std::env::temp_dir().join(format!("sokki-recall-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let paths = Paths {
        index: root.join("index.sqlite"),
        meetings_dir: root.join("meetings"),
        history_db: None,
        user_name: "Ich".into(),
    };
    std::fs::create_dir_all(root.join("notes")).unwrap();
    std::fs::create_dir_all(&paths.meetings_dir).unwrap();
    let mut records = Vec::new();
    for m in &corpus {
        let id = m["id"].as_str().unwrap();
        let segments: Vec<Segment> = m["turns"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(i, t)| {
                let speaker = t["speaker"].as_str().unwrap();
                Segment {
                    // One turn every 10 s, so turn i is at i*10 s.
                    start_s: i as f64 * 10.0,
                    end_s: i as f64 * 10.0 + 9.0,
                    track: if speaker == "Ich" { TrackKind::Mic } else { TrackKind::System },
                    speaker: speaker.into(),
                    text: t["text"].as_str().unwrap().into(),
                }
            })
            .collect();
        let note = MeetingNote {
            meta: MeetingMeta {
                id: Some(id.into()),
                started_at: m["started_at"].as_str().unwrap().into(),
                duration_s: 60.0,
                source_app: m["app"].as_str().map(Into::into),
            },
            transcript: Transcript { segments },
            summary: None,
        };
        let path = root.join("notes").join(format!("{id}.md"));
        std::fs::write(&path, render_note(&note, &NoteOptions::default())).unwrap();
        write_segment_lines(&paths.meetings_dir.join(id), &note.transcript).unwrap();
        records.push(json!({"id": id, "started_at": m["started_at"], "duration_s": 60.0,
            "status": {"kind": "done"}, "source_app": m["app"], "note_path": path.to_string_lossy()}));
    }
    std::fs::write(paths.meetings_dir.join("index.json"), serde_json::to_string(&records).unwrap()).unwrap();
    let ix = Index::open(&paths.index).unwrap();
    let r = sync(&ix, &paths).unwrap();
    assert!(r.errors.is_empty(), "{:?}", r.errors);

    // The note joins consecutive turns of one speaker into one paragraph,
    // cited at its first turn: turn index -> the paragraph's start.
    let mut para_start: BTreeMap<(String, i64), i64> = BTreeMap::new();
    for m in &corpus {
        let id = m["id"].as_str().unwrap().to_string();
        let mut start = 0i64;
        let mut last: Option<&str> = None;
        for (i, t) in m["turns"].as_array().unwrap().iter().enumerate() {
            let sp = t["speaker"].as_str().unwrap();
            if last != Some(sp) {
                start = i as i64 * 10_000;
                last = Some(sp);
            }
            para_start.insert((id.clone(), i as i64), start);
        }
    }

    let modes = [Mode::Words, Mode::Trigram, Mode::Both];
    // mode -> category -> (questions, hit@1, hit@5, reciprocal ranks)
    let mut table: BTreeMap<&str, BTreeMap<String, (usize, usize, usize, f64)>> = BTreeMap::new();
    let mut per_question = Vec::new();
    for q in &questions {
        let cat = q["category"].as_str().unwrap().to_string();
        let expect: Vec<String> = q["expect"]
            .as_array()
            .unwrap()
            .iter()
            .map(|e| {
                let m = e["meeting"].as_str().unwrap();
                let at = para_start[&(m.to_string(), e["turn"].as_i64().unwrap())];
                sokki_index::cite("m", m, Some(at))
            })
            .collect();
        let mut row = json!({"category": cat, "query": q["query"]});
        for mode in modes {
            let out = query::search(
                &ix,
                &SearchOpts {
                    query: q["query"].as_str().unwrap().into(),
                    kind: None,
                    section: Some("transcript".into()),
                    mode,
                    list: ListOpts { n: 10, max_tokens: 100_000, ..Default::default() },
                },
            )
            .unwrap();
            let cites: Vec<&str> = out["results"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["cite"].as_str().unwrap())
                .collect();
            let rank = cites.iter().position(|c| expect.iter().any(|e| e == c));
            let e = table.entry(mode.as_str()).or_default().entry(cat.clone()).or_default();
            e.0 += 1;
            e.1 += (rank == Some(0)) as usize;
            e.2 += rank.is_some_and(|r| r < 5) as usize;
            e.3 += rank.map(|r| 1.0 / (r as f64 + 1.0)).unwrap_or(0.0);
            row[mode.as_str()] = json!(rank.map(|r| r + 1));
        }
        per_question.push(row);
    }

    let mut out = json!({"set": "memory-search-de", "questions": questions.len(), "modes": {}, "per_question": per_question});
    println!("\nrecall on memory-search-de ({} questions, {} meetings)", questions.len(), corpus.len());
    for mode in modes {
        let cats = &table[mode.as_str()];
        let total = cats.values().fold((0, 0, 0, 0.0), |a, c| (a.0 + c.0, a.1 + c.1, a.2 + c.2, a.3 + c.3));
        println!(
            "{:8} recall@1 {:.2}  recall@5 {:.2}  MRR {:.2}",
            mode.as_str(),
            total.1 as f64 / total.0 as f64,
            total.2 as f64 / total.0 as f64,
            total.3 / total.0 as f64
        );
        let mut m = json!({"recall_at_1": total.1 as f64 / total.0 as f64, "recall_at_5": total.2 as f64 / total.0 as f64, "mrr": total.3 / total.0 as f64, "by_category": {}});
        for (cat, c) in cats {
            println!("    {:15} {:>2} q  recall@5 {:.2}", cat, c.0, c.2 as f64 / c.0 as f64);
            m["by_category"][cat] = json!({"questions": c.0, "recall_at_5": c.2 as f64 / c.0 as f64, "recall_at_1": c.1 as f64 / c.0 as f64});
        }
        out["modes"][mode.as_str()] = m;
    }
    let runs = evals().join("runs/memory-search");
    std::fs::create_dir_all(&runs).unwrap();
    let file = runs.join(format!("{}.json", chrono_date()));
    std::fs::write(&file, serde_json::to_string_pretty(&out).unwrap()).unwrap();
    println!("written to {}", file.display());
    let _ = std::fs::remove_dir_all(&root);
}

fn chrono_date() -> String {
    let t = sokki_index::time::local_rfc3339(sokki_index::time::now_ms());
    t.get(..10).unwrap_or("run").to_string()
}
