//! Post-processing hook seams (ADR-010: four native features, not a plugin
//! mechanism) — the pre-run `check_skip` seam (F-115, [`skip`]) plus, in a
//! later milestone task, the post-run `on_complete` hook chain (F-111
//! ai_summary, F-113 export_docx, F-116 transcript_resizer). Design:
//! `docs/superpowers/specs/2026-07-25-m7-completeness-design.md` §2, §4.

pub mod docx;
pub mod skip;
pub mod summary;

use crate::domain::Millis;
use crate::settings::{AiSummarySettings, AutoResizeSettings, ExportDocxSettings, SkipSettings};

/// A point-in-time snapshot of every post-processing hook's settings, taken
/// once per task ([`crate::queue::RunnerDeps::postproc`], design spec §2.4)
/// so a whole task's hook chain — the pre-run skip check and, later, the
/// post-run hooks — runs against one consistent view even if the user edits
/// Preferences mid-run.
///
/// Deliberately carries **no** `Serialize`/`Deserialize` derive:
/// [`Self::summary_api_key`] is a live secret a [`PostprocConfigSource`]
/// resolves from the keyring at snapshot time, never persisted to disk — a
/// derive here would be a standing invitation to round-trip it through JSON
/// somewhere by accident. [`std::fmt::Debug`] is hand-written below to
/// redact that one field instead (the M6 key-hygiene rule; model:
/// `GeneralTabState`'s `Debug`,
/// `crates/humm-gui/src/views/prefs_general.rs:234-246`).
#[derive(Clone, PartialEq)]
pub struct PostprocConfig {
    /// F-115 skip-already-transcribed settings.
    pub skip: SkipSettings,
    /// F-111 ai_summary settings.
    pub summary: AiSummarySettings,
    /// The AI-summary hook's resolved API key (a keyring lookup the
    /// [`PostprocConfigSource`] implementation makes at snapshot time) —
    /// `None` when no key is stored or the lookup failed.
    pub summary_api_key: Option<String>,
    /// F-113 export_docx settings.
    pub docx: ExportDocxSettings,
    /// F-116 transcript_resizer settings.
    pub resize: AutoResizeSettings,
    /// The paragraph-split threshold used by text-mode/DOCX-paragraph
    /// export hooks, resolved once per snapshot (design spec §7 ledger #L7;
    /// see [`crate::viewer::text_view::paragraph_split_from_env`]).
    pub paragraph_split: Millis,
}

impl std::fmt::Debug for PostprocConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let key_display = match &self.summary_api_key {
            Some(_) => "Some(\u{ab}redacted\u{bb})",
            None => "None",
        };
        f.debug_struct("PostprocConfig")
            .field("skip", &self.skip)
            .field("summary", &self.summary)
            .field("summary_api_key", &key_display)
            .field("docx", &self.docx)
            .field("resize", &self.resize)
            .field("paragraph_split", &self.paragraph_split)
            .finish()
    }
}

/// Supplies a fresh [`PostprocConfig`] snapshot per task
/// ([`crate::queue::RunnerDeps::postproc`]). `None` there — every caller
/// that predates M7, and any test double that doesn't need hooks — means
/// every hook is off, exactly as if this trait didn't exist yet.
pub trait PostprocConfigSource: Send + Sync {
    /// Returns the current settings snapshot.
    fn snapshot(&self) -> PostprocConfig;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> PostprocConfig {
        PostprocConfig {
            skip: SkipSettings::default(),
            summary: AiSummarySettings::default(),
            summary_api_key: Some("sk-super-secret".to_string()),
            docx: ExportDocxSettings::default(),
            resize: AutoResizeSettings::default(),
            paragraph_split: Millis(2000),
        }
    }

    #[test]
    fn debug_redacts_summary_api_key_when_present() {
        let text = format!("{:?}", sample());
        assert!(!text.contains("sk-super-secret"));
        assert!(text.contains("redacted"));
    }

    #[test]
    fn debug_shows_none_when_no_key() {
        let mut cfg = sample();
        cfg.summary_api_key = None;
        let text = format!("{:?}", cfg);
        assert!(!text.contains("redacted"));
        assert!(text.contains("summary_api_key: \"None\""));
    }
}
