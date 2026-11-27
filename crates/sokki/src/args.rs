//! Flags for the index commands, strict: an unknown flag fails loudly
//! with the list of known ones, instead of being ignored and returning
//! unfiltered results.

use anyhow::{bail, Context, Result};
use std::collections::HashMap;

pub struct Args {
    pub pos: Vec<String>,
    values: HashMap<String, String>,
    switches: Vec<String>,
}

impl Args {
    /// `values`: flags that take a value (`--since 14d`, `--since=14d`,
    /// `-n 5`); `switches`: flags without one (`--json`).
    pub fn parse(cmd: &str, words: &[&str], values: &[&str], switches: &[&str]) -> Result<Args> {
        let mut out = Args {
            pos: Vec::new(),
            values: HashMap::new(),
            switches: Vec::new(),
        };
        let mut i = 0;
        while i < words.len() {
            let w = words[i];
            if w == "--" {
                out.pos.extend(words[i + 1..].iter().map(|s| s.to_string()));
                break;
            }
            if w.starts_with('-') && w.len() > 1 && w.parse::<f64>().is_err() {
                let (name, inline) = match w.split_once('=') {
                    Some((n, v)) => (n, Some(v)),
                    None => (w, None),
                };
                if values.contains(&name) {
                    let v = match inline {
                        Some(v) => v.to_string(),
                        None => {
                            i += 1;
                            words
                                .get(i)
                                .with_context(|| format!("{name} needs a value (sokki {cmd} --help)"))?
                                .to_string()
                        }
                    };
                    if out.values.insert(name.to_string(), v).is_some() {
                        bail!("{name} given twice");
                    }
                } else if switches.contains(&name) && inline.is_none() {
                    out.switches.push(name.to_string());
                } else {
                    let mut known: Vec<&str> = values.iter().chain(switches).copied().collect();
                    known.sort();
                    bail!(
                        "unknown flag {name} for `sokki {cmd}`. Known: {}",
                        known.join(" ")
                    );
                }
            } else {
                out.pos.push(w.to_string());
            }
            i += 1;
        }
        Ok(out)
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    pub fn on(&self, name: &str) -> bool {
        self.switches.iter().any(|s| s == name)
    }

    pub fn number(&self, name: &str, default: usize) -> Result<usize> {
        match self.get(name) {
            Some(v) => v
                .parse()
                .with_context(|| format!("{name} needs a whole number, not {v:?}")),
            None => Ok(default),
        }
    }

    /// The flags given with values, for `filters_applied`.
    pub fn shown(&self, skip: &[&str]) -> Vec<(String, String)> {
        let mut v: Vec<(String, String)> = self
            .values
            .iter()
            .filter(|(k, _)| !skip.contains(&k.as_str()))
            .map(|(k, v)| (k.trim_start_matches('-').to_string(), v.clone()))
            .collect();
        v.sort();
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unknown_flags_fail_and_known_ones_parse() {
        let a = Args::parse(
            "search",
            &["Vertrag", "--since", "14d", "-n=3", "--json", "Druckerei"],
            &["--since", "-n"],
            &["--json"],
        )
        .unwrap();
        assert_eq!(a.pos, vec!["Vertrag", "Druckerei"]);
        assert_eq!(a.get("--since"), Some("14d"));
        assert_eq!(a.number("-n", 10).unwrap(), 3);
        assert!(a.on("--json"));
        let e = Args::parse("search", &["x", "--persn", "anna"], &["--person"], &["--json"])
            .err()
            .unwrap()
            .to_string();
        assert!(e.contains("unknown flag --persn") && e.contains("--person"), "{e}");
        assert!(Args::parse("search", &["--since"], &["--since"], &[]).is_err());
        assert!(Args::parse("search", &["--json=1"], &[], &["--json"]).is_err());
    }
}
