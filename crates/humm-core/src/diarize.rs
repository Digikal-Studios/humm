//! Speaker diarization boundary trait and types (F-087, M8 design spec §6,
//! ADR-024).
//!
//! Buzz's speaker identification runs NeMo's MSDD model plus
//! `ctc-forced-aligner` and a punctuation-restoration stack
//! (`speaker_identification_widget.py`) — not ported (ADR-011 outcome-level
//! parity): humm instead runs an ort-backed segmentation + embedding +
//! clustering pipeline (`humm-diarize`'s `OrtDiarizer`, implementing
//! [`Diarizer`] via the `speakrs` crate, ADR-024). This module holds only the
//! trait and its value types — pure, adapter-free, and therefore usable from
//! `humm-core`'s own unit tests without linking `ort`/`speakrs` (Standing
//! Policy adapter isolation: `humm-diarize` is the only crate naming either).
//!
//! [`assign_speakers`] and [`label_segments`] turn those [`SpeakerTurn`]s
//! into transcript-segment speaker labels — temporal-overlap assignment,
//! "Speaker 0..N" relabeling by first appearance, and consecutive-same-
//! speaker merge — pure functions, no I/O, unit-tested in this module
//! directly (Buzz's save path, `speaker_identification_widget.py:727-800`).

use crate::boundary::Pcm;
use crate::cancel::CancelToken;
use crate::domain::{Millis, Segment};

/// One detected speaker turn: a contiguous span of audio attributed to a
/// single speaker.
///
/// `speaker` is a diarizer-local, zero-based index (e.g. `speakrs`'s
/// `SPEAKER_00` -> `0`) — not yet a human-facing "Speaker N" label. Relabeling
/// by order of first appearance across a whole run (Buzz's label shape,
/// `speaker_identification_widget.py:743-773`) is the transcript-mapping
/// layer's job, not this type's.
///
/// **Two properties later mapping code must not assume away:**
/// - **Ordering is a contract [`Diarizer::diarize`] guarantees, not an
///   incidental property of any one implementation.** `humm-diarize`'s
///   `OrtDiarizer` re-sorts by `start` defensively before returning, rather
///   than trusting `speakrs`'s own internal ordering to hold across every
///   inference path (sequential vs. concurrent) — see its module doc.
///   Callers may rely on the chronological-order guarantee; they must not
///   assume any particular *implementation* achieves it for free.
/// - **Turns from *different* speakers can overlap in time.** A diarizer
///   scans each speaker's activity independently (`speakrs`'s pipeline
///   binarizes one frame-activation column per speaker), so two speakers'
///   turns can genuinely overlap (simultaneous/cross-talk speech) — the
///   turn list is not a clean non-overlapping partition of the timeline.
///   The overlap-to-transcript-segment mapping ([`assign_speakers`]) must
///   handle overlapping candidate turns explicitly (e.g. "maximal overlap
///   wins"), not assume at most one turn is ever active at a given instant.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SpeakerTurn {
    /// Turn start time.
    pub start: Millis,
    /// Turn end time.
    pub end: Millis,
    /// Zero-based, diarizer-local speaker index.
    pub speaker: usize,
}

/// A coarse phase of a diarization run, for progress reporting.
///
/// Buzz's Qt progress dialog shows a numbered step count
/// (`speaker_identification_widget.py`'s "N/8 ..."); humm instead reports
/// the diarizer's *real* stages (UX_SPEC: progress must be real, not
/// cosmetic). Not every implementation can distinguish every variant here —
/// see `humm-diarize`'s `OrtDiarizer` doc for which stages it actually
/// emits and why.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiarizeStage {
    /// Loading the diarization models into memory (constructing inference
    /// sessions from the local model files).
    LoadingModels,
    /// Running speaker-change segmentation over the audio.
    Segmenting,
    /// Extracting per-segment speaker embeddings.
    Embedding,
    /// Clustering embeddings into speaker identities.
    Clustering,
}

/// One progress update from a [`Diarizer::diarize`] run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DiarizeProgress {
    /// The stage this update describes.
    pub stage: DiarizeStage,
    /// Fractional completion within `stage`, in `[0.0, 1.0]`, when the
    /// implementation can genuinely measure it. `None` — never a fake
    /// interpolated value — when it can't (e.g. a single monolithic call
    /// with no intermediate hook).
    pub fraction: Option<f32>,
}

/// Errors from a [`Diarizer::diarize`] run.
#[derive(Debug, thiserror::Error)]
pub enum DiarizeError {
    /// The diarization models are missing or incomplete locally.
    #[error("diarization models missing — {detail}")]
    ModelsMissing {
        /// Actionable detail (which file(s), and how to obtain them).
        detail: String,
    },
    /// Diarization ran but failed.
    #[error("diarization failed — {detail}")]
    Failed {
        /// Diagnostic detail from the underlying pipeline.
        detail: String,
    },
    /// The operation was canceled via `CancelToken`.
    #[error("diarization canceled")]
    Canceled,
}

/// Detects speaker turns in already-decoded audio (F-087).
///
/// Implemented by `humm-diarize`'s `OrtDiarizer` (an ort-backed segmentation,
/// embedding, and clustering pipeline via the `speakrs` crate, ADR-024).
/// Buzz's NeMo MSDD + ctc-forced-aligner + punctuation stack is not ported
/// (ADR-011 outcome-level parity: speaker count/boundaries are diffable,
/// labels never are).
pub trait Diarizer: Send + Sync {
    /// Diarizes `audio`, emitting [`DiarizeProgress`] updates to `progress`
    /// and checking `cancel` cooperatively. Returns the detected speaker
    /// turns sorted by `start` (a contract implementations must actively
    /// uphold, not merely inherit — see [`SpeakerTurn`]'s doc); turns from
    /// different speakers may still overlap in time (also [`SpeakerTurn`]'s
    /// doc) — sorted is not the same claim as non-overlapping.
    fn diarize(
        &self,
        audio: &Pcm,
        progress: &mut dyn FnMut(DiarizeProgress),
        cancel: &CancelToken,
    ) -> Result<Vec<SpeakerTurn>, DiarizeError>;
}

/// Assigns each of `segments` a speaker index by maximal temporal overlap
/// with `turns`, then relabels the result `0..N` in order of first
/// appearance across `segments` — Buzz's "Speaker N" label shape
/// (`speaker_identification_widget.py:743-773`); this returns the bare
/// index, [`label_segments`] does the `"Speaker N"` string formatting.
///
/// Per segment:
/// - The candidate turn with the largest overlapping duration wins. Turns
///   from *different* speakers can overlap in time ([`SpeakerTurn`]'s doc),
///   so more than one turn may be a genuine candidate for the same segment;
///   **ties go to the earlier turn** — the first one reached while scanning
///   `turns` in order. Since [`Diarizer::diarize`] guarantees `turns` sorted
///   by `start`, "first reached" and "earlier-starting" coincide.
/// - A segment with **zero** overlap against every turn (e.g. a transcript
///   gap the diarizer didn't cover) is assigned the turn nearest by
///   **midpoint distance** instead; ties again go to the earlier turn.
/// - `turns` empty -> every segment gets speaker `0` (nothing to assign
///   against, so everything is one speaker).
///
/// Relabeling walks `segments` in order and assigns new indices `0, 1, 2,
/// ...` the first time each *raw* speaker value is seen — by first
/// appearance in `segments`' own sequence, not by the raw index's numeric
/// value or the order speakers first appear in `turns`. For example, if the
/// turn overlapping the first segment happens to carry raw speaker `1` and
/// the turn overlapping the second segment carries raw speaker `0`, the
/// result is `[0, 1]` (first thing seen becomes speaker 0) — not `[1, 0]`.
///
/// Assumes `segments` is already ordered by `start` (the store's own
/// ordering guarantee, `docs/design/ARCHITECTURE.md`); this function does
/// not re-sort or validate that assumption.
pub fn assign_speakers(segments: &[Segment], turns: &[SpeakerTurn]) -> Vec<usize> {
    if turns.is_empty() {
        return vec![0; segments.len()];
    }

    let raw: Vec<usize> = segments.iter().map(|seg| raw_speaker(seg, turns)).collect();
    relabel_by_first_appearance(&raw)
}

/// Overlap between `segment` and `turn`, in milliseconds; `0` (never
/// negative) when they don't overlap at all.
fn overlap_ms(segment: &Segment, turn: &SpeakerTurn) -> i64 {
    let start = segment.start.0.max(turn.start.0);
    let end = segment.end.0.min(turn.end.0);
    (end - start).max(0)
}

/// Midpoint of a `[start, end]` span, in milliseconds.
fn midpoint_ms(start: Millis, end: Millis) -> i64 {
    (start.0 + end.0) / 2
}

/// The raw (diarizer-local) speaker index [`assign_speakers`] picks for one
/// segment — see its doc for the max-overlap / nearest-midpoint / tie
/// rules. Callers must not pass an empty `turns` (the only caller,
/// [`assign_speakers`], checks that first).
fn raw_speaker(segment: &Segment, turns: &[SpeakerTurn]) -> usize {
    let mut best: Option<(usize, i64)> = None; // (turn index, overlap ms)
    for (i, turn) in turns.iter().enumerate() {
        let overlap = overlap_ms(segment, turn);
        let is_better = match best {
            Some((_, best_overlap)) => overlap > best_overlap,
            None => overlap > 0,
        };
        if is_better {
            best = Some((i, overlap));
        }
    }
    if let Some((i, _)) = best {
        return turns[i].speaker;
    }

    // Zero overlap with every turn: fall back to nearest by midpoint
    // distance (strict `<` keeps the earlier turn on a distance tie, same
    // rule as the overlap path above).
    let seg_mid = midpoint_ms(segment.start, segment.end);
    let mut nearest: Option<(usize, i64)> = None; // (turn index, distance ms)
    for (i, turn) in turns.iter().enumerate() {
        let dist = (midpoint_ms(turn.start, turn.end) - seg_mid).abs();
        let is_nearer = match nearest {
            Some((_, best_dist)) => dist < best_dist,
            None => true,
        };
        if is_nearer {
            nearest = Some((i, dist));
        }
    }
    let (i, _) = nearest.expect("turns is non-empty — checked by assign_speakers");
    turns[i].speaker
}

/// Relabels raw speaker indices `0..N` in order of first appearance in
/// `raw` (see [`assign_speakers`]'s doc for why this is first-appearance-
/// in-segment-order, not raw-value order).
fn relabel_by_first_appearance(raw: &[usize]) -> Vec<usize> {
    let mut seen: Vec<usize> = Vec::new();
    raw.iter()
        .map(|&speaker| match seen.iter().position(|&s| s == speaker) {
            Some(pos) => pos,
            None => {
                seen.push(speaker);
                seen.len() - 1
            }
        })
        .collect()
}

/// `names[idx]`, or `"Speaker {idx}"` when `idx` is out of range for
/// `names`. Defensive fallback — `speakers[i]` values are always in range
/// when `names` was built from the same relabeled speaker set (e.g. one
/// name per unique index returned by [`assign_speakers`]), but a caller
/// passing a shorter `names` gets a readable placeholder instead of a
/// panic.
fn speaker_name(idx: usize, names: &[String]) -> String {
    names
        .get(idx)
        .cloned()
        .unwrap_or_else(|| format!("Speaker {idx}"))
}

/// Builds display segments from `segments`/`speakers` (as returned by
/// [`assign_speakers`]) and `names`, in Buzz's `"{name}: {text}"` save shape
/// (`speaker_identification_widget.py:757` / `:780`,
/// `on_save_button_clicked`). `speakers[i]` indexes into `names`; an
/// out-of-range index falls back to `"Speaker {idx}"` (see
/// [`speaker_name`]).
///
/// Output segments always carry an empty `translation`: Buzz's save path
/// constructs fresh `Segment`s with no translation at all (`:754-782`), and
/// `humm_store`'s `mark_replaced_completed` drops translations on insert
/// regardless (Buzz parity — same reasoning as `crate::resize`'s
/// `equalize`/`extend_endings`).
///
/// `merge == false`: every segment is labeled independently, timestamps
/// preserved exactly (`:774-782`).
///
/// `merge == true`: consecutive segments merge when they resolve to the
/// **same entered name** — not the same raw speaker index. Buzz's merge
/// compares the *mapped* name (`:747`: `speaker_name =
/// speaker_mapping.get(entry['speaker'], entry['speaker'])`, then `:749`:
/// `previous_segment['speaker'] == speaker_name`), so two different speaker
/// indices given the same name in `names` **do** merge; this is mirrored
/// here by comparing the resolved name string, never `speakers[i]` itself.
/// A merged run keeps the first segment's `start`, extends `end` to the
/// last segment's `end`, and joins texts with a single space (`:750-751`).
///
/// `speakers` is assumed to have one entry per `segments` element (the
/// contract [`assign_speakers`] always upholds for the same segment slice);
/// if the slices differ in length, the shorter one wins via
/// [`Iterator::zip`] and any extra `segments` are silently dropped — not a
/// case this function detects or corrects.
pub fn label_segments(
    segments: &[Segment],
    speakers: &[usize],
    names: &[String],
    merge: bool,
) -> Vec<Segment> {
    if !merge {
        return segments
            .iter()
            .zip(speakers)
            .map(|(seg, &speaker)| Segment {
                start: seg.start,
                end: seg.end,
                text: format!("{}: {}", speaker_name(speaker, names), seg.text),
                translation: String::new(),
            })
            .collect();
    }

    /// An in-progress merged run of consecutive same-name segments.
    struct Run {
        start: Millis,
        end: Millis,
        name: String,
        text: String,
    }

    let finalize = |run: Run| Segment {
        start: run.start,
        end: run.end,
        text: format!("{}: {}", run.name, run.text),
        translation: String::new(),
    };

    let mut out = Vec::new();
    let mut run: Option<Run> = None;

    for (seg, &speaker) in segments.iter().zip(speakers) {
        let name = speaker_name(speaker, names);
        match &mut run {
            Some(r) if r.name == name => {
                r.end = seg.end;
                r.text.push(' ');
                r.text.push_str(&seg.text);
            }
            _ => {
                if let Some(prev) = run.take() {
                    out.push(finalize(prev));
                }
                run = Some(Run {
                    start: seg.start,
                    end: seg.end,
                    name,
                    text: seg.text.clone(),
                });
            }
        }
    }
    if let Some(prev) = run {
        out.push(finalize(prev));
    }

    out
}

/// Labels `segments` with the existing PCM diarizer. Speaker count and a
/// named algorithm are not inputs: [`Diarizer::diarize`] takes audio only.
pub fn identify_with_diarizer(
    segments: &[Segment],
    audio: &Pcm,
    diarizer: &dyn Diarizer,
    cancel: &CancelToken,
) -> Result<Vec<Segment>, DiarizeError> {
    let turns = diarizer.diarize(audio, &mut |_| {}, cancel)?;
    let speakers = assign_speakers(segments, &turns);
    let count = speakers
        .iter()
        .copied()
        .max()
        .map(|speaker| speaker + 1)
        .unwrap_or(0);
    let names: Vec<String> = (0..count)
        .map(|speaker| format!("Speaker {speaker}"))
        .collect();
    Ok(label_segments(segments, &speakers, &names, true))
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn seg(start: i64, end: i64, text: &str) -> Segment {
        Segment {
            start: Millis(start),
            end: Millis(end),
            text: text.to_string(),
            translation: String::new(),
        }
    }

    fn turn(start: i64, end: i64, speaker: usize) -> SpeakerTurn {
        SpeakerTurn {
            start: Millis(start),
            end: Millis(end),
            speaker,
        }
    }

    fn names(labels: &[&str]) -> Vec<String> {
        labels.iter().map(|s| s.to_string()).collect()
    }

    // -- assign_speakers --------------------------------------------------

    #[test]
    fn assign_by_max_overlap() {
        let turns = vec![
            turn(0, 700, 5),     // 700ms overlap with seg 1
            turn(600, 1000, 9),  // 400ms overlap with seg 1
            turn(1000, 2000, 9), // full overlap with seg 2
        ];
        let segments = vec![seg(0, 1000, "a"), seg(1000, 2000, "b")];

        // raw speakers [5, 9] relabel to [0, 1] by first appearance.
        assert_eq!(assign_speakers(&segments, &turns), vec![0, 1]);
    }

    #[test]
    fn assign_tie_breaks_to_earlier_turn() {
        let turns = vec![
            turn(0, 500, 9),    // earlier turn: 500ms overlap with seg 1
            turn(500, 1000, 3), // later turn: 500ms overlap with seg 1 -- tie
        ];
        let segments = vec![
            seg(0, 1000, "a"),   // ties between turn 0 (speaker 9) and turn 1 (speaker 3)
            seg(500, 1000, "b"), // unambiguously overlaps only turn 1 (speaker 3)
        ];

        // If the tie went to the earlier turn (speaker 9), raw = [9, 3],
        // relabeled [0, 1]. If it went to the later turn instead, raw would
        // be [3, 3], relabeled [0, 0].
        assert_eq!(assign_speakers(&segments, &turns), vec![0, 1]);
    }

    #[test]
    fn zero_overlap_takes_nearest_turn() {
        let turns = vec![
            turn(0, 1000, 0),    // turn A, midpoint 500
            turn(5000, 6000, 1), // turn B, midpoint 5500
        ];
        let segments = vec![
            seg(100, 200, "a"),   // overlaps turn A directly
            seg(4000, 4200, "b"), // zero overlap; midpoint 4100 is nearer B (dist 1400) than A (dist 3600)
        ];

        assert_eq!(assign_speakers(&segments, &turns), vec![0, 1]);
    }

    #[test]
    fn relabel_orders_by_first_appearance() {
        // Turns arrive in raw-speaker order [1, 0]; segments align with
        // them in that order, so segment speakers relabel to [0, 1] (first
        // appearance), not [1, 0] (raw value).
        let turns = vec![turn(0, 1000, 1), turn(1000, 2000, 0)];
        let segments = vec![seg(0, 1000, "a"), seg(1000, 2000, "b")];

        assert_eq!(assign_speakers(&segments, &turns), vec![0, 1]);
    }

    #[test]
    fn empty_turns_single_speaker() {
        let segments = vec![
            seg(0, 1000, "a"),
            seg(1000, 2000, "b"),
            seg(2000, 3000, "c"),
        ];
        assert_eq!(assign_speakers(&segments, &[]), vec![0, 0, 0]);
    }

    // -- label_segments -----------------------------------------------------

    #[test]
    fn label_without_merge_prefixes_every_segment() {
        let mut segments = vec![seg(0, 1000, "hello"), seg(1000, 2000, "world")];
        segments[0].translation = "hola".to_string(); // must not survive to output

        let out = label_segments(&segments, &[0, 1], &names(&["Alice", "Bob"]), false);

        assert_eq!(
            out,
            vec![
                Segment {
                    start: Millis(0),
                    end: Millis(1000),
                    text: "Alice: hello".to_string(),
                    translation: String::new(),
                },
                Segment {
                    start: Millis(1000),
                    end: Millis(2000),
                    text: "Bob: world".to_string(),
                    translation: String::new(),
                },
            ]
        );
    }

    #[test]
    fn label_out_of_range_speaker_falls_back_to_speaker_n() {
        let segments = vec![seg(0, 1000, "hi")];
        let out = label_segments(&segments, &[2], &names(&["Alice"]), false);
        assert_eq!(out[0].text, "Speaker 2: hi");
    }

    #[test]
    fn merge_joins_consecutive_same_speaker() {
        let segments = vec![
            seg(0, 1000, "a"),
            seg(1000, 2000, "b"),
            seg(2000, 3000, "c"),
        ];
        let speakers = vec![0, 0, 1];
        let out = label_segments(&segments, &speakers, &names(&["Alice", "Bob"]), true);

        assert_eq!(
            out,
            vec![
                Segment {
                    start: Millis(0),
                    end: Millis(2000),
                    text: "Alice: a b".to_string(),
                    translation: String::new(),
                },
                Segment {
                    start: Millis(2000),
                    end: Millis(3000),
                    text: "Bob: c".to_string(),
                    translation: String::new(),
                },
            ]
        );
        // Boundary is preserved exactly across the speaker change.
        assert_eq!(out[0].end, out[1].start);
    }

    #[test]
    fn merge_respects_renames() {
        // Two different (already-relabeled) speaker indices given the same
        // entered name merge, mirroring Buzz's merge-on-mapped-name
        // behavior (`:747-751`).
        let segments = vec![seg(0, 1000, "hi"), seg(1000, 2000, "there")];
        let speakers = vec![0, 1];
        let out = label_segments(&segments, &speakers, &names(&["Alice", "Alice"]), true);

        assert_eq!(
            out,
            vec![Segment {
                start: Millis(0),
                end: Millis(2000),
                text: "Alice: hi there".to_string(),
                translation: String::new(),
            }]
        );
    }

    #[test]
    fn merge_single_segment_is_a_passthrough() {
        let segments = vec![seg(0, 1000, "solo")];
        let out = label_segments(&segments, &[0], &names(&["Alice"]), true);
        assert_eq!(
            out,
            vec![Segment {
                start: Millis(0),
                end: Millis(1000),
                text: "Alice: solo".to_string(),
                translation: String::new(),
            }]
        );
    }

    struct OneTurn;

    impl Diarizer for OneTurn {
        fn diarize(
            &self,
            _audio: &Pcm,
            _progress: &mut dyn FnMut(DiarizeProgress),
            _cancel: &CancelToken,
        ) -> Result<Vec<SpeakerTurn>, DiarizeError> {
            Ok(vec![turn(0, 2000, 0)])
        }
    }

    #[test]
    fn identify_with_diarizer_prefixes_the_pcm_speakers() {
        let segments = vec![seg(0, 1000, "hello"), seg(1000, 2000, "there")];
        let labeled = identify_with_diarizer(
            &segments,
            &Pcm::new(vec![0.0; 8]),
            &OneTurn,
            &CancelToken::new(),
        )
        .expect("diarize");
        assert_eq!(labeled[0].text, "Speaker 0: hello there");
    }
}
