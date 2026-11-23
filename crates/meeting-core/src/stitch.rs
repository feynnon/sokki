//! Speakers found during the recording, window by window.
//!
//! The diarizer only takes whole recordings, so during a call it runs on
//! overlapping windows of each track (every few minutes, the last few
//! minutes). Its speaker labels are local to a window ("S1" in one window
//! may be "S2" in the next). [`Stitcher`] joins the windows into one set of
//! speakers for the whole track, the way online diarization usually works
//! (e.g. diart): a window's speaker is the earlier speaker it shares the
//! overlap with; one that doesn't speak in the overlap is matched by its
//! voice embedding; anyone else is new.
//!
//! The end of each window is left out (the diarizer sees less of what
//! follows there), and taken from the next window instead.

use std::collections::HashMap;

use crate::SpeakerTurn;

/// How windows are laid out and joined.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StitchConfig {
    /// New audio per window, seconds: a window runs every `step_s`.
    pub step_s: f64,
    /// Each window reaches back this far before the previous window's end.
    pub overlap_s: f64,
    /// The last part of each window (but the last window) is left to the
    /// next one, seconds. Must be smaller than `overlap_s`, so the windows
    /// share committed audio to match speakers in.
    pub guard_s: f64,
    /// Voices with at least this cosine similarity are one speaker, for a
    /// window speaker not heard in the overlap.
    pub same_voice: f32,
}

impl Default for StitchConfig {
    fn default() -> Self {
        Self {
            step_s: 300.0,
            overlap_s: 60.0,
            guard_s: 30.0,
            same_voice: 0.6,
        }
    }
}

impl StitchConfig {
    /// The window to diarize once the track has `samples_s` seconds, given
    /// where the previous window ended (`None`: first window). Returns
    /// (start, end) when a window is due.
    pub fn next_window(&self, last_end_s: Option<f64>, samples_s: f64) -> Option<(f64, f64)> {
        let start = last_end_s.map_or(0.0, |e| (e - self.overlap_s).max(0.0));
        let end = last_end_s.unwrap_or(0.0) + self.step_s;
        (samples_s >= end).then_some((start, end))
    }
}

/// One speaker of the whole track.
#[derive(Debug, Clone)]
struct Global {
    id: String,
    /// Sum of window embeddings, weighted by speech time.
    embedding: Option<Vec<f32>>,
    speech_s: f64,
}

/// Joins window diarizations of one track. Feed windows in order.
#[derive(Debug, Clone)]
pub struct Stitcher {
    cfg: StitchConfig,
    turns: Vec<SpeakerTurn>,
    speakers: Vec<Global>,
    /// Turns up to here are final.
    committed_s: f64,
    /// Where the last window ended.
    last_end_s: Option<f64>,
}

impl Stitcher {
    pub fn new(cfg: StitchConfig) -> Self {
        Self {
            cfg,
            turns: Vec::new(),
            speakers: Vec::new(),
            committed_s: 0.0,
            last_end_s: None,
        }
    }

    pub fn config(&self) -> &StitchConfig {
        &self.cfg
    }

    /// Turns up to here are final.
    pub fn committed_s(&self) -> f64 {
        self.committed_s
    }

    /// Where the last window ended (`None` before the first).
    pub fn last_end_s(&self) -> Option<f64> {
        self.last_end_s
    }

    /// Adds the diarization of the window `start_s..end_s` (turn times
    /// relative to the window). `last`: the recording ends with it, so
    /// nothing is left to the next window.
    pub fn add_window(&mut self, start_s: f64, end_s: f64, turns: Vec<SpeakerTurn>, last: bool) {
        let commit_end = if last {
            end_s
        } else {
            (end_s - self.cfg.guard_s).max(self.committed_s)
        };
        let local: Vec<SpeakerTurn> = turns
            .into_iter()
            .map(|t| SpeakerTurn {
                start_s: t.start_s + start_s,
                end_s: t.end_s + start_s,
                ..t
            })
            .collect();
        let map = self.match_speakers(&local, start_s);
        for t in &local {
            let a = t.start_s.max(self.committed_s);
            let b = t.end_s.min(commit_end);
            if b <= a {
                continue;
            }
            let id = map[&t.speaker].clone();
            self.turns.push(SpeakerTurn {
                start_s: a,
                end_s: b,
                speaker: id.clone(),
                embedding: None,
            });
            let g = self.speakers.iter_mut().find(|g| g.id == id).expect("mapped");
            g.speech_s += b - a;
        }
        // Each window speaker's embedding adds to its global speaker, by
        // how long it spoke in the window.
        let mut seen: HashMap<&str, (f64, Option<&Vec<f32>>)> = HashMap::new();
        for t in &local {
            let e = seen.entry(t.speaker.as_str()).or_insert((0.0, None));
            e.0 += t.end_s - t.start_s;
            if e.1.is_none() {
                e.1 = t.embedding.as_ref();
            }
        }
        for (local_id, (dur, emb)) in seen {
            let Some(emb) = emb else { continue };
            let g = self
                .speakers
                .iter_mut()
                .find(|g| g.id == map[local_id])
                .expect("mapped");
            let sum = g.embedding.get_or_insert_with(|| vec![0.0; emb.len()]);
            if sum.len() == emb.len() {
                for (s, x) in sum.iter_mut().zip(emb) {
                    *s += *x * dur as f32;
                }
            }
        }
        self.committed_s = commit_end;
        self.last_end_s = Some(end_s);
    }

    /// Global id for each window speaker.
    fn match_speakers(&mut self, local: &[SpeakerTurn], window_start_s: f64) -> HashMap<String, String> {
        let (lo, hi) = (window_start_s, self.committed_s);
        let mut ids: Vec<&str> = Vec::new();
        for t in local {
            if !ids.contains(&t.speaker.as_str()) {
                ids.push(&t.speaker);
            }
        }
        // Shared speech in the overlap, per (window speaker, global speaker).
        let mut shared: Vec<(f64, usize, usize)> = Vec::new();
        if hi > lo {
            for (li, lid) in ids.iter().enumerate() {
                for (gi, g) in self.speakers.iter().enumerate() {
                    let mut s = 0.0;
                    for a in local.iter().filter(|t| t.speaker == *lid) {
                        for b in self.turns.iter().filter(|t| t.speaker == g.id) {
                            let x = a.start_s.max(b.start_s).max(lo);
                            let y = a.end_s.min(b.end_s).min(hi);
                            s += (y - x).max(0.0);
                        }
                    }
                    if s > 0.0 {
                        shared.push((s, li, gi));
                    }
                }
            }
        }
        shared.sort_by(|a, b| b.0.total_cmp(&a.0));
        let mut map: HashMap<String, String> = HashMap::new();
        let mut used = vec![false; self.speakers.len()];
        for (_, li, gi) in shared {
            if map.contains_key(ids[li]) || used[gi] {
                continue;
            }
            used[gi] = true;
            map.insert(ids[li].to_string(), self.speakers[gi].id.clone());
        }
        // Not in the overlap: the nearest voice, if close enough.
        for lid in &ids {
            if map.contains_key(*lid) {
                continue;
            }
            let emb = local
                .iter()
                .find(|t| t.speaker == *lid)
                .and_then(|t| t.embedding.as_ref());
            let best = emb.and_then(|e| {
                self.speakers
                    .iter()
                    .enumerate()
                    .filter(|(gi, _)| !used[*gi])
                    .filter_map(|(gi, g)| Some((gi, cosine(e, g.embedding.as_ref()?))))
                    .max_by(|a, b| a.1.total_cmp(&b.1))
            });
            match best {
                Some((gi, sim)) if sim >= self.cfg.same_voice => {
                    used[gi] = true;
                    map.insert(lid.to_string(), self.speakers[gi].id.clone());
                }
                _ => {
                    let id = format!("S{}", self.speakers.len() + 1);
                    self.speakers.push(Global {
                        id: id.clone(),
                        embedding: None,
                        speech_s: 0.0,
                    });
                    used.push(true);
                    map.insert(lid.to_string(), id);
                }
            }
        }
        map
    }

    /// The track's turns so far, in time order, each with its speaker's
    /// embedding (normalised), as a diarizer returns them.
    pub fn turns(&self) -> Vec<SpeakerTurn> {
        let emb: HashMap<&str, Vec<f32>> = self
            .speakers
            .iter()
            .filter_map(|g| Some((g.id.as_str(), normalized(g.embedding.as_ref()?))))
            .collect();
        let mut out: Vec<SpeakerTurn> = self
            .turns
            .iter()
            .map(|t| SpeakerTurn {
                embedding: emb.get(t.speaker.as_str()).cloned(),
                ..t.clone()
            })
            .collect();
        out.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
        // A turn cut at a window seam continues in the next window.
        let mut joined: Vec<SpeakerTurn> = Vec::with_capacity(out.len());
        for t in out {
            if let Some(last) = joined.last_mut() {
                if last.speaker == t.speaker && t.start_s - last.end_s < 1e-6 {
                    last.end_s = last.end_s.max(t.end_s);
                    continue;
                }
            }
            joined.push(t);
        }
        joined
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na * nb)
    }
}

fn normalized(v: &[f32]) -> Vec<f32> {
    let n: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n == 0.0 {
        v.to_vec()
    } else {
        v.iter().map(|x| x / n).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(s: f64, e: f64, sp: &str, emb: Option<Vec<f32>>) -> SpeakerTurn {
        SpeakerTurn {
            start_s: s,
            end_s: e,
            speaker: sp.into(),
            embedding: emb,
        }
    }

    /// A call: A and B alternate every 20 s, C joins at 500 s.
    fn truth(s: f64, e: f64) -> Vec<(f64, f64, &'static str)> {
        let mut out = Vec::new();
        let mut x = 0.0;
        while x < e {
            let who = if x >= 500.0 && (x as i64 / 20) % 3 == 0 {
                "C"
            } else if (x as i64 / 20) % 2 == 0 {
                "A"
            } else {
                "B"
            };
            let (a, b) = (x.max(s), (x + 18.0).min(e));
            if b > a {
                out.push((a, b, who));
            }
            x += 20.0;
        }
        out
    }

    fn voice(who: &str) -> Vec<f32> {
        match who {
            "A" => vec![1.0, 0.0, 0.0],
            "B" => vec![0.0, 1.0, 0.0],
            _ => vec![0.0, 0.0, 1.0],
        }
    }

    /// What a diarizer says about one window: the true turns, relative to
    /// the window, with labels in order of first appearance in the window
    /// (so they differ from window to window).
    fn diarize(s: f64, e: f64) -> Vec<SpeakerTurn> {
        let mut order: Vec<&str> = Vec::new();
        truth(s, e)
            .into_iter()
            .map(|(a, b, who)| {
                if !order.contains(&who) {
                    order.push(who);
                }
                let local = format!("S{}", order.iter().position(|w| *w == who).unwrap() + 1);
                t(a - s, b - s, &local, Some(voice(who)))
            })
            .collect()
    }

    fn run(total_s: f64, cfg: StitchConfig) -> Vec<SpeakerTurn> {
        let mut st = Stitcher::new(cfg);
        let mut have = 0.0;
        while have < total_s {
            have = (have + 10.0).min(total_s);
            if let Some((s, e)) = cfg.next_window(st.last_end_s(), have) {
                st.add_window(s, e, diarize(s, e), false);
            }
        }
        let s = st.last_end_s().map_or(0.0, |e| (e - cfg.overlap_s).max(0.0));
        st.add_window(s, total_s, diarize(s, total_s), true);
        st.turns()
    }

    #[test]
    fn windows_join_into_the_speakers_of_the_whole_track() {
        let turns = run(1000.0, StitchConfig::default());
        // Same speaker for the same voice throughout, three in all.
        let mut by_voice: HashMap<&str, &str> = HashMap::new();
        let truth = truth(0.0, 1000.0);
        assert_eq!(turns.len(), truth.len());
        for (got, (a, b, who)) in turns.iter().zip(&truth) {
            assert!((got.start_s - a).abs() < 1e-6 && (got.end_s - b).abs() < 1e-6);
            let id = by_voice.entry(who).or_insert(got.speaker.as_str());
            assert_eq!(*id, got.speaker, "{who} at {a}");
            assert_eq!(got.embedding.as_deref(), Some(&voice(who)[..]));
        }
        assert_eq!(by_voice.len(), 3);
    }

    #[test]
    fn a_voice_back_after_a_silent_overlap_is_recognised() {
        // A speaks at the start, then only B for a while, then A again.
        let mut st = Stitcher::new(StitchConfig::default());
        st.add_window(0.0, 300.0, vec![t(0.0, 100.0, "S1", Some(voice("A"))), t(110.0, 300.0, "S2", Some(voice("B")))], false);
        // Second window: B first (in the overlap), then A again after it.
        st.add_window(240.0, 600.0, vec![t(0.0, 200.0, "S1", Some(voice("B"))), t(210.0, 360.0, "S2", Some(voice("A")))], false);
        let turns = st.turns();
        let ids: Vec<&str> = turns.iter().map(|t| t.speaker.as_str()).collect();
        let a1 = &turns[0].speaker;
        let a2 = &turns.iter().find(|t| t.start_s >= 450.0).unwrap().speaker;
        assert_eq!(a1, a2, "{ids:?}");
        // A new voice not heard before gets its own speaker.
        st.add_window(540.0, 900.0, vec![t(0.0, 60.0, "S1", Some(voice("A"))), t(100.0, 360.0, "S2", Some(voice("C")))], true);
        let mut speakers: Vec<String> = st.turns().iter().map(|t| t.speaker.clone()).collect();
        speakers.sort();
        speakers.dedup();
        assert_eq!(speakers.len(), 3);
    }

    #[test]
    fn windows_are_due_every_step_with_the_overlap() {
        let c = StitchConfig::default();
        assert_eq!(c.next_window(None, 299.0), None);
        assert_eq!(c.next_window(None, 300.0), Some((0.0, 300.0)));
        assert_eq!(c.next_window(Some(300.0), 599.0), None);
        assert_eq!(c.next_window(Some(300.0), 600.0), Some((240.0, 600.0)));
    }
}
