//! Engine-independent clean-up of raw diarization output into turns that
//! satisfy the `meeting_core::Diarizer` contract: sorted, non-overlapping,
//! no tiny fragments, same-speaker neighbours merged.

/// A raw segment from an engine: times in seconds and an engine cluster index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RawSegment {
    pub start_s: f64,
    pub end_s: f64,
    pub cluster: i32,
}

impl RawSegment {
    pub fn duration(&self) -> f64 {
        self.end_s - self.start_s
    }
}

/// Clean-up knobs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PostprocessConfig {
    /// Turns shorter than this are absorbed into a neighbour (or dropped
    /// when no neighbour is within `absorb_gap_s`).
    pub min_turn_s: f64,
    /// Same-speaker turns separated by at most this much silence are merged.
    pub merge_gap_s: f64,
    /// A short fragment is absorbed only into a neighbour this close.
    pub absorb_gap_s: f64,
}

impl Default for PostprocessConfig {
    fn default() -> Self {
        Self {
            min_turn_s: 0.3,
            merge_gap_s: 0.5,
            absorb_gap_s: 1.0,
        }
    }
}

/// Returns clean, sorted, non-overlapping segments (still with engine
/// cluster ids).
pub fn clean(raw: &[RawSegment], cfg: &PostprocessConfig) -> Vec<RawSegment> {
    let mut segs: Vec<RawSegment> = raw
        .iter()
        .copied()
        .filter(|s| s.start_s.is_finite() && s.end_s.is_finite() && s.end_s > s.start_s)
        .map(|s| RawSegment {
            start_s: s.start_s.max(0.0),
            ..s
        })
        .collect();
    segs.sort_by(|a, b| {
        a.start_s
            .total_cmp(&b.start_s)
            .then(a.end_s.total_cmp(&b.end_s))
    });

    // 1. Remove overlaps. An earlier turn keeps its time; a later turn of
    //    another speaker starts where it ends, or is dropped when it lies
    //    entirely inside it (overlapped speech, e.g. a short "mhm").
    let mut out: Vec<RawSegment> = Vec::with_capacity(segs.len());
    for mut s in segs {
        if let Some(last) = out.last_mut() {
            if s.start_s < last.end_s {
                if s.cluster == last.cluster {
                    last.end_s = last.end_s.max(s.end_s);
                    continue;
                }
                if s.end_s <= last.end_s {
                    continue;
                }
                s.start_s = last.end_s;
            }
        }
        out.push(s);
    }

    merge_same_speaker(&mut out, cfg.merge_gap_s);

    // 2. Absorb tiny fragments, shortest first, into the closer neighbour.
    while let Some((i, _)) = out
        .iter()
        .enumerate()
        .filter(|(_, s)| s.duration() < cfg.min_turn_s)
        .min_by(|a, b| a.1.duration().total_cmp(&b.1.duration()))
    {
        let gap_prev = (i > 0).then(|| out[i].start_s - out[i - 1].end_s);
        let gap_next = (i + 1 < out.len()).then(|| out[i + 1].start_s - out[i].end_s);
        let into_prev = match (gap_prev, gap_next) {
            (Some(p), Some(n)) if p <= cfg.absorb_gap_s && n <= cfg.absorb_gap_s => {
                if (p - n).abs() < 1e-9 {
                    out[i - 1].duration() >= out[i + 1].duration()
                } else {
                    p < n
                }
            }
            (Some(p), _) if p <= cfg.absorb_gap_s => true,
            (_, Some(n)) if n <= cfg.absorb_gap_s => false,
            _ => {
                out.remove(i);
                continue;
            }
        };
        let frag = out.remove(i);
        if into_prev {
            out[i - 1].end_s = frag.end_s;
        } else {
            out[i].start_s = frag.start_s;
        }
        merge_same_speaker(&mut out, cfg.merge_gap_s);
    }
    out
}

fn merge_same_speaker(segs: &mut Vec<RawSegment>, max_gap: f64) {
    let mut merged: Vec<RawSegment> = Vec::with_capacity(segs.len());
    for s in segs.drain(..) {
        if let Some(last) = merged.last_mut() {
            if last.cluster == s.cluster && s.start_s - last.end_s <= max_gap {
                last.end_s = last.end_s.max(s.end_s);
                continue;
            }
        }
        merged.push(s);
    }
    *segs = merged;
}

/// Engine cluster ids in order of first appearance; index `i` gets label
/// `S{i+1}`.
pub fn cluster_order(segs: &[RawSegment]) -> Vec<i32> {
    let mut order = Vec::new();
    for s in segs {
        if !order.contains(&s.cluster) {
            order.push(s.cluster);
        }
    }
    order
}

/// Merges clusters until at most `max` remain: repeatedly the two clusters
/// with the most similar embeddings (cosine), the one with less talk time
/// relabelled into the other; their embedding becomes the talk-time
/// weighted mean. A cluster without an embedding goes into the cluster with
/// the most talk time. Segments may need [`clean`] again afterwards.
pub fn merge_clusters_to(
    segs: &mut [RawSegment],
    embeddings: &mut std::collections::HashMap<i32, Vec<f32>>,
    max: usize,
) {
    loop {
        let clusters = cluster_order(segs);
        if clusters.len() <= max.max(1) {
            return;
        }
        let lone = clusters
            .iter()
            .filter(|c| !embeddings.contains_key(c))
            .min_by(|a, b| talk(segs, **a).total_cmp(&talk(segs, **b)));
        let (from, into) = if let Some(&lone) = lone {
            let biggest = *clusters
                .iter()
                .filter(|c| **c != lone)
                .max_by(|a, b| talk(segs, **a).total_cmp(&talk(segs, **b)))
                .unwrap();
            (lone, biggest)
        } else {
            let (_, a, b) = most_similar(&clusters, embeddings).expect("two or more clusters");
            smaller_first(segs, a, b)
        };
        merge_pair(segs, embeddings, from, into);
    }
}

/// Merges clusters whose embeddings have cosine similarity of at least
/// `min_similarity`, most similar pair first (one voice split in two by the
/// diarizer, e.g. after a change in loudness or distance to the mic).
/// Clusters without an embedding are left alone. Segments may need [`clean`]
/// again afterwards. Returns the number of merges.
pub fn merge_similar(
    segs: &mut [RawSegment],
    embeddings: &mut std::collections::HashMap<i32, Vec<f32>>,
    min_similarity: f32,
) -> usize {
    let mut merges = 0;
    loop {
        let clusters: Vec<i32> = cluster_order(segs)
            .into_iter()
            .filter(|c| embeddings.contains_key(c))
            .collect();
        match most_similar(&clusters, embeddings) {
            Some((sim, a, b)) if sim >= min_similarity => {
                let (from, into) = smaller_first(segs, a, b);
                merge_pair(segs, embeddings, from, into);
                merges += 1;
            }
            _ => return merges,
        }
    }
}

fn talk(segs: &[RawSegment], c: i32) -> f64 {
    segs.iter()
        .filter(|s| s.cluster == c)
        .map(RawSegment::duration)
        .sum()
}

/// The pair of `clusters` (all with embeddings) with the highest cosine.
fn most_similar(
    clusters: &[i32],
    embeddings: &std::collections::HashMap<i32, Vec<f32>>,
) -> Option<(f32, i32, i32)> {
    let mut best: Option<(f32, i32, i32)> = None;
    for (i, a) in clusters.iter().enumerate() {
        for b in &clusters[i + 1..] {
            let sim = crate::vector::cosine(&embeddings[a], &embeddings[b]);
            if best.is_none_or(|(s, _, _)| sim > s) {
                best = Some((sim, *a, *b));
            }
        }
    }
    best
}

/// (the cluster with less talk time, the other).
fn smaller_first(segs: &[RawSegment], a: i32, b: i32) -> (i32, i32) {
    if talk(segs, a) < talk(segs, b) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Relabels `from` into `into`; the embedding becomes the talk-time weighted mean.
fn merge_pair(
    segs: &mut [RawSegment],
    embeddings: &mut std::collections::HashMap<i32, Vec<f32>>,
    from: i32,
    into: i32,
) {
    if let (Some(ef), Some(ei)) = (embeddings.get(&from), embeddings.get(&into)) {
        let (wf, wi) = (talk(segs, from) as f32, talk(segs, into) as f32);
        let mixed: Vec<f32> = ef.iter().zip(ei).map(|(f, i)| wf * f + wi * i).collect();
        embeddings.insert(into, crate::vector::l2_normalize(&mixed));
    }
    embeddings.remove(&from);
    for s in segs.iter_mut() {
        if s.cluster == from {
            s.cluster = into;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start_s: f64, end_s: f64, cluster: i32) -> RawSegment {
        RawSegment {
            start_s,
            end_s,
            cluster,
        }
    }

    fn cfg() -> PostprocessConfig {
        PostprocessConfig::default()
    }

    #[test]
    fn empty_stays_empty() {
        assert!(clean(&[], &cfg()).is_empty());
    }

    #[test]
    fn sorts_and_removes_overlap() {
        let out = clean(&[seg(5.0, 8.0, 1), seg(0.0, 6.0, 0)], &cfg());
        assert_eq!(out, vec![seg(0.0, 6.0, 0), seg(6.0, 8.0, 1)]);
    }

    #[test]
    fn drops_segment_inside_another_speakers_turn() {
        let out = clean(&[seg(0.0, 10.0, 0), seg(3.0, 4.0, 1)], &cfg());
        assert_eq!(out, vec![seg(0.0, 10.0, 0)]);
    }

    #[test]
    fn merges_same_speaker_short_gap_only() {
        let out = clean(
            &[seg(0.0, 2.0, 0), seg(2.3, 4.0, 0), seg(5.0, 6.0, 0)],
            &cfg(),
        );
        assert_eq!(out, vec![seg(0.0, 4.0, 0), seg(5.0, 6.0, 0)]);
    }

    #[test]
    fn absorbs_fragment_into_closer_neighbour() {
        // Fragment of speaker 1 between two turns of speaker 0: absorbed and
        // the three collapse into one turn.
        let out = clean(
            &[seg(0.0, 2.0, 0), seg(2.1, 2.3, 1), seg(2.4, 4.0, 0)],
            &cfg(),
        );
        assert_eq!(out, vec![seg(0.0, 4.0, 0)]);

        // Closer to the next turn.
        let out = clean(
            &[seg(0.0, 2.0, 0), seg(2.8, 3.0, 2), seg(3.1, 5.0, 1)],
            &cfg(),
        );
        assert_eq!(out, vec![seg(0.0, 2.0, 0), seg(2.8, 5.0, 1)]);
    }

    #[test]
    fn drops_isolated_fragment() {
        let out = clean(&[seg(0.0, 2.0, 0), seg(5.0, 5.2, 1)], &cfg());
        assert_eq!(out, vec![seg(0.0, 2.0, 0)]);
        assert!(clean(&[seg(1.0, 1.1, 0)], &cfg()).is_empty());
    }

    #[test]
    fn output_is_sorted_and_non_overlapping() {
        let raw = [
            seg(0.0, 3.0, 0),
            seg(2.5, 5.0, 1),
            seg(4.9, 5.1, 0),
            seg(5.05, 9.0, 2),
            seg(8.0, 8.5, 1),
            seg(9.2, 12.0, 2),
        ];
        let out = clean(&raw, &cfg());
        for w in out.windows(2) {
            assert!(w[0].end_s <= w[1].start_s, "{out:?}");
        }
        assert!(out.iter().all(|s| s.duration() >= 0.3));
    }

    #[test]
    fn invalid_segments_ignored() {
        let out = clean(
            &[seg(2.0, 1.0, 0), seg(f64::NAN, 1.0, 0), seg(-0.5, 1.0, 0)],
            &cfg(),
        );
        assert_eq!(out, vec![seg(0.0, 1.0, 0)]);
    }

    #[test]
    fn merges_most_similar_clusters() {
        let mut segs = vec![seg(0.0, 5.0, 1), seg(6.0, 7.0, 2), seg(8.0, 12.0, 3)];
        let mut emb = std::collections::HashMap::new();
        emb.insert(1, vec![1.0, 0.0]);
        emb.insert(2, vec![0.9, 0.1]);
        emb.insert(3, vec![0.0, 1.0]);
        merge_clusters_to(&mut segs, &mut emb, 2);
        // 2 is closest to 1 and talks less, so it becomes 1.
        assert_eq!(cluster_order(&segs), vec![1, 3]);
        assert_eq!(segs[1].cluster, 1);
        assert!(!emb.contains_key(&2));
        let e = &emb[&1];
        assert!(((e[0] * e[0] + e[1] * e[1]).sqrt() - 1.0).abs() < 1e-5);

        // Without embeddings: into the cluster with the most talk time.
        let mut segs = vec![seg(0.0, 5.0, 1), seg(6.0, 7.0, 2)];
        merge_clusters_to(&mut segs, &mut std::collections::HashMap::new(), 1);
        assert_eq!(cluster_order(&segs), vec![1]);
    }

    #[test]
    fn merges_only_similar_clusters() {
        // 1 and 2: one voice split in two (cosine 0.8); 3: someone else.
        let mut segs = vec![seg(0.0, 2.0, 2), seg(3.0, 9.0, 1), seg(10.0, 14.0, 3)];
        let mut emb = std::collections::HashMap::new();
        emb.insert(1, vec![1.0, 0.0, 0.0]);
        emb.insert(2, vec![0.8, 0.6, 0.0]);
        emb.insert(3, vec![0.0, 0.0, 1.0]);
        assert_eq!(merge_similar(&mut segs, &mut emb, 0.75), 1);
        // 2 talks less, so it becomes 1.
        assert_eq!(cluster_order(&segs), vec![1, 3]);
        assert_eq!(merge_similar(&mut segs, &mut emb, 0.75), 0);
        // Below the threshold nothing merges.
        let mut segs = vec![seg(0.0, 2.0, 1), seg(3.0, 9.0, 2)];
        let mut emb = std::collections::HashMap::new();
        emb.insert(1, vec![1.0, 0.0]);
        emb.insert(2, vec![0.7, 0.714]);
        assert_eq!(merge_similar(&mut segs, &mut emb, 0.75), 0);
        assert_eq!(cluster_order(&segs), vec![1, 2]);
    }

    #[test]
    fn first_appearance_order() {
        let segs = [seg(0.0, 1.0, 3), seg(1.0, 2.0, 0), seg(2.0, 3.0, 3)];
        assert_eq!(cluster_order(&segs), vec![3, 0]);
    }
}
