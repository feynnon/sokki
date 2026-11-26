//! Runs the typo model from the command line, for tests and measurements.
//!
//!     sokki-typos <tables dir> <lexicon dir> fixes   < text     one line per fix: start, typed, fix
//!     sokki-typos <tables dir> <lexicon dir> words   < lines    "prev words<TAB>word" -> fix or empty
//!     sokki-typos <tables dir> <lexicon dir> report <journal dir> [--examples]
//!         What Sokki would have fixed in the "Keep what I write" journal, per app: counts only;
//!         --examples also prints each fix (the user's own text: never into a report or commit).
//!
//! SOKKI_TYPOS_KEYBOARD sets the letter keys (`Typos::set_keyboard`): "qwerty", "qwertz" (the
//! default, the phone's), or three rows separated by spaces.
//! SPDX-License-Identifier: GPL-3.0-only

use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::time::Instant;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: sokki-typos <tables dir> <lexicon dir> fixes|words");
        std::process::exit(2);
    }
    let t0 = Instant::now();
    let mut typos = match sokki_typos::Typos::load(Path::new(&args[1]), Path::new(&args[2])) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    };
    if let Ok(k) = std::env::var("SOKKI_TYPOS_KEYBOARD") {
        let rows: Vec<&str> = match k.as_str() {
            "qwerty" => vec!["qwertyuiop", "asdfghjkl", "zxcvbnm"],
            "qwertz" => vec!["qwertzuiopü", "asdfghjklöä", "yxcvbnm"],
            k => k.split(' ').collect(),
        };
        typos.set_keyboard(&rows);
    }
    eprintln!("loaded in {} ms", t0.elapsed().as_millis());
    let out = std::io::stdout();
    let mut out = out.lock();
    match args[3].as_str() {
        "fixes" => {
            let mut text = String::new();
            std::io::stdin().read_to_string(&mut text).unwrap();
            for f in typos.fixes(&text) {
                writeln!(out, "{}\t{}\t{}", f.start, f.typed, f.fix).unwrap();
            }
        }
        "words" => {
            let mut times = Vec::new();
            for line in std::io::stdin().lock().lines() {
                let line = line.unwrap();
                let (prev, word) = line.split_once('\t').unwrap_or(("", line.as_str()));
                let prevs: Vec<&str> = prev.split(' ').filter(|w| !w.is_empty()).collect();
                let t = Instant::now();
                let fix = typos.correction(word, &prevs, None);
                times.push(t.elapsed().as_secs_f64() * 1000.0);
                writeln!(out, "{}", fix.unwrap_or_default()).unwrap();
            }
            times.sort_by(|a, b| a.partial_cmp(b).unwrap());
            if !times.is_empty() {
                let at = |q: f64| times[((times.len() - 1) as f64 * q) as usize];
                eprintln!(
                    "{} words: median {:.3} ms, p95 {:.3} ms, max {:.3} ms",
                    times.len(),
                    at(0.5),
                    at(0.95),
                    at(1.0)
                );
            }
        }
        "report" => {
            let dir = args.get(4).map(Path::new).unwrap_or_else(|| {
                eprintln!("report needs the journal folder");
                std::process::exit(2)
            });
            report(typos, dir, args.iter().any(|a| a == "--examples"), &mut out);
        }
        m => {
            eprintln!("unknown mode {m}");
            std::process::exit(2);
        }
    }
}

#[derive(Default)]
struct App {
    entries: usize,
    words: usize,
    typos: usize,
    capitals: usize,
    replaceable: usize,
    not_replaceable: usize,
}

/// The words a piece of writing added: the text after what the field already held.
fn written(prefill: &str, text: &str) -> String {
    match text.strip_prefix(prefill) {
        Some(rest) if !prefill.is_empty() => rest.to_string(),
        _ => text.to_string(),
    }
}

fn report(mut typos: sokki_typos::Typos, dir: &Path, examples: bool, out: &mut impl Write) {
    let mut entries: Vec<serde_json::Value> = Vec::new();
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .map(|r| r.filter_map(|e| e.ok().map(|e| e.path())).collect())
        .unwrap_or_default();
    files.sort();
    for f in files.iter().filter(|p| {
        let n = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
        n.starts_with("journal-") && n.ends_with(".jsonl")
    }) {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        for line in text.lines() {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(line) {
                if v["t"] == "entry" {
                    entries.push(v);
                }
            }
        }
    }
    let s = |v: &serde_json::Value, k: &str| v[k].as_str().unwrap_or("").to_string();
    // the keyboard's history: words typed in two or more entries are the user's own
    let mut seen: std::collections::HashMap<String, usize> = Default::default();
    for e in &entries {
        let mut once = std::collections::HashSet::new();
        for w in written(&s(e, "prefill"), &s(e, "text")).split(|c: char| !c.is_alphabetic()) {
            if !w.is_empty() && once.insert(w.to_string()) {
                *seen.entry(w.to_string()).or_default() += 1;
            }
        }
    }
    typos
        .lexicon
        .set_personal(seen.into_iter().filter(|(_, n)| *n >= 2).map(|(w, _)| w));
    let mut apps: std::collections::BTreeMap<String, App> = Default::default();
    let t = Instant::now();
    for e in &entries {
        let text = written(&s(e, "prefill"), &s(e, "text"));
        let a = apps.entry(s(e, "app")).or_default();
        a.entries += 1;
        a.words += text
            .split(|c: char| !c.is_alphabetic())
            .filter(|w| !w.is_empty())
            .count();
        match e["replaceable"].as_bool() {
            Some(true) => a.replaceable += 1,
            Some(false) => a.not_replaceable += 1,
            None => {}
        }
        for f in typos.fixes(&text) {
            if sokki_typos::fold(&f.typed) == sokki_typos::fold(&f.fix) {
                a.capitals += 1;
            } else {
                a.typos += 1;
                if examples {
                    eprintln!("  {}: {} -> {}", s(e, "app"), f.typed, f.fix);
                }
            }
        }
    }
    let total = apps.values().fold(App::default(), |mut x, a| {
        x.entries += a.entries;
        x.words += a.words;
        x.typos += a.typos;
        x.capitals += a.capitals;
        x.replaceable += a.replaceable;
        x.not_replaceable += a.not_replaceable;
        x
    });
    writeln!(
        out,
        "{:40} {:>7} {:>7} {:>6} {:>9} {:>9} {:>12}",
        "app", "entries", "words", "typos", "per 1000", "capitals", "replaceable"
    )
    .unwrap();
    let line = |name: &str, a: &App, out: &mut dyn Write| {
        let per = if a.words > 0 {
            1000.0 * a.typos as f64 / a.words as f64
        } else {
            0.0
        };
        let rep = match a.replaceable + a.not_replaceable {
            0 => "unknown".to_string(),
            n => format!("{}/{}", a.replaceable, n),
        };
        writeln!(
            out,
            "{:40} {:>7} {:>7} {:>6} {:>9.1} {:>9} {:>12}",
            name, a.entries, a.words, a.typos, per, a.capitals, rep
        )
        .unwrap();
    };
    for (name, a) in &apps {
        line(name, a, out);
    }
    line("all apps", &total, out);
    eprintln!("checked in {} ms", t.elapsed().as_millis());
}
