//! Transcript viewer core logic (F-080/F-081/F-082): timestamp parsing,
//! segment edit clamp rules, search, and the Text/Translation view text
//! builders.
//!
//! Pure, UI-free decision/formatting logic — the `humm-gui` viewer window
//! drives it and owns all I/O (store writes, egui widget state, playback).
//! Ported from Buzz's `transcription_segments_editor_widget.py` and
//! `transcription_viewer_widget.py`; exact line ranges are cited in each
//! submodule's doc comment. Design:
//! `docs/superpowers/specs/2026-07-23-m4-transcript-viewer-design.md`
//! §2.1-2.3.

pub mod edit;
pub mod search;
pub mod text_view;
pub mod timestamp;
