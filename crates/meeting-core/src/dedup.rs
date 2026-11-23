//! Cross-track echo removal.
//!
//! During a laptop call without headphones the microphone also hears the
//! remote side coming out of the speakers, so remote speech shows up twice:
//! cleanly on the system track and as a quieter copy on the mic track. The
//! system track is the authoritative copy; this module removes (or trims)
//! mic segments that repeat system speech at the same time.
//!
//! Idea credit: the mic-bleed pass `drop_mic_bleed` in Lark
//! (MIT-licensed fork of Handy, `src-tauri/src/managers/meeting.rs`), which
//! drops a mic entry whose text is >75 % Levenshtein-similar to a system entry
//! within 8 s. This version works on tokens instead of whole strings so it can
//! also handle partial echoes (a mic segment that is half echo, half genuine
//! speech) and mic segments that span several system segments.
//!
//! Algorithm, per mic segment:
//! 1. Collect the system segments that overlap it in time (± `window_s`).
//! 2. Tokenise both sides (lower case, umlauts folded, punctuation dropped)
//!    and give every token an estimated time by spreading the segment's
//!    duration evenly over its tokens.
//! 3. Align the mic tokens against the system tokens with a longest common
//!    subsequence, where two tokens match when they are the same word (small
//!    spelling differences allowed) *and* their estimated times are close.
//!    The time condition is what keeps genuine speech that merely uses the
//!    same words as the remote side.
//! 4. Matched mic tokens that form a dense run (≥ `min_run_tokens` matches,
//!    gaps of at most `max_gap_tokens` unmatched tokens, ≥ `min_run_density`)
//!    are echo. Short segments (fewer tokens than a run needs) are echo only
//!    when every token matched.
//! 5. Echo runs are cut out. What is left is kept as its own segment(s) if it
//!    has at least `min_keep_tokens` tokens; otherwise the segment is dropped.
//!
//! System segments are never changed.

use crate::lang::{tokenize, tokens_similar};
use crate::{Segment, TrackKind};

#[derive(Debug, Clone, PartialEq)]
pub struct DedupConfig {
    /// How far apart in time a mic and a system segment may be and still be
    /// compared, in seconds.
    pub window_s: f64,
    /// Base tolerance for matching two tokens' estimated times, in seconds.
    /// The effective tolerance also grows with segment length because token
    /// times are only estimated.
    pub token_time_tolerance_s: f64,
    /// Matched tokens needed in one run before it counts as echo.
    pub min_run_tokens: usize,
    /// Unmatched tokens allowed inside a run (ASR noise in the echo copy).
    pub max_gap_tokens: usize,
    /// Share of matched tokens a run needs.
    pub min_run_density: f64,
    /// A leftover piece of a trimmed segment needs at least this many tokens
    /// to be kept; smaller leftovers are ASR noise around the echo.
    pub min_keep_tokens: usize,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            window_s: 1.5,
            token_time_tolerance_s: 2.5,
            min_run_tokens: 4,
            max_gap_tokens: 2,
            min_run_density: 0.6,
            min_keep_tokens: 3,
        }
    }
}

/// What happened during dedup.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DedupReport {
    /// Mic segments removed completely.
    pub removed: usize,
    /// Mic segments of which only an echo part was cut out.
    pub trimmed: usize,
}

/// Removes mic echoes of system speech. `segments` may be in any order; the
/// result keeps the input order (trimmed pieces take the place of their
/// original segment).
pub fn remove_echoes(segments: Vec<Segment>, cfg: &DedupConfig) -> (Vec<Segment>, DedupReport) {
    let mut report = DedupReport::default();
    let system: Vec<&Segment> = segments
        .iter()
        .filter(|s| s.track == TrackKind::System)
        .collect();
    if system.is_empty() {
        return (segments, report);
    }
    let mut out = Vec::with_capacity(segments.len());
    for seg in &segments {
        if seg.track != TrackKind::Mic {
            out.push(seg.clone());
            continue;
        }
        match check_segment(seg, &system, cfg) {
            Verdict::Keep => out.push(seg.clone()),
            Verdict::Drop => report.removed += 1,
            Verdict::Trim(pieces) => {
                report.trimmed += 1;
                out.extend(pieces);
            }
        }
    }
    (out, report)
}

#[derive(Debug)]
enum Verdict {
    Keep,
    Drop,
    Trim(Vec<Segment>),
}

/// A token with its estimated time.
struct Tok {
    norm: String,
    word_idx: usize,
    t: f64,
}

fn timed_tokens(seg: &Segment) -> Vec<Tok> {
    let toks = tokenize(&seg.text);
    let n = toks.len().max(1) as f64;
    let dur = (seg.end_s - seg.start_s).max(0.0);
    toks.into_iter()
        .enumerate()
        .map(|(i, (norm, word_idx))| Tok {
            norm,
            word_idx,
            t: seg.start_s + dur * (i as f64 + 0.5) / n,
        })
        .collect()
}

fn check_segment(mic: &Segment, system: &[&Segment], cfg: &DedupConfig) -> Verdict {
    let mut cands: Vec<&Segment> = system
        .iter()
        .copied()
        .filter(|s| s.start_s < mic.end_s + cfg.window_s && s.end_s > mic.start_s - cfg.window_s)
        .collect();
    if cands.is_empty() {
        return Verdict::Keep;
    }
    cands.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));

    let mic_toks = timed_tokens(mic);
    if mic_toks.is_empty() {
        return Verdict::Keep;
    }
    let sys_toks: Vec<Tok> = cands.iter().flat_map(|s| timed_tokens(s)).collect();
    if sys_toks.is_empty() {
        return Verdict::Keep;
    }
    let longest = cands
        .iter()
        .map(|s| s.end_s - s.start_s)
        .fold(mic.end_s - mic.start_s, f64::max);
    let tol = cfg.token_time_tolerance_s + 0.2 * longest.max(0.0);

    let matched = align(&mic_toks, &sys_toks, tol);
    let n = mic_toks.len();
    let n_matched = matched.iter().filter(|m| **m).count();

    // Short segment: too few tokens to form a run. Echo only if all of it
    // matched; a single word additionally needs the system side to be a
    // similarly short utterance, so a genuine "ja" said while the remote
    // side happens to say "ja, ..." survives.
    if n < cfg.min_run_tokens {
        if n_matched < n {
            return Verdict::Keep;
        }
        if n == 1 {
            let short_sys = cands.iter().any(|s| {
                let k = tokenize(&s.text).len();
                k <= 3
                    && s.start_s < mic.end_s + cfg.window_s
                    && s.end_s > mic.start_s - cfg.window_s
            });
            if !short_sys {
                return Verdict::Keep;
            }
        }
        return Verdict::Drop;
    }

    let runs = echo_runs(&matched, cfg);
    if runs.is_empty() {
        return Verdict::Keep;
    }
    let mut is_echo = vec![false; n];
    for (a, b) in &runs {
        for e in is_echo.iter_mut().take(*b + 1).skip(*a) {
            *e = true;
        }
    }

    // Leftover groups of consecutive non-echo tokens.
    let mut groups: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < n {
        if is_echo[i] {
            i += 1;
            continue;
        }
        let start = i;
        while i < n && !is_echo[i] {
            i += 1;
        }
        groups.push((start, i - 1));
    }
    let kept: Vec<(usize, usize)> = groups
        .into_iter()
        .filter(|(a, b)| b - a + 1 >= cfg.min_keep_tokens)
        .collect();
    if kept.is_empty() {
        return Verdict::Drop;
    }

    let words: Vec<&str> = mic.text.split_whitespace().collect();
    let dur = (mic.end_s - mic.start_s).max(0.0);
    let pieces = kept
        .into_iter()
        .map(|(a, b)| {
            // Rebuild from the original words, keeping punctuation-only words
            // that sit between kept tokens.
            let first_word = mic_toks[a].word_idx;
            let last_word = mic_toks[b].word_idx;
            let text = words[first_word..=last_word].join(" ");
            Segment {
                start_s: mic.start_s + dur * a as f64 / n as f64,
                end_s: mic.start_s + dur * (b + 1) as f64 / n as f64,
                track: mic.track,
                speaker: mic.speaker.clone(),
                text: capitalize_first(&text),
            }
        })
        .collect();
    Verdict::Trim(pieces)
}

pub(crate) fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        None => String::new(),
    }
}

/// LCS alignment; returns, per mic token, whether it was matched to a system
/// token that is the same word and close in time.
fn align(mic: &[Tok], sys: &[Tok], tol: f64) -> Vec<bool> {
    let (n, m) = (mic.len(), sys.len());
    let eq = |i: usize, j: usize| -> bool {
        (mic[i].t - sys[j].t).abs() <= tol && tokens_similar(&mic[i].norm, &sys[j].norm)
    };
    // dp[i][j] = LCS of mic[i..] and sys[j..]
    let mut dp = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            dp[i][j] = if eq(i, j) {
                dp[i + 1][j + 1] + 1
            } else {
                dp[i + 1][j].max(dp[i][j + 1])
            };
        }
    }
    let mut matched = vec![false; n];
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if eq(i, j) && dp[i][j] == dp[i + 1][j + 1] + 1 {
            matched[i] = true;
            i += 1;
            j += 1;
        } else if dp[i + 1][j] >= dp[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    matched
}

/// Dense runs of matched tokens, as inclusive index ranges.
fn echo_runs(matched: &[bool], cfg: &DedupConfig) -> Vec<(usize, usize)> {
    let mut runs = Vec::new();
    let n = matched.len();
    let mut i = 0;
    while i < n {
        if !matched[i] {
            i += 1;
            continue;
        }
        let start = i;
        let mut end = i;
        let mut hits = 1;
        let mut j = i + 1;
        let mut gap = 0;
        while j < n {
            if matched[j] {
                hits += 1;
                end = j;
                gap = 0;
            } else {
                gap += 1;
                if gap > cfg.max_gap_tokens {
                    break;
                }
            }
            j += 1;
        }
        let len = end - start + 1;
        if hits >= cfg.min_run_tokens && hits as f64 / len as f64 >= cfg.min_run_density {
            runs.push((start, end));
        }
        i = end + 1;
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(track: TrackKind, start: f64, end: f64, speaker: &str, text: &str) -> Segment {
        Segment {
            start_s: start,
            end_s: end,
            track,
            speaker: speaker.into(),
            text: text.into(),
        }
    }
    fn mic(start: f64, end: f64, text: &str) -> Segment {
        seg(TrackKind::Mic, start, end, "Ich", text)
    }
    fn sys(start: f64, end: f64, text: &str) -> Segment {
        seg(TrackKind::System, start, end, "Speaker 2", text)
    }
    fn run(segs: Vec<Segment>) -> (Vec<Segment>, DedupReport) {
        remove_echoes(segs, &DedupConfig::default())
    }

    #[test]
    fn exact_echo_is_removed() {
        let (out, rep) = run(vec![
            sys(
                3.5,
                8.9,
                "Ja, wir hören dich gut. Lass uns direkt mit dem Status Update starten.",
            ),
            mic(
                3.6,
                9.0,
                "Ja, wir hören dich gut. Lass uns direkt mit dem Status Update starten.",
            ),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].track, TrackKind::System);
        assert_eq!(rep.removed, 1);
    }

    #[test]
    fn echo_with_asr_noise_is_removed() {
        let (out, _) = run(vec![
            sys(
                9.6,
                15.3,
                "Von meiner Seite gibt es gute Nachrichten. Das Release ist gestern live gegangen.",
            ),
            mic(
                9.7,
                15.4,
                "von meiner seite gibts gute nachricht das relis ist gestern life gegangen",
            ),
        ]);
        assert_eq!(out.len(), 1, "{out:?}");
        assert_eq!(out[0].track, TrackKind::System);
    }

    #[test]
    fn echo_spanning_two_system_segments_is_removed() {
        let (out, _) = run(vec![
            sys(3.5, 8.9, "Ja, wir hören dich gut. Lass uns direkt mit dem Status Update starten."),
            sys(9.6, 15.3, "Von meiner Seite gibt es gute Nachrichten. Das Release ist gestern live gegangen."),
            mic(3.6, 15.4, "Ja wir hören dich gut lass uns direkt mit dem Status Update starten von meiner Seite gibt es gute Nachrichten das Release ist gestern live gegangen"),
        ]);
        assert!(out.iter().all(|s| s.track == TrackKind::System), "{out:?}");
    }

    #[test]
    fn genuine_overlapping_speech_is_kept() {
        let (out, rep) = run(vec![
            sys(
                10.0,
                16.0,
                "Wir sollten die Kampagne erst im Dezember starten, wegen der Feiertage.",
            ),
            mic(12.0, 15.0, "Moment, ich muss kurz was nachschauen."),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(rep, DedupReport::default());
    }

    #[test]
    fn same_words_at_a_different_time_are_kept() {
        // The user repeats the remote sentence much later: not an echo.
        let (out, _) = run(vec![
            sys(0.0, 5.0, "Lass uns das Budget bis Freitag bestätigen."),
            mic(40.0, 45.0, "Lass uns das Budget bis Freitag bestätigen."),
        ]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn quoting_the_remote_side_in_a_long_turn_is_kept() {
        // Overlaps in time (window) but the shared words are far apart in the
        // estimated token timing, and the shared run is short.
        let (out, _) = run(vec![
            sys(0.0, 3.0, "Das Budget ist knapp."),
            mic(3.2, 20.0, "Ja, ich weiß, dass das Budget knapp ist, aber wir haben im letzten Quartal trotzdem mehr Leads bekommen als geplant, also würde ich gern weitermachen."),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[1].text.len(), "Ja, ich weiß, dass das Budget knapp ist, aber wir haben im letzten Quartal trotzdem mehr Leads bekommen als geplant, also würde ich gern weitermachen.".len());
    }

    #[test]
    fn partial_echo_is_trimmed_and_genuine_part_kept() {
        // The mic segment starts with genuine speech, then the diarizer ran it
        // into the echo of the remote answer.
        let (out, rep) = run(vec![
            mic(16.0, 28.0, "Sehr schön. Gab es Probleme beim Deployment? Nur kleinere Dinge, die haben wir aber schon behoben."),
            sys(19.6, 23.0, "Nur kleinere Dinge, die haben wir aber schon behoben."),
        ]);
        assert_eq!(rep.trimmed, 1, "{out:?}");
        let mic_out: Vec<_> = out.iter().filter(|s| s.track == TrackKind::Mic).collect();
        assert_eq!(mic_out.len(), 1);
        assert_eq!(
            mic_out[0].text,
            "Sehr schön. Gab es Probleme beim Deployment?"
        );
        assert!(mic_out[0].end_s < 23.0);
        assert_eq!(mic_out[0].start_s, 16.0);
    }

    #[test]
    fn partial_echo_in_the_middle_splits_the_segment() {
        let (out, _) = run(vec![
            mic(0.0, 18.0, "Okay das klingt gut für mich. Die Zahlen sind besser als letzte Woche gewesen. Dann machen wir das genau so weiter."),
            sys(5.5, 12.5, "Die Zahlen sind besser als letzte Woche gewesen."),
        ]);
        let mic_out: Vec<_> = out.iter().filter(|s| s.track == TrackKind::Mic).collect();
        assert_eq!(mic_out.len(), 2, "{out:?}");
        assert_eq!(mic_out[0].text, "Okay das klingt gut für mich.");
        assert_eq!(mic_out[1].text, "Dann machen wir das genau so weiter.");
        assert!(mic_out[0].end_s <= mic_out[1].start_s);
    }

    #[test]
    fn echo_with_a_stray_word_at_the_edge_is_removed() {
        let (out, _) = run(vec![
            sys(
                20.0,
                25.0,
                "Die liegen bei achtzehn Euro, also etwas über unserem Ziel.",
            ),
            mic(
                19.8,
                25.3,
                "äh die liegen bei achtzehn euro also etwas über unserem ziel hm",
            ),
        ]);
        assert!(out.iter().all(|s| s.track == TrackKind::System), "{out:?}");
    }

    #[test]
    fn short_echo_of_short_utterance_is_removed() {
        let (out, _) = run(vec![
            sys(5.0, 5.6, "Ja, genau."),
            mic(5.05, 5.7, "Ja genau"),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].track, TrackKind::System);
    }

    #[test]
    fn genuine_backchannel_during_remote_speech_is_kept() {
        // Remote says a long sentence containing "ja"; the user says "Ja."
        let (out, _) = run(vec![
            sys(
                0.0,
                8.0,
                "Wir haben ja schon letzte Woche über den Launch der neuen Landingpage gesprochen.",
            ),
            mic(4.0, 4.4, "Ja."),
        ]);
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn mic_only_meeting_is_untouched() {
        let segs = vec![
            mic(0.0, 2.0, "Hallo zusammen"),
            mic(2.0, 4.0, "Hallo zusammen"),
        ];
        let (out, rep) = run(segs.clone());
        assert_eq!(out, segs);
        assert_eq!(rep, DedupReport::default());
    }

    #[test]
    fn system_segments_are_never_dropped() {
        let segs = vec![
            sys(0.0, 2.0, "Das gleiche nochmal"),
            sys(0.5, 2.5, "Das gleiche nochmal"),
        ];
        let (out, _) = run(segs.clone());
        assert_eq!(out, segs);
    }

    #[test]
    fn english_echo_with_misheard_words_is_removed() {
        // Same example as Lark's test: "website"→"Win Side", "ship"→"shift".
        let (out, _) = run(vec![
            mic(
                28.0,
                32.0,
                "The website copy needs a final review before we ship",
            ),
            sys(
                28.2,
                32.1,
                "The Win Side copy needs a final review before we shift.",
            ),
        ]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].track, TrackKind::System);
    }
}
