//! Tracks → transcript: diarize each track, transcribe each speaker turn,
//! merge both tracks by time, remove mic echoes, and give speakers names.
//!
//! Naming happens *after* echo removal, so an echo cluster on the mic track
//! (the remote voices picked up from the laptop speakers) neither uses up a
//! "Speaker N" number nor gets mistaken for the user.

use std::collections::HashMap;

use anyhow::{bail, Result};

use crate::dedup::{self, DedupConfig, DedupReport};
use crate::{
    Diarizer, Segment, SpeakerTurn, Track, TrackKind, Transcriber, Transcript, SAMPLE_RATE,
};

#[derive(Debug, Clone, PartialEq)]
pub struct PipelineConfig {
    /// Label for the main microphone speaker (the person recording).
    pub user_name: String,
    /// Audio added before and after each turn when slicing it for the
    /// transcriber, so word edges are not clipped. Seconds.
    pub pad_s: f64,
    /// Turns longer than this are split into chunks for the transcriber.
    pub max_chunk_s: f64,
    /// A chunk cut is never placed earlier than this after the previous cut
    /// (and, where possible, the last chunk is at least this long).
    pub min_chunk_s: f64,
    /// Consecutive turns of the same speaker with a gap up to this are joined
    /// before transcription (gives the transcriber more context), as long as
    /// the result stays within `max_chunk_s`.
    pub merge_gap_s: f64,
    /// Turns shorter than this are skipped (diarizer slivers).
    pub min_turn_s: f64,
    /// Cross-track echo removal; `None` turns it off.
    pub dedup: Option<DedupConfig>,
    /// A speaker cluster with less than this share of its track's speech and
    /// less than `ghost_max_s` seconds is a diarization artefact ("Speaker 5"
    /// with two words); its lines go to the nearest real speaker.
    pub ghost_max_share: f64,
    pub ghost_max_s: f64,
    /// Acoustic echo check of mic turns against the system track, before
    /// transcription; `None` turns it off.
    pub echo: Option<crate::echo::EchoConfig>,
    /// Transcribe consecutive turns of a track together, up to this many
    /// seconds, and give each word to the turn it falls in. Short turns on
    /// their own give the model too little context: on muffled call audio it
    /// then guesses the language and writes garbage ("anoň", "Stavbe se").
    /// Needs a transcriber with word times; `None` transcribes turn by turn.
    pub window_s: Option<f64>,
    /// A window never bridges a pause between turns longer than this.
    pub window_max_gap_s: f64,
    /// Take a turn's words from pieces even when a piece holds more than
    /// one speaker (see [`Hooks::pieces`]). Off: such pieces are
    /// transcribed again turn by turn.
    pub pieces_any_speaker: bool,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            user_name: "Ich".into(),
            pad_s: 0.15,
            max_chunk_s: 25.0,
            min_chunk_s: 8.0,
            merge_gap_s: 0.5,
            min_turn_s: 0.25,
            dedup: Some(DedupConfig::default()),
            echo: Some(crate::echo::EchoConfig::default()),
            ghost_max_share: 0.04,
            ghost_max_s: 8.0,
            // Off: on mixed German/English calls a window holding both
            // languages comes out in one of them (WER 3.8% -> 31.2% on
            // `de_en_mixed`), though phone audio improves (17.6% -> 15.3%).
            window_s: None,
            window_max_gap_s: 2.0,
            pieces_any_speaker: false,
        }
    }
}

/// Gives real names to known voices. Called once per speaker cluster that
/// has speech in the final transcript. Return `None` to fall back to the
/// generic label ("Ich" for the main mic speaker, "Speaker N" otherwise).
///
/// Implemented for closures `Fn(TrackKind, &str, Option<&[f32]>) -> Option<String>`.
pub trait SpeakerNamer: Send + Sync {
    /// `cluster` is the diarizer's id, local to the track (e.g. "S1");
    /// `embedding` is the cluster's voice embedding if the diarizer gave one.
    fn name(&self, track: TrackKind, cluster: &str, embedding: Option<&[f32]>) -> Option<String>;
}

impl<F> SpeakerNamer for F
where
    F: Fn(TrackKind, &str, Option<&[f32]>) -> Option<String> + Send + Sync,
{
    fn name(&self, track: TrackKind, cluster: &str, embedding: Option<&[f32]>) -> Option<String> {
        self(track, cluster, embedding)
    }
}

/// Optional callbacks for [`run_with`].
#[derive(Default, Clone, Copy)]
pub struct Hooks<'a> {
    pub namer: Option<&'a dyn SpeakerNamer>,
    /// Called after each transcriber call with (done, total).
    pub progress: Option<&'a (dyn Fn(usize, usize) + Sync)>,
    /// Pieces transcribed during the recording ([`crate::live`]). A turn
    /// that pieces of one speaker cover takes its words from them instead
    /// of being transcribed again.
    pub pieces: Option<&'a [crate::live::Piece]>,
    /// Speakers found during the recording ([`crate::stitch`]), per track
    /// kind: a track listed here is not diarized again.
    pub turns: Option<&'a [(TrackKind, Vec<SpeakerTurn>)]>,
}

/// One speaker cluster in the result, so the app can e.g. offer "remember
/// this voice" with the embedding.
#[derive(Debug, Clone, PartialEq)]
pub struct SpeakerInfo {
    pub track: TrackKind,
    /// Index into the `tracks` slice passed to [`run`].
    pub track_index: usize,
    /// Diarizer cluster id (or "all" when diarization failed).
    pub cluster: String,
    /// Display name used in the transcript.
    pub name: String,
    /// Speech time in the final transcript, seconds.
    pub speech_s: f64,
    /// The main microphone speaker.
    pub is_user: bool,
    /// The name came from the [`SpeakerNamer`].
    pub named_by_hook: bool,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PipelineResult {
    pub transcript: Transcript,
    /// Transcription jobs, and how many of them took their words from
    /// [`Hooks::pieces`].
    pub jobs: usize,
    pub jobs_from_pieces: usize,
    /// Speakers in order of first appearance.
    pub speakers: Vec<SpeakerInfo>,
    /// Problems that did not stop the run, one short sentence each for the
    /// user (no error text): see [`SPEAKERS_NOT_SEPARATED`] and
    /// [`PARTS_NOT_TRANSCRIBED`].
    pub warnings: Vec<String>,
    /// The same problems with their error text, for the log.
    pub details: Vec<String>,
    pub dedup: DedupReport,
    /// Mic turns dropped before transcription as acoustic echo of the system track.
    pub echo_turns_dropped: usize,
    /// Seconds spent finding speakers (all tracks, with the acoustic echo
    /// check of mic turns), for the log.
    pub speakers_s: f64,
    /// Seconds spent transcribing, for the log.
    pub transcribe_s: f64,
}

/// Shown when the diarizer failed on a track (or could not be loaded).
pub const SPEAKERS_NOT_SEPARATED: &str =
    "Speakers couldn't be told apart, so each audio track is one speaker.";
/// Shown when some transcriber calls failed.
pub const PARTS_NOT_TRANSCRIBED: &str = "Parts of the audio couldn't be transcribed.";

/// Adds `sentence` to `warnings` unless it is there already.
pub fn warn_once(warnings: &mut Vec<String>, sentence: &str) {
    if !warnings.iter().any(|w| w == sentence) {
        warnings.push(sentence.to_string());
    }
}

/// Runs the pipeline without a naming hook or progress callback.
pub fn run(
    tracks: &[Track],
    diarizer: &dyn Diarizer,
    transcriber: &dyn Transcriber,
    cfg: &PipelineConfig,
) -> Result<PipelineResult> {
    run_with(tracks, diarizer, transcriber, cfg, Hooks::default())
}

/// Text for each job, in order (empty when nothing was recognised or the
/// transcriber failed; failures are counted and the first few reported).
///
/// With `cfg.window_s` and a transcriber that gives word times, consecutive
/// jobs of a track are transcribed together and each word goes to the job
/// whose time span holds the word's midpoint (or the nearest job).
fn transcribe_jobs(
    tracks: &[Track],
    jobs: &[Job],
    transcriber: &dyn Transcriber,
    cfg: &PipelineConfig,
    hooks: Hooks<'_>,
    from_pieces: &[Option<String>],
    failures: &mut usize,
    details: &mut Vec<String>,
) -> Vec<String> {
    let total = jobs.len();
    let mut texts = vec![String::new(); total];
    let mut done = 0usize;
    let mut fail = |details: &mut Vec<String>, kind: TrackKind, at: f64, e: &anyhow::Error| {
        *failures += 1;
        if *failures <= 3 {
            details.push(format!(
                "Transcription failed for {} audio at {}: {e:#}",
                track_label(kind),
                crate::format_mmss(at)
            ));
        }
    };
    let mut per_job = |i: usize, texts: &mut Vec<String>, details: &mut Vec<String>| {
        let job = &jobs[i];
        let track = &tracks[job.track_index];
        let pad_a = if job.pad_before { cfg.pad_s } else { 0.0 };
        let pad_b = if job.pad_after { cfg.pad_s } else { 0.0 };
        let slice = slice_samples(&track.samples, job.start_s - pad_a, job.end_s + pad_b);
        match transcriber.transcribe(slice) {
            Ok(t) => texts[i] = t,
            Err(e) => fail(details, track.kind, job.start_s, &e),
        }
    };
    let progress = |done: usize| {
        if let Some(p) = hooks.progress {
            p(done, total);
        }
    };

    let mut words_supported = cfg.window_s.is_some();
    let mut i = 0;
    while i < total {
        if let Some(words) = &from_pieces[i] {
            if !words.is_empty() {
                // A turn often starts mid-sentence of its piece; as its own
                // line it starts with a capital, as the transcriber would
                // have written it (seen: 70 of 216 lines in lower case on a
                // real call, against 33 of 229 transcribed turn by turn).
                texts[i] = crate::dedup::capitalize_first(&transcriber.polish(words));
            }
            i += 1;
            done += 1;
            progress(done);
            continue;
        }
        if !words_supported {
            per_job(i, &mut texts, details);
            i += 1;
            done += 1;
            progress(done);
            continue;
        }
        // Window: jobs i..j of one track.
        let window_s = cfg.window_s.unwrap_or(f64::MAX);
        let mut j = i + 1;
        while j < total
            && from_pieces[j].is_none()
            && jobs[j].track_index == jobs[i].track_index
            && !jobs[j].break_before
            && jobs[j].start_s - jobs[j - 1].end_s <= cfg.window_max_gap_s
            && jobs[j].end_s - jobs[i].start_s <= window_s
        {
            j += 1;
        }
        let track = &tracks[jobs[i].track_index];
        let from = (jobs[i].start_s - cfg.pad_s).max(0.0);
        let to = jobs[j - 1].end_s + cfg.pad_s;
        let slice = slice_samples(&track.samples, from, to);
        match transcriber.transcribe_words(slice) {
            Ok(Some(words)) => {
                let mut parts: Vec<Vec<String>> = vec![Vec::new(); j - i];
                for w in words {
                    let text = w.text.trim();
                    if text.is_empty() {
                        continue;
                    }
                    let mid = from + (w.start_s + w.end_s) / 2.0;
                    let k = nearest_job(&jobs[i..j], mid);
                    parts[k].push(text.to_string());
                }
                for (k, p) in parts.into_iter().enumerate() {
                    if !p.is_empty() {
                        texts[i + k] = transcriber.polish(&p.join(" "));
                    }
                }
            }
            Ok(None) => {
                // No word times from this transcriber: turn by turn from here on.
                words_supported = false;
                continue;
            }
            Err(e) => {
                details.push(format!(
                    "Transcribing {} audio at {} in one piece failed ({e:#}); trying turn by turn.",
                    track_label(track.kind),
                    crate::format_mmss(jobs[i].start_s)
                ));
                for k in i..j {
                    per_job(k, &mut texts, details);
                }
            }
        }
        done += j - i;
        progress(done);
        i = j;
    }
    texts
}

/// The raw words of each job that pieces transcribed during the recording
/// cover, or `None` for a job to transcribe now.
///
/// A job takes its words from the pieces only when they cover it fully and
/// every one of them holds a single speaker: each job that overlaps such a
/// piece is of the job's cluster. A piece across a change of speaker is
/// transcribed again turn by turn, as without pieces, so two voices (or two
/// languages) never share one transcriber run that the pipeline would not
/// have made itself. Each word goes to the job whose padded span it overlaps
/// most, as a job's own audio slice would hold every word it overlaps;
/// words outside every padded job are dropped, as that audio would not have
/// been transcribed.
fn texts_from_pieces(
    tracks: &[Track],
    jobs: &[Job],
    pieces: &[crate::live::Piece],
    cfg: &PipelineConfig,
) -> Vec<Option<String>> {
    let mut out: Vec<Option<String>> = vec![None; jobs.len()];
    let overlaps = |a0: f64, a1: f64, b0: f64, b1: f64| a0 < b1 && b0 < a1;
    for (ti, track) in tracks.iter().enumerate() {
        let mut ps: Vec<&crate::live::Piece> =
            pieces.iter().filter(|p| p.track == track.kind).collect();
        ps.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
        let js: Vec<usize> = (0..jobs.len())
            .filter(|&j| jobs[j].track_index == ti)
            .collect();
        // Jobs overlapping each piece, and whether the piece is one speaker.
        let on_piece: Vec<Vec<usize>> = ps
            .iter()
            .map(|p| {
                js.iter()
                    .copied()
                    .filter(|&j| overlaps(jobs[j].start_s, jobs[j].end_s, p.start_s, p.end_s))
                    .collect()
            })
            .collect();
        let one_speaker: Vec<bool> = on_piece
            .iter()
            .map(|on| {
                on.first().is_none_or(|&f| {
                    cfg.pieces_any_speaker || on.iter().all(|&j| jobs[j].cluster == jobs[f].cluster)
                })
            })
            .collect();
        // Which jobs the pieces cover, without a gap and with one speaker each.
        let mut usable = vec![false; jobs.len()];
        for &j in &js {
            let (s, e) = (jobs[j].start_s, jobs[j].end_s);
            let mut reach = s;
            let mut ok = true;
            for (k, p) in ps.iter().enumerate() {
                if !overlaps(s, e, p.start_s, p.end_s) {
                    continue;
                }
                // Pieces are contiguous; tolerate float rounding at the seams.
                if p.start_s > reach + 1e-3 || !one_speaker[k] {
                    ok = false;
                    break;
                }
                reach = reach.max(p.end_s);
            }
            usable[j] = ok && reach + 1e-3 >= e;
        }
        let mut words: Vec<Vec<&str>> = vec![Vec::new(); jobs.len()];
        for p in &ps {
            for w in &p.words {
                let text = w.text.trim();
                if text.is_empty() {
                    continue;
                }
                // The job whose padded span the word overlaps most: what
                // its own audio slice would have held, at least in part.
                let shared = |j: usize| {
                    (w.end_s.min(jobs[j].end_s + cfg.pad_s)
                        - w.start_s.max(jobs[j].start_s - cfg.pad_s))
                    .max(0.0)
                };
                let best = js
                    .iter()
                    .copied()
                    .filter(|&j| shared(j) > 0.0 || touches(w, &jobs[j], cfg.pad_s))
                    .max_by(|&a, &b| shared(a).total_cmp(&shared(b)));
                if let Some(j) = best.filter(|&j| usable[j]) {
                    words[j].push(text);
                }
            }
        }
        for &j in &js {
            if usable[j] {
                out[j] = Some(words[j].join(" "));
            }
        }
    }
    out
}

/// A zero-length word (a time stamp only) inside a job's padded span.
fn touches(w: &crate::TimedWord, job: &Job, pad_s: f64) -> bool {
    w.start_s >= job.start_s - pad_s && w.start_s <= job.end_s + pad_s
}

/// Index of the job whose span holds `t`, else the one closest to it.
fn nearest_job(jobs: &[Job], t: f64) -> usize {
    let dist = |j: &Job| {
        if t < j.start_s {
            j.start_s - t
        } else if t > j.end_s {
            t - j.end_s
        } else {
            0.0
        }
    };
    let mut best = 0;
    for (k, j) in jobs.iter().enumerate() {
        if dist(j) < dist(&jobs[best]) {
            best = k;
        }
    }
    best
}

/// A transcription job: one chunk of one (possibly merged) turn.
struct Job {
    track_index: usize,
    cluster: String,
    start_s: f64,
    end_s: f64,
    /// Audio was left out right before this job (an echo turn), so a
    /// transcription window must not reach back over it.
    break_before: bool,
    pad_before: bool,
    pad_after: bool,
}

pub fn run_with(
    tracks: &[Track],
    diarizer: &dyn Diarizer,
    transcriber: &dyn Transcriber,
    cfg: &PipelineConfig,
    hooks: Hooks<'_>,
) -> Result<PipelineResult> {
    let mut warnings = Vec::new();
    let mut details = Vec::new();
    // (track index, cluster) -> embedding
    let mut embeddings: HashMap<(usize, String), Vec<f32>> = HashMap::new();
    let mut jobs: Vec<Job> = Vec::new();
    let mut echo_dropped = 0usize;
    let started = std::time::Instant::now();

    for (ti, track) in tracks.iter().enumerate() {
        let dur = track.duration_s();
        if dur <= 0.0 {
            continue;
        }
        let found = hooks
            .turns
            .and_then(|all| all.iter().find(|(k, _)| *k == track.kind))
            .map(|(_, t)| Ok(t.clone()));
        let turns = match found.unwrap_or_else(|| diarizer.diarize(&track.samples)) {
            Ok(t) => sanitize_turns(t, dur),
            Err(e) => {
                details.push(format!(
                    "Diarization failed for the {} track ({e:#}); treating the whole track as one speaker.",
                    track_label(track.kind)
                ));
                warn_once(&mut warnings, SPEAKERS_NOT_SEPARATED);
                vec![SpeakerTurn {
                    start_s: 0.0,
                    end_s: dur,
                    speaker: "all".into(),
                    embedding: None,
                }]
            }
        };
        for t in &turns {
            if let Some(e) = &t.embedding {
                embeddings
                    .entry((ti, t.speaker.clone()))
                    .or_insert_with(|| e.clone());
            }
        }
        let system = tracks.iter().find(|t| t.kind == TrackKind::System);
        let mut skipped = true; // a new track starts a new window
        for turn in merge_turns(turns, cfg) {
            if turn.end_s - turn.start_s < cfg.min_turn_s {
                continue;
            }
            if let (TrackKind::Mic, Some(system), Some(ecfg)) =
                (track.kind, system, cfg.echo.as_ref())
            {
                let share = crate::echo::echo_share(
                    &track.samples,
                    &system.samples,
                    turn.start_s,
                    turn.end_s,
                    ecfg,
                );
                if share >= ecfg.drop_share {
                    echo_dropped += 1;
                    skipped = true;
                    continue;
                }
            }
            let chunks = split_turn(&track.samples, turn.start_s, turn.end_s, cfg);
            let last = chunks.len() - 1;
            for (ci, (s, e)) in chunks.into_iter().enumerate() {
                jobs.push(Job {
                    track_index: ti,
                    cluster: turn.speaker.clone(),
                    start_s: s,
                    end_s: e,
                    pad_before: ci == 0,
                    pad_after: ci == last,
                    break_before: std::mem::take(&mut skipped),
                });
            }
        }
    }

    let speakers_s = started.elapsed().as_secs_f64();

    // Transcribe.
    let started = std::time::Instant::now();
    let total = jobs.len();
    let mut failures = 0usize;
    let mut raw: Vec<(Segment, usize, String)> = Vec::new();
    // Jobs the pieces from the recording cover need no transcriber call.
    let from_pieces = match hooks.pieces {
        Some(pieces) => texts_from_pieces(tracks, &jobs, pieces, cfg),
        None => vec![None; total],
    };
    let jobs_from_pieces = from_pieces.iter().filter(|t| t.is_some()).count();
    let texts = transcribe_jobs(
        tracks,
        &jobs,
        transcriber,
        cfg,
        hooks,
        &from_pieces,
        &mut failures,
        &mut details,
    );
    let transcribe_s = started.elapsed().as_secs_f64();
    for (job, text) in jobs.iter().zip(texts) {
        let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
        if text.is_empty() {
            continue;
        }
        raw.push((
            Segment {
                start_s: job.start_s,
                end_s: job.end_s,
                track: tracks[job.track_index].kind,
                speaker: cluster_key(job.track_index, &job.cluster),
                text,
            },
            job.track_index,
            job.cluster.clone(),
        ));
    }
    if failures > 3 {
        details.push(format!("{failures} transcriber calls failed in total."));
    }
    if total > 0 && failures == total {
        bail!("all {total} transcriber calls failed: {details:?}");
    }
    if failures > 0 {
        warn_once(&mut warnings, PARTS_NOT_TRANSCRIBED);
    }

    // Merge by time. Mic first on ties so the user's line leads.
    raw.sort_by(|a, b| {
        a.0.start_s
            .total_cmp(&b.0.start_s)
            .then((a.0.track != TrackKind::Mic).cmp(&(b.0.track != TrackKind::Mic)))
            .then(a.0.end_s.total_cmp(&b.0.end_s))
    });
    let mut key_to_cluster: HashMap<String, (usize, String)> = HashMap::new();
    for (seg, ti, cl) in &raw {
        key_to_cluster.insert(seg.speaker.clone(), (*ti, cl.clone()));
    }
    let segments: Vec<Segment> = raw.into_iter().map(|(s, _, _)| s).collect();

    let (segments, dedup_report) = match &cfg.dedup {
        Some(dc) => dedup::remove_echoes(segments, dc),
        None => (segments, DedupReport::default()),
    };
    let segments = absorb_ghost_speakers(segments, cfg.ghost_max_share, cfg.ghost_max_s);

    // Name speakers.
    let mut order: Vec<String> = Vec::new();
    let mut speech: HashMap<String, f64> = HashMap::new();
    for s in &segments {
        if !order.contains(&s.speaker) {
            order.push(s.speaker.clone());
        }
        *speech.entry(s.speaker.clone()).or_default() += (s.end_s - s.start_s).max(0.0);
    }
    let user_key = pick_user(&order, &speech, &key_to_cluster, tracks);

    let mut names: HashMap<String, String> = HashMap::new();
    let mut speakers = Vec::new();
    // "Speaker 1" is free when nobody is the user (single microphone).
    let mut next_generic = if user_key.is_some() { 2 } else { 1 };
    for key in &order {
        let (ti, cluster) = key_to_cluster[key].clone();
        let kind = tracks[ti].kind;
        let emb = embeddings.get(&(ti, cluster.clone())).cloned();
        let hook_name = hooks
            .namer
            .and_then(|n| n.name(kind, &cluster, emb.as_deref()))
            .map(|n| n.trim().to_string())
            .filter(|n| !n.is_empty() && !is_generic_label(n));
        let is_user = user_key.as_deref() == Some(key.as_str());
        let named_by_hook = hook_name.is_some();
        let name = match hook_name {
            Some(n) => n,
            None if is_user => cfg.user_name.clone(),
            None => {
                let n = format!("Speaker {next_generic}");
                next_generic += 1;
                n
            }
        };
        names.insert(key.clone(), name.clone());
        speakers.push(SpeakerInfo {
            track: kind,
            track_index: ti,
            cluster,
            name,
            speech_s: speech[key],
            is_user,
            named_by_hook,
            embedding: emb,
        });
    }
    let mut segments = segments;
    for s in &mut segments {
        s.speaker = names[&s.speaker].clone();
    }
    segments.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));

    Ok(PipelineResult {
        transcript: Transcript { segments },
        jobs: total,
        jobs_from_pieces,
        speakers,
        warnings,
        details,
        dedup: dedup_report,
        echo_turns_dropped: echo_dropped,
        speakers_s,
        transcribe_s,
    })
}

fn track_label(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Mic => "microphone",
        TrackKind::System => "system audio",
    }
}

/// Internal unique speaker key during the run; replaced by display names.
fn cluster_key(track_index: usize, cluster: &str) -> String {
    format!("\u{1}{track_index}\u{1}{cluster}")
}

fn is_generic_label(name: &str) -> bool {
    name.strip_prefix("Speaker ")
        .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
}

/// Clamps turns to the track, drops empty ones, sorts by start.
fn sanitize_turns(mut turns: Vec<SpeakerTurn>, dur: f64) -> Vec<SpeakerTurn> {
    for t in &mut turns {
        t.start_s = t.start_s.clamp(0.0, dur);
        t.end_s = t.end_s.clamp(0.0, dur);
    }
    turns.retain(|t| t.end_s > t.start_s && t.start_s.is_finite() && t.end_s.is_finite());
    turns.sort_by(|a, b| a.start_s.total_cmp(&b.start_s));
    turns
}

/// Joins consecutive turns of the same speaker separated by a short gap.
fn merge_turns(turns: Vec<SpeakerTurn>, cfg: &PipelineConfig) -> Vec<SpeakerTurn> {
    let mut out: Vec<SpeakerTurn> = Vec::with_capacity(turns.len());
    for t in turns {
        if let Some(last) = out.last_mut() {
            if last.speaker == t.speaker
                && t.start_s - last.end_s <= cfg.merge_gap_s
                && t.end_s - last.start_s <= cfg.max_chunk_s
            {
                last.end_s = last.end_s.max(t.end_s);
                continue;
            }
        }
        out.push(t);
    }
    out
}

fn slice_samples(samples: &[f32], start_s: f64, end_s: f64) -> &[f32] {
    let sr = SAMPLE_RATE as f64;
    let a = ((start_s.max(0.0) * sr) as usize).min(samples.len());
    let b = ((end_s.max(0.0) * sr).ceil() as usize).min(samples.len());
    &samples[a..b.max(a)]
}

/// Splits a turn into chunks of at most `max_chunk_s`, cutting at the
/// quietest 100 ms window in the allowed range. Returns (start, end) pairs.
/// Who the person recording is. Only decided for an online meeting — a
/// system track with speech — where the microphone is the user's side: the
/// mic voice with the most speech is the user. With everything on one
/// microphone (phone on speaker, people in a room) nobody is guessed: every
/// voice gets a neutral label unless a remembered voice names it.
fn pick_user(
    order: &[String],
    speech: &HashMap<String, f64>,
    key_to_cluster: &HashMap<String, (usize, String)>,
    tracks: &[Track],
) -> Option<String> {
    let kind = |k: &String| tracks[key_to_cluster[k].0].kind;
    let remote_speech = order.iter().any(|k| kind(k) == TrackKind::System);
    if !remote_speech {
        return None;
    }
    order
        .iter()
        .filter(|k| kind(k) == TrackKind::Mic)
        .max_by(|a, b| speech[*a].total_cmp(&speech[*b]))
        .cloned()
}

/// Gives the lines of tiny speaker clusters to the speaker of the nearest
/// line on the same track (see `PipelineConfig::ghost_max_share`).
/// A cluster with at most this many words (and a tiny share) is a ghost.
const GHOST_MAX_WORDS: usize = 20;

fn absorb_ghost_speakers(mut segments: Vec<Segment>, max_share: f64, max_s: f64) -> Vec<Segment> {
    let mut speech: HashMap<(TrackKind, String), f64> = HashMap::new();
    let mut words: HashMap<(TrackKind, String), usize> = HashMap::new();
    let mut per_track: HashMap<TrackKind, f64> = HashMap::new();
    for s in &segments {
        let d = (s.end_s - s.start_s).max(0.0);
        *speech.entry((s.track, s.speaker.clone())).or_default() += d;
        *words.entry((s.track, s.speaker.clone())).or_default() +=
            s.text.split_whitespace().count();
        *per_track.entry(s.track).or_default() += d;
    }
    // A ghost has a tiny share of its track and either little time or few
    // words: seen on a real call, "Speaker 4" with seven back-channel lines
    // ("Yeah", "Yep", "Design"), 12 words but more than `max_s` seconds.
    let ghost = |track: TrackKind, sp: &str| -> bool {
        let key = (track, sp.to_string());
        let own = speech[&key];
        let total = per_track[&track];
        let others = speech.keys().any(|(t, k)| *t == track && k != sp);
        others && own < max_share * total && (own < max_s || words[&key] <= GHOST_MAX_WORDS)
    };
    let snapshot: Vec<(TrackKind, String, f64, f64)> = segments
        .iter()
        .map(|s| (s.track, s.speaker.clone(), s.start_s, s.end_s))
        .collect();
    for (i, seg) in segments.iter_mut().enumerate() {
        if !ghost(seg.track, &seg.speaker) {
            continue;
        }
        let (a, b) = (snapshot[i].2, snapshot[i].3);
        // Nearest by the gap between lines; on a tie the earlier speaker, who
        // was talking just before.
        let nearest = snapshot
            .iter()
            .filter(|(t, sp, _, _)| *t == seg.track && !ghost(*t, sp))
            .map(|(_, sp, s0, s1)| ((s0 - b).max(a - s1).max(0.0), *s0, sp))
            .min_by(|x, y| x.0.total_cmp(&y.0).then(x.1.total_cmp(&y.1)));
        if let Some((_, _, sp)) = nearest {
            seg.speaker = sp.clone();
        }
    }
    segments
}

pub fn split_turn(
    samples: &[f32],
    start_s: f64,
    end_s: f64,
    cfg: &PipelineConfig,
) -> Vec<(f64, f64)> {
    let max = cfg.max_chunk_s.max(1.0);
    let min = cfg.min_chunk_s.clamp(0.5, max * 0.9);
    let mut out = Vec::new();
    let mut cur = start_s;
    while end_s - cur > max {
        let lo = cur + min;
        let mut hi = (cur + max).min(end_s - min);
        if hi < lo {
            hi = cur + max;
        }
        let cut = quietest_point(samples, lo, hi).unwrap_or(cur + max);
        out.push((cur, cut));
        cur = cut;
    }
    out.push((cur, end_s));
    out
}

/// Centre of the quietest 100 ms window (10 ms hop) in [lo, hi].
pub(crate) fn quietest_point(samples: &[f32], lo: f64, hi: f64) -> Option<f64> {
    const WIN: usize = (SAMPLE_RATE / 10) as usize; // 100 ms
    const HOP: usize = (SAMPLE_RATE / 100) as usize; // 10 ms
    let sr = SAMPLE_RATE as f64;
    let a = (lo * sr) as usize;
    let b = ((hi * sr) as usize).min(samples.len());
    if b <= a + WIN {
        return None;
    }
    let mut best: Option<(f64, usize)> = None;
    let mut pos = a;
    while pos + WIN <= b {
        let e: f64 = samples[pos..pos + WIN]
            .iter()
            .map(|x| (*x as f64) * (*x as f64))
            .sum();
        // Mild preference for later cuts (up to 1.5x energy penalty at the
        // earliest position) so chunks stay close to `max_chunk_s` when the
        // audio has no clear pause; a real pause is orders of magnitude
        // quieter than speech and still wins.
        let earliness = (b - WIN - pos) as f64 / (b - WIN - a).max(1) as f64;
        let score = e * (1.0 + 0.5 * earliness);
        if best.is_none_or(|(bs, _)| score < bs) {
            best = Some((score, pos));
        }
        pos += HOP;
    }
    best.map(|(_, p)| (p + WIN / 2) as f64 / sr)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::TimedWord;
    use anyhow::anyhow;
    use std::sync::Mutex;

    const SR: f64 = SAMPLE_RATE as f64;

    /// Audio where every sample's value encodes its own time, so the fake
    /// transcriber can tell which part of the track it received.
    fn timed_track(kind: TrackKind, dur_s: f64) -> Track {
        let n = (dur_s * SR) as usize;
        Track {
            kind,
            samples: (0..n)
                .map(|i| 0.1 + (i as f64 / SR) as f32 * 1e-4)
                .collect(),
        }
    }
    fn time_of(sample: f32) -> f64 {
        ((sample - 0.1) / 1e-4) as f64
    }

    struct FakeDiarizer {
        /// Per call (track order), turns or an error.
        results: Mutex<Vec<Result<Vec<SpeakerTurn>, String>>>,
    }
    impl FakeDiarizer {
        fn new(results: Vec<Result<Vec<SpeakerTurn>, String>>) -> Self {
            Self {
                results: Mutex::new(results),
            }
        }
    }
    impl Diarizer for FakeDiarizer {
        fn diarize(&self, _samples: &[f32]) -> Result<Vec<SpeakerTurn>> {
            let mut r = self.results.lock().unwrap();
            r.remove(0).map_err(|e| anyhow!(e))
        }
    }

    /// Returns the text of the script entry whose time range contains the
    /// middle of the audio it is given; records the (start, end) it saw.
    struct FakeTranscriber {
        script: Vec<(f64, f64, &'static str)>,
        calls: Mutex<Vec<(f64, f64)>>,
        fail_at: Option<f64>,
    }
    impl FakeTranscriber {
        fn new(script: Vec<(f64, f64, &'static str)>) -> Self {
            Self {
                script,
                calls: Mutex::new(vec![]),
                fail_at: None,
            }
        }
    }
    impl Transcriber for FakeTranscriber {
        fn transcribe(&self, samples: &[f32]) -> Result<String> {
            let (a, b) = (time_of(samples[0]), time_of(*samples.last().unwrap()));
            self.calls.lock().unwrap().push((a, b));
            let mid = (a + b) / 2.0;
            if let Some(f) = self.fail_at {
                if (a..=b).contains(&f) {
                    return Err(anyhow!("engine exploded"));
                }
            }
            Ok(self
                .script
                .iter()
                .find(|(s, e, _)| (*s..*e).contains(&mid))
                .map(|(_, _, t)| t.to_string())
                .unwrap_or_default())
        }
    }

    /// Gives word times: `words` are (time on the track, word). Returns the
    /// words inside the audio it receives, timed relative to its start.
    struct WordTranscriber {
        words: Vec<(f64, &'static str)>,
        calls: Mutex<Vec<(f64, f64)>>,
        fail_at: Option<f64>,
    }
    impl WordTranscriber {
        fn new(words: Vec<(f64, &'static str)>) -> Self {
            Self {
                words,
                calls: Mutex::new(vec![]),
                fail_at: None,
            }
        }
        fn in_range(&self, a: f64, b: f64) -> Vec<(f64, &'static str)> {
            self.words
                .iter()
                .copied()
                .filter(|(t, _)| *t >= a && *t <= b)
                .collect()
        }
    }
    impl Transcriber for WordTranscriber {
        fn transcribe(&self, samples: &[f32]) -> Result<String> {
            let (a, b) = (time_of(samples[0]), time_of(*samples.last().unwrap()));
            self.calls.lock().unwrap().push((a, b));
            Ok(self
                .in_range(a, b)
                .iter()
                .map(|(_, w)| *w)
                .collect::<Vec<_>>()
                .join(" "))
        }
        fn transcribe_words(&self, samples: &[f32]) -> Result<Option<Vec<TimedWord>>> {
            let (a, b) = (time_of(samples[0]), time_of(*samples.last().unwrap()));
            if self.fail_at.is_some_and(|f| (a..=b).contains(&f)) {
                return Err(anyhow!("engine exploded"));
            }
            self.calls.lock().unwrap().push((a, b));
            Ok(Some(
                self.in_range(a, b)
                    .into_iter()
                    .map(|(t, w)| TimedWord {
                        start_s: t - a - 0.1,
                        end_s: t - a + 0.1,
                        text: format!(" {w}"),
                    })
                    .collect(),
            ))
        }
        fn polish(&self, text: &str) -> String {
            text.replace("ähm ", "")
        }
    }

    fn turn(s: f64, e: f64, sp: &str) -> SpeakerTurn {
        SpeakerTurn {
            start_s: s,
            end_s: e,
            speaker: sp.into(),
            embedding: None,
        }
    }

    fn no_dedup() -> PipelineConfig {
        PipelineConfig {
            dedup: None,
            ..Default::default()
        }
    }

    #[test]
    fn ghost_speakers_are_absorbed() {
        let seg = |sp: &str, a: f64, b: f64| Segment {
            start_s: a,
            end_s: b,
            track: TrackKind::System,
            speaker: sp.into(),
            text: "x".into(),
        };
        let segs = vec![
            seg("A", 0.0, 30.0),
            seg("G", 30.5, 31.0), // ghost: half a second
            seg("B", 31.5, 60.0),
            seg("A", 60.0, 90.0),
        ];
        let out = absorb_ghost_speakers(segs, 0.04, 8.0);
        assert_eq!(out[1].speaker, "A");
        assert_eq!(out.iter().filter(|s| s.speaker == "G").count(), 0);
        // A lone speaker on a track is never a ghost.
        let solo = absorb_ghost_speakers(vec![seg("Z", 0.0, 1.0)], 0.04, 8.0);
        assert_eq!(solo[0].speaker, "Z");
    }

    #[test]
    fn mic_main_speaker_is_user_and_others_numbered_globally() {
        let tracks = vec![
            timed_track(TrackKind::Mic, 60.0),
            timed_track(TrackKind::System, 60.0),
        ];
        let d = FakeDiarizer::new(vec![
            // mic: S1 talks 10 s, S2 (person in the room) 3 s, S2 first.
            Ok(vec![turn(1.0, 4.0, "S2"), turn(5.0, 15.0, "S1")]),
            // system: S1 first at 0.5 (before everything), S2 later.
            Ok(vec![
                turn(0.5, 0.9, "S1"),
                turn(20.0, 25.0, "S2"),
                turn(30.0, 35.0, "S1"),
            ]),
        ]);
        let t = FakeTranscriber::new(vec![
            (0.0, 1.0, "Hallo?"),
            (1.0, 4.5, "Ich bin auch im Raum."),
            (5.0, 15.5, "Lass uns anfangen."),
            (19.0, 26.0, "Gute Idee."),
            (29.0, 36.0, "Einverstanden."),
        ]);
        let r = run(&tracks, &d, &t, &no_dedup()).unwrap();
        let lines: Vec<(String, String)> = r
            .transcript
            .segments
            .iter()
            .map(|s| (s.speaker.clone(), s.text.clone()))
            .collect();
        assert_eq!(
            lines,
            vec![
                ("Speaker 2".into(), "Hallo?".into()), // system S1, first
                ("Speaker 3".into(), "Ich bin auch im Raum.".into()), // mic S2
                ("Ich".into(), "Lass uns anfangen.".into()), // mic S1 (most speech)
                ("Speaker 4".into(), "Gute Idee.".into()), // system S2
                ("Speaker 2".into(), "Einverstanden.".into()),
            ]
        );
        assert!(r.warnings.is_empty());
        let user = r.speakers.iter().find(|s| s.is_user).unwrap();
        assert_eq!((user.track, user.cluster.as_str()), (TrackKind::Mic, "S1"));
    }

    #[test]
    fn same_cluster_id_on_two_tracks_are_different_speakers() {
        let tracks = vec![
            timed_track(TrackKind::Mic, 20.0),
            timed_track(TrackKind::System, 20.0),
        ];
        let d = FakeDiarizer::new(vec![
            Ok(vec![turn(0.0, 5.0, "S1")]),
            Ok(vec![turn(6.0, 9.0, "S1")]),
        ]);
        let t = FakeTranscriber::new(vec![(0.0, 5.5, "eins"), (5.5, 10.0, "zwei")]);
        let r = run(&tracks, &d, &t, &no_dedup()).unwrap();
        assert_eq!(r.transcript.speakers(), vec!["Ich", "Speaker 2"]);
    }

    #[test]
    fn naming_hook_wins_and_generic_labels_stay_unique() {
        let tracks = vec![
            timed_track(TrackKind::Mic, 30.0),
            timed_track(TrackKind::System, 30.0),
        ];
        let d = FakeDiarizer::new(vec![
            Ok(vec![SpeakerTurn {
                embedding: Some(vec![1.0, 0.0]),
                ..turn(0.0, 10.0, "S1")
            }]),
            Ok(vec![
                SpeakerTurn {
                    embedding: Some(vec![0.0, 1.0]),
                    ..turn(11.0, 14.0, "S1")
                },
                turn(15.0, 18.0, "S2"),
                turn(19.0, 22.0, "S3"),
            ]),
        ]);
        let t = FakeTranscriber::new(vec![
            (0.0, 10.5, "Ich starte."),
            (10.5, 14.5, "Hier Anna."),
            (14.5, 18.5, "Hier jemand."),
            (18.5, 23.0, "Und noch wer."),
        ]);
        let namer = |track: TrackKind, cluster: &str, emb: Option<&[f32]>| -> Option<String> {
            match (track, cluster, emb) {
                (TrackKind::Mic, _, Some(e)) if e[0] > 0.5 => Some("Robin".into()),
                (TrackKind::System, "S1", Some(e)) if e[1] > 0.5 => Some("Anna".into()),
                // A hook that returns a generic-looking label is ignored.
                (TrackKind::System, "S2", _) => Some("Speaker 3".into()),
                _ => None,
            }
        };
        let r = run_with(
            &tracks,
            &d,
            &t,
            &no_dedup(),
            Hooks {
                namer: Some(&namer),
                progress: None,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(
            r.transcript.speakers(),
            vec!["Robin", "Anna", "Speaker 2", "Speaker 3"]
        );
        let robin = &r.speakers[0];
        assert!(robin.is_user && robin.named_by_hook);
        assert_eq!(robin.embedding.as_deref(), Some(&[1.0f32, 0.0][..]));
    }

    #[test]
    fn long_turns_are_chunked_at_quiet_points() {
        // 60 s turn, loud everywhere except quiet dips at 20 s and 41 s.
        let mut track = timed_track(TrackKind::Mic, 62.0);
        let loud: Vec<f32> = track.samples.iter().map(|x| x * 3.0).collect();
        track.samples = loud;
        for dip in [20.0, 41.0] {
            let a = ((dip - 0.1) * SR) as usize;
            let b = ((dip + 0.1) * SR) as usize;
            for x in &mut track.samples[a..b] {
                *x = 0.0;
            }
        }
        let cfg = no_dedup();
        let chunks = split_turn(&track.samples, 1.0, 61.0, &cfg);
        assert_eq!(chunks.len(), 3, "{chunks:?}");
        assert!((chunks[0].1 - 20.0).abs() < 0.1, "{chunks:?}");
        assert!((chunks[1].1 - 41.0).abs() < 0.1, "{chunks:?}");
        assert_eq!(chunks[2].1, 61.0);
        for (a, b) in &chunks {
            assert!(b - a <= cfg.max_chunk_s + 1e-9);
        }
    }

    #[test]
    fn chunks_become_segments_and_padding_only_at_turn_edges() {
        let track = timed_track(TrackKind::Mic, 70.0);
        let d = FakeDiarizer::new(vec![Ok(vec![turn(2.0, 62.0, "S1")])]);
        let t = FakeTranscriber::new(vec![(0.0, 70.0, "bla")]);
        let r = run(std::slice::from_ref(&track), &d, &t, &no_dedup()).unwrap();
        let calls = t.calls.lock().unwrap().clone();
        assert_eq!(calls.len(), 3);
        // First call starts 0.15 s early; the last ends 0.15 s late.
        assert!((calls[0].0 - 1.85).abs() < 0.01, "{calls:?}");
        assert!((calls[2].1 - 62.15).abs() < 0.01, "{calls:?}");
        // Inner cut: no padding, so consecutive calls do not overlap.
        assert!(calls[1].0 >= calls[0].1 - 0.001, "{calls:?}");
        assert_eq!(r.transcript.segments.len(), 3);
        assert!(calls.iter().all(|(a, b)| b - a <= 25.0 + 0.31));
    }

    #[test]
    fn padding_is_clamped_to_track_bounds() {
        let track = timed_track(TrackKind::Mic, 5.0);
        let d = FakeDiarizer::new(vec![Ok(vec![turn(0.0, 5.0, "S1")])]);
        let t = FakeTranscriber::new(vec![(0.0, 5.0, "x")]);
        run(std::slice::from_ref(&track), &d, &t, &no_dedup()).unwrap();
        let calls = t.calls.lock().unwrap().clone();
        assert!(
            calls[0].0.abs() < 0.001 && (calls[0].1 - 5.0).abs() < 0.01,
            "{calls:?}"
        );
    }

    #[test]
    fn empty_text_and_tiny_turns_are_skipped() {
        let track = timed_track(TrackKind::Mic, 20.0);
        let d = FakeDiarizer::new(vec![Ok(vec![
            turn(0.0, 0.1, "S1"),
            turn(1.0, 3.0, "S1"),
            turn(5.0, 8.0, "S1"),
        ])]);
        let t = FakeTranscriber::new(vec![(4.0, 9.0, "  Nur das hier.  ")]);
        let r = run(std::slice::from_ref(&track), &d, &t, &no_dedup()).unwrap();
        assert_eq!(t.calls.lock().unwrap().len(), 2);
        assert_eq!(r.transcript.segments.len(), 1);
        assert_eq!(r.transcript.segments[0].text, "Nur das hier.");
    }

    #[test]
    fn same_speaker_turns_with_small_gaps_are_merged() {
        let track = timed_track(TrackKind::Mic, 20.0);
        let d = FakeDiarizer::new(vec![Ok(vec![
            turn(1.0, 3.0, "S1"),
            turn(3.3, 5.0, "S1"),
            turn(5.1, 6.0, "S2"),
            turn(9.0, 10.0, "S1"),
        ])]);
        let t = FakeTranscriber::new(vec![(0.0, 20.0, "x")]);
        run(std::slice::from_ref(&track), &d, &t, &no_dedup()).unwrap();
        assert_eq!(t.calls.lock().unwrap().len(), 3);
    }

    #[test]
    fn diarizer_failure_falls_back_to_one_speaker_with_warning() {
        let tracks = vec![
            timed_track(TrackKind::Mic, 40.0),
            timed_track(TrackKind::System, 40.0),
        ];
        let d = FakeDiarizer::new(vec![
            Ok(vec![turn(1.0, 4.0, "S1")]),
            Err("model not loaded".into()),
        ]);
        let t = FakeTranscriber::new(vec![(0.0, 4.5, "Hallo."), (4.5, 40.0, "Remote spricht.")]);
        let r = run(&tracks, &d, &t, &no_dedup()).unwrap();
        assert_eq!(r.warnings, vec![SPEAKERS_NOT_SEPARATED]);
        assert_eq!(r.details.len(), 1);
        assert!(r.details[0].contains("system audio") && r.details[0].contains("model not loaded"));
        // Whole 40 s system track -> 2 chunks, both "Speaker 2".
        let sys: Vec<_> = r
            .transcript
            .segments
            .iter()
            .filter(|s| s.track == TrackKind::System)
            .collect();
        assert_eq!(sys.len(), 2);
        assert!(sys.iter().all(|s| s.speaker == "Speaker 2"));
        assert_eq!(
            r.speakers
                .iter()
                .find(|s| s.track == TrackKind::System)
                .unwrap()
                .cluster,
            "all"
        );
    }

    #[test]
    fn segments_are_merged_in_time_order() {
        let tracks = vec![
            timed_track(TrackKind::Mic, 30.0),
            timed_track(TrackKind::System, 30.0),
        ];
        let d = FakeDiarizer::new(vec![
            Ok(vec![turn(0.0, 2.0, "S1"), turn(10.0, 12.0, "S1")]),
            Ok(vec![turn(4.0, 6.0, "S1"), turn(20.0, 22.0, "S1")]),
        ]);
        let t = FakeTranscriber::new(vec![
            (0.0, 3.0, "a"),
            (3.0, 7.0, "b"),
            (9.0, 13.0, "c"),
            (19.0, 23.0, "d"),
        ]);
        let r = run(&tracks, &d, &t, &no_dedup()).unwrap();
        let texts: Vec<&str> = r
            .transcript
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(texts, vec!["a", "b", "c", "d"]);
    }

    #[test]
    fn echo_cluster_on_mic_does_not_become_user_or_use_a_number() {
        // Laptop call without headphones: the mic diarizer finds the user (S1,
        // little speech) and an echo cluster (S2, lots of remote speech).
        let tracks = vec![
            timed_track(TrackKind::Mic, 40.0),
            timed_track(TrackKind::System, 40.0),
        ];
        let d = FakeDiarizer::new(vec![
            Ok(vec![
                turn(0.5, 2.9, "S1"),
                turn(3.5, 15.3, "S2"),
                turn(16.0, 18.9, "S1"),
            ]),
            Ok(vec![turn(3.5, 8.9, "S1"), turn(9.6, 15.3, "S2")]),
        ]);
        struct ByTrack;
        impl Transcriber for ByTrack {
            fn transcribe(&self, s: &[f32]) -> Result<String> {
                let mid = (time_of(s[0]) + time_of(*s.last().unwrap())) / 2.0;
                // Slice midpoints: mic S1 1.7 and 17.45, mic S2 (echo) 9.4,
                // system S1 6.2, system S2 12.45.
                Ok(match mid {
                    m if m < 3.0 => "Guten Morgen, könnt ihr mich gut hören?",
                    m if m < 7.0 => "Ja, wir hören dich gut. Lass uns direkt mit dem Status Update starten.",
                    m if m < 10.0 => "Ja, wir hören dich gut. Lass uns direkt mit dem Status Update starten. Von meiner Seite gibt es gute Nachrichten. Das Release ist gestern live gegangen.",
                    m if m < 15.5 => "Von meiner Seite gibt es gute Nachrichten. Das Release ist gestern live gegangen.",
                    _ => "Sehr schön. Gab es Probleme beim Deployment?",
                }
                .to_string())
            }
        }
        let r = run(&tracks, &d, &ByTrack, &PipelineConfig::default()).unwrap();
        assert_eq!(r.dedup.removed, 1, "{:#?}", r.transcript);
        assert_eq!(
            r.transcript.speakers(),
            vec!["Ich", "Speaker 2", "Speaker 3"]
        );
        assert_eq!(r.transcript.segments.len(), 4);
    }

    #[test]
    fn transcriber_failure_is_a_warning_unless_everything_fails() {
        let track = timed_track(TrackKind::Mic, 20.0);
        let d = FakeDiarizer::new(vec![Ok(vec![turn(1.0, 3.0, "S1"), turn(5.0, 7.0, "S1")])]);
        let mut t = FakeTranscriber::new(vec![(0.0, 20.0, "ok")]);
        t.fail_at = Some(6.0);
        let r = run(std::slice::from_ref(&track), &d, &t, &no_dedup()).unwrap();
        assert_eq!(r.transcript.segments.len(), 1);
        assert_eq!(r.warnings, vec![PARTS_NOT_TRANSCRIBED]);
        assert_eq!(r.details.len(), 1);

        let d = FakeDiarizer::new(vec![Ok(vec![turn(5.0, 7.0, "S1")])]);
        assert!(run(std::slice::from_ref(&track), &d, &t, &no_dedup()).is_err());
    }

    #[test]
    fn silent_and_empty_tracks_give_empty_transcript() {
        let tracks = vec![
            Track {
                kind: TrackKind::Mic,
                samples: vec![],
            },
            timed_track(TrackKind::System, 10.0),
        ];
        let d = FakeDiarizer::new(vec![Ok(vec![])]);
        let t = FakeTranscriber::new(vec![]);
        let r = run(&tracks, &d, &t, &PipelineConfig::default()).unwrap();
        assert!(r.transcript.segments.is_empty());
        assert!(r.speakers.is_empty());
    }

    #[test]
    fn progress_is_reported() {
        let track = timed_track(TrackKind::Mic, 20.0);
        let d = FakeDiarizer::new(vec![Ok(vec![turn(1.0, 3.0, "S1"), turn(5.0, 7.0, "S2")])]);
        let t = FakeTranscriber::new(vec![(0.0, 20.0, "x")]);
        let seen = Mutex::new(vec![]);
        let progress = |d: usize, n: usize| seen.lock().unwrap().push((d, n));
        run_with(
            std::slice::from_ref(&track),
            &d,
            &t,
            &no_dedup(),
            Hooks {
                namer: None,
                progress: Some(&progress),
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(*seen.lock().unwrap(), vec![(1, 2), (2, 2)]);
    }

    #[test]
    fn short_turns_are_transcribed_together_and_words_go_to_their_turn() {
        let tracks = vec![timed_track(TrackKind::System, 40.0)];
        let d = FakeDiarizer::new(vec![Ok(vec![
            turn(0.0, 3.0, "S1"),
            turn(3.2, 4.0, "S2"),
            turn(4.1, 9.0, "S1"),
            // 6 s pause: a new window
            turn(15.0, 18.0, "S2"),
        ])]);
        let t = WordTranscriber::new(vec![
            (1.0, "Hallo"),
            (2.5, "zusammen."),
            (3.5, "Ja."),
            (5.0, "ähm"),
            (5.5, "Weiter"),
            (8.9, "geht's."), // spills a little past its turn's end
            (16.0, "Tschüss."),
        ]);
        let r = run(
            &tracks,
            &d,
            &t,
            &PipelineConfig {
                window_s: Some(25.0),
                ..no_dedup()
            },
        )
        .unwrap();
        let calls = t.calls.lock().unwrap().clone();
        assert_eq!(
            calls.len(),
            2,
            "one window per stretch of speech: {calls:?}"
        );
        let lines: Vec<(&str, &str)> = r
            .transcript
            .segments
            .iter()
            .map(|s| (s.speaker.as_str(), s.text.as_str()))
            .collect();
        assert_eq!(
            lines,
            vec![
                ("Speaker 1", "Hallo zusammen."),
                ("Speaker 2", "Ja."),
                ("Speaker 1", "Weiter geht's."),
                ("Speaker 2", "Tschüss."),
            ]
        );
    }

    #[test]
    fn windows_stay_within_the_length_limit() {
        let tracks = vec![timed_track(TrackKind::System, 120.0)];
        let turns: Vec<SpeakerTurn> = (0..20)
            .map(|i| {
                turn(
                    i as f64 * 5.0,
                    i as f64 * 5.0 + 4.5,
                    if i % 2 == 0 { "S1" } else { "S2" },
                )
            })
            .collect();
        let d = FakeDiarizer::new(vec![Ok(turns)]);
        let t = WordTranscriber::new((0..20).map(|i| (i as f64 * 5.0 + 2.0, "wort")).collect());
        let cfg = PipelineConfig {
            window_s: Some(25.0),
            ..no_dedup()
        };
        let r = run(&tracks, &d, &t, &cfg).unwrap();
        let calls = t.calls.lock().unwrap().clone();
        assert!(calls.len() >= 4, "{calls:?}");
        for (a, b) in &calls {
            assert!(
                b - a <= 25.0 + 2.0 * cfg.pad_s + 1e-6,
                "window {a}..{b} too long"
            );
        }
        let words: usize = r
            .transcript
            .segments
            .iter()
            .map(|s| s.text.split_whitespace().count())
            .sum();
        assert_eq!(words, 20, "every word lands exactly once");
    }

    #[test]
    fn a_failed_window_is_retried_turn_by_turn_with_a_warning() {
        let tracks = vec![timed_track(TrackKind::System, 20.0)];
        let d = FakeDiarizer::new(vec![Ok(vec![turn(0.0, 3.0, "S1"), turn(3.5, 6.0, "S2")])]);
        let mut t = WordTranscriber::new(vec![(1.0, "eins"), (4.0, "zwei")]);
        t.fail_at = Some(2.0);
        let r = run(
            &tracks,
            &d,
            &t,
            &PipelineConfig {
                window_s: Some(25.0),
                ..no_dedup()
            },
        )
        .unwrap();
        let texts: Vec<&str> = r
            .transcript
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect();
        assert_eq!(texts, vec!["eins", "zwei"]);
        // Recovered, so nothing for the user; the log has it.
        assert!(r.warnings.is_empty(), "{:?}", r.warnings);
        assert!(
            r.details.iter().any(|w| w.contains("turn by turn")),
            "{:?}",
            r.details
        );
    }

    #[test]
    fn without_window_turns_are_transcribed_one_by_one() {
        let tracks = vec![timed_track(TrackKind::System, 20.0)];
        let d = FakeDiarizer::new(vec![Ok(vec![turn(0.0, 3.0, "S1"), turn(3.5, 6.0, "S2")])]);
        let t = WordTranscriber::new(vec![(1.0, "eins"), (4.0, "zwei")]);
        let cfg = PipelineConfig {
            window_s: None,
            ..no_dedup()
        };
        run(&tracks, &d, &t, &cfg).unwrap();
        assert_eq!(t.calls.lock().unwrap().len(), 2);
    }
    #[test]
    fn back_channel_ghost_with_few_words_is_absorbed() {
        let seg = |sp: &str, a: f64, b: f64, t: &str| Segment {
            start_s: a,
            end_s: b,
            track: TrackKind::System,
            speaker: sp.into(),
            text: t.into(),
        };
        let mut segs = vec![seg("A", 0.0, 400.0, "lang"), seg("B", 400.0, 800.0, "lang")];
        // Seven short replies, 10 s in all (over max_s) but only 10 words.
        for k in 0..7 {
            let a = 100.0 + k as f64 * 50.0;
            segs.push(seg("G", a, a + 1.5, "Yeah, genau."));
        }
        let out = absorb_ghost_speakers(segs, 0.04, 8.0);
        assert!(out.iter().all(|s| s.speaker != "G"));
    }

    // ------------------------------------------------ pieces from the call

    fn piece(s: f64, e: f64, words: &[(f64, &str)]) -> crate::live::Piece {
        crate::live::Piece {
            track: TrackKind::Mic,
            start_s: s,
            end_s: e,
            words: words
                .iter()
                .map(|(t, w)| TimedWord {
                    start_s: t - 0.1,
                    end_s: t + 0.1,
                    text: format!(" {w}"),
                })
                .collect(),
        }
    }

    /// S1 at 1-5 and 6-9, S2 at 12-15, S1 again at 20-24. The second piece
    /// holds S2 and S1.
    fn run_with_pieces(
        pieces: &[crate::live::Piece],
        any_speaker: bool,
    ) -> (PipelineResult, Vec<(f64, f64)>) {
        let tracks = vec![timed_track(TrackKind::Mic, 30.0)];
        let d = FakeDiarizer::new(vec![Ok(vec![
            turn(1.0, 5.0, "S1"),
            turn(6.0, 9.0, "S1"),
            turn(12.0, 15.0, "S2"),
            turn(20.0, 24.0, "S1"),
        ])]);
        let t = WordTranscriber::new(vec![(2.0, "neu"), (13.0, "fünf"), (21.0, "sechs")]);
        let cfg = PipelineConfig {
            echo: None,
            pieces_any_speaker: any_speaker,
            ..Default::default()
        };
        let r = run_with(
            &tracks,
            &d,
            &t,
            &cfg,
            Hooks {
                pieces: Some(pieces),
                ..Default::default()
            },
        )
        .unwrap();
        let calls = t.calls.lock().unwrap().clone();
        (r, calls)
    }

    fn texts(r: &PipelineResult) -> Vec<&str> {
        r.transcript
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect()
    }

    fn call_pieces() -> Vec<crate::live::Piece> {
        vec![
            // Words at 9.8 lie beyond the turn's padding: not transcribed
            // without pieces either, so dropped.
            piece(
                0.0,
                10.0,
                &[
                    (2.0, "eins"),
                    (3.0, "ähm"),
                    (4.0, "zwei"),
                    (7.0, "drei"),
                    (9.8, "rauschen"),
                ],
            ),
            piece(10.0, 22.0, &[(13.0, "falsch"), (21.0, "auch falsch")]),
            piece(22.0, 30.0, &[(23.0, "falsch")]),
        ]
    }

    #[test]
    fn turns_of_one_speaker_pieces_take_their_words_from_them() {
        let (r, calls) = run_with_pieces(&call_pieces(), false);
        assert_eq!(texts(&r), vec!["Eins zwei", "Drei", "fünf", "sechs"]);
        assert_eq!((r.jobs, r.jobs_from_pieces), (4, 2));
        // Only the turns under the piece with two speakers were transcribed.
        assert_eq!(calls.len(), 2, "{calls:?}");
        assert!((calls[0].0 - 11.85).abs() < 0.01 && (calls[0].1 - 15.15).abs() < 0.01);
        assert!((calls[1].0 - 19.85).abs() < 0.01 && (calls[1].1 - 24.15).abs() < 0.01);
    }

    #[test]
    fn with_any_speaker_every_covered_turn_takes_the_pieces_words() {
        let (r, calls) = run_with_pieces(&call_pieces(), true);
        assert_eq!(
            texts(&r),
            vec!["Eins zwei", "Drei", "Falsch", "Auch falsch falsch"]
        );
        assert_eq!(r.jobs_from_pieces, 4);
        assert!(calls.is_empty());
    }

    #[test]
    fn a_turn_the_pieces_do_not_fully_cover_is_transcribed() {
        // Only the first 5.5 s were transcribed during the call (a piece
        // failed, or the rest is the tail).
        let (r, calls) = run_with_pieces(&[piece(0.0, 5.5, &[(2.0, "eins")])], false);
        assert_eq!(texts(&r), vec!["Eins", "fünf", "sechs"]);
        assert_eq!(r.jobs_from_pieces, 1);
        assert_eq!(calls.len(), 3);
    }
}
