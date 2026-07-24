//! Fluent-backed string catalog (F-008 scaffold).
//!
//! Every user-visible string in `humm-gui` is a key resolved through
//! [`tr`]/[`tr_args`] rather than an inline literal — Buzz's gettext catalog
//! (`buzz/locale.py`) ported to Fluent (ADR-014). Only the `en` catalog is
//! embedded this milestone; locale negotiation (Buzz's 15 UI locales,
//! `buzz-notes-options-shortcuts-telemetry.md` §F-008) lands with the locale
//! ports in a later milestone. Until then the bundle is always English.
//!
//! [`FluentBundle`] is not `Sync` (its default memoizer uses interior
//! mutability that isn't thread-safe), so it cannot live in a plain `static`.
//! egui itself is single-threaded (all `tr`/`tr_args` calls happen from the
//! UI thread that owns the `egui::Context`), so a `thread_local!` is the
//! right tool: no cross-thread sharing is ever needed, and each thread that
//! happens to call `tr` (only ever one, in practice) gets its own bundle,
//! built once and reused.

use std::cell::RefCell;

use fluent::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use unic_langid::langid;

/// The embedded English Fluent catalog source.
const EN_FTL: &str = include_str!("../assets/i18n/en/humm.ftl");

/// Every catalog key `humm-gui` actually references from view code, asserted
/// resolvable by this module's unit test (the F-008 "matrix method": unit
/// catalog load). Extend this list in the same commit as any `tr("new-key")`
/// call — an unlisted key can silently render as its own name in the UI with
/// nothing catching it.
pub fn all_keys_used() -> &'static [&'static str] {
    &[
        "hero-drop",
        "hero-import-button",
        "table-col-file-name",
        "table-col-model",
        "table-col-task",
        "table-col-status",
        "table-col-date-completed",
        "table-col-date-added",
        "table-col-notes",
        "table-menu-reset-column-order",
        "table-menu-restart",
        "table-menu-rename",
        "table-menu-notes",
        "task-transcribe",
        "task-translate",
        // Toolbar (F-004, Task 8; Record unlocked F-004/F-060, Task 10)
        "toolbar-record",
        "toolbar-new-file",
        "toolbar-new-url",
        "toolbar-open-transcript",
        "toolbar-open-transcript-tooltip",
        "toolbar-cancel",
        "toolbar-clear",
        // Menu bar (F-004, Task 8)
        "menu-file",
        "menu-file-import-file",
        "menu-file-import-url",
        "menu-file-import-folder",
        "menu-help-title",
        "menu-help-about",
        "menu-help-docs",
        "menu-help-docs-tooltip",
        "menu-help-preferences",
        // Clear-History confirm modal (F-026, Task 8)
        "clear-confirm-title",
        "clear-confirm-body",
        "clear-confirm-ok",
        "clear-confirm-cancel",
        "clear-error",
        // Rename/Notes modals (F-026, Task 8)
        "rename-title",
        "rename-label",
        "rename-error",
        "notes-title",
        "notes-label",
        "notes-error",
        "dialog-ok",
        "dialog-cancel",
        // Toasts (F-026, Task 8)
        "toast-completed",
        "toast-completed-suffix",
        "toast-canceled",
        "toast-detail-title",
        "toast-detail-open-log",
        "toast-detail-retry",
        "toast-detail-close",
        "retry-error-unavailable",
        // Store-health banner (F-004, Task 8)
        "banner-store-health",
        // Drag-and-drop overlay (F-021, Task 9)
        "drop-overlay-text",
        // URL-import capability gating (F-022, Task 9)
        "url-import-unavailable-tooltip",
        // URL-import dialog (F-022, Task 9)
        "url-dialog-title",
        "url-dialog-label",
        "url-dialog-placeholder",
        "url-dialog-invalid",
        "url-dialog-ok",
        "url-dialog-cancel",
        // Options sheet (F-023, Task 9)
        "options-model-label",
        "options-task-label",
        "options-language-label",
        "language-detect",
        "options-word-timestamps",
        "options-extract-speech",
        "options-extract-speech-tooltip",
        "options-export-label",
        "options-export-txt",
        "options-export-srt",
        "options-export-vtt",
        "options-summary-no-formats",
        "options-advanced",
        "options-initial-prompt-label",
        "options-hf-repo-label",
        "options-run",
        // Options sheet -- backend selector + Api Key row + LLM translation
        // block (F-023 extension, F-041, F-031, Task 14)
        "options-backend-label",
        "options-backend-whisper",
        "options-backend-openai-api",
        "options-api-key-label",
        "options-llm-heading",
        "options-llm-enable",
        "options-llm-model-label",
        "options-llm-model-presets-button",
        "options-llm-prompt-label",
        // Update chip + dialog (views/update_notice.rs, F-006, ADR-015)
        "update-chip-label",
        "update-dialog-title",
        "update-dialog-header",
        "update-dialog-current-version",
        "update-dialog-new-version",
        "update-dialog-release-notes",
        "update-dialog-open-release-page",
        "update-dialog-close",
        // About window (views/about.rs, F-006 gui-manual half)
        "about-app-name",
        "about-version-prefix",
        "about-check-button",
        "about-show-logs-button",
        "about-close-button",
        "about-up-to-date",
        "about-check-failed",
        // Preferences window shell (views/preferences.rs, F-044/ADR-009)
        "prefs-title",
        "prefs-tab-general",
        "prefs-tab-models",
        "prefs-tab-privacy",
        // Preferences General tab -- OpenAI block only (views/prefs_general.rs,
        // partial F-100, design spec §7.1, Task 13)
        "prefs-general-openai-key-label",
        "prefs-general-openai-key-placeholder",
        "prefs-general-key-show",
        "prefs-general-key-hide",
        "prefs-general-test-button",
        "prefs-general-test-title",
        "prefs-general-test-success-body",
        "prefs-general-invalid-key-title",
        "prefs-general-invalid-key-body",
        "prefs-general-key-save-error-title",
        "prefs-general-key-save-error-body",
        "prefs-general-base-url-label",
        "prefs-general-base-url-placeholder",
        "prefs-general-model-label",
        "prefs-general-model-placeholder",
        // Privacy tab (views/preferences.rs, ADR-009, UX §J-11)
        "privacy-telemetry-checkbox",
        "privacy-telemetry-disabled-caption",
        "privacy-telemetry-items-intro",
        "privacy-telemetry-item-app-version",
        "privacy-telemetry-item-system-name",
        "privacy-telemetry-item-os-version",
        "privacy-telemetry-item-kernel-release",
        "privacy-telemetry-item-cpu-architecture",
        "privacy-telemetry-item-system-locale",
        "privacy-telemetry-item-identifier",
        "privacy-update-checkbox",
        // Models tab (models_ui.rs, F-044 basic)
        "models-section-downloaded",
        "models-section-available",
        "models-download-button",
        "models-delete-button",
        "models-show-location-button",
        "models-cancel-button",
        "models-custom-url-label",
        "models-delete-confirm-title",
        "models-delete-confirm-body",
        "models-download-error-title",
        "models-download-failed",
        // Transcript viewer window (views/viewer.rs, F-080)
        "viewer-view",
        "viewer-mode-text",
        "viewer-mode-translation",
        "viewer-mode-timestamps",
        "viewer-export",
        "viewer-translate",
        "viewer-translate-tooltip",
        "viewer-resize",
        "viewer-identify-speakers",
        "viewer-identify-speakers-tooltip",
        "viewer-find",
        "viewer-find-label",
        "viewer-find-placeholder",
        "viewer-find-clear",
        "viewer-find-prev-tooltip",
        "viewer-find-next-tooltip",
        "viewer-empty",
        "viewer-open-error",
        // Transcript segment editor table (views/viewer_table.rs, F-081)
        "viewer-col-start",
        "viewer-col-end",
        "viewer-col-text",
        "viewer-col-translation",
        "viewer-saved",
        "viewer-unsaved-warning",
        // Synced playback: player row + controls bar (F-083, views/viewer.rs,
        // views/playback_controls.rs)
        "viewer-player-loading",
        "viewer-player-failed",
        "viewer-playback-controls",
        "viewer-loop-segment",
        "viewer-loop-segment-tooltip",
        "viewer-follow-audio",
        "viewer-follow-audio-tooltip",
        "viewer-speed",
        "viewer-scroll-to-current",
        "viewer-scroll-to-current-tooltip",
        // Export menu (views/export_menu.rs, F-084)
        "viewer-export-text",
        "viewer-export-translation",
        "viewer-export-dialog-title",
        "viewer-export-filter",
        "viewer-export-failed",
        "export-written",
        // Transcript resizer window (views/resizer.rs, F-086)
        "resizer-settings",
        "resizer-create-new",
        "resizer-extend-heading",
        "resizer-extend-label",
        "resizer-extend-button",
        "resizer-resize-heading",
        "resizer-resize-label",
        "resizer-resize-button",
        "resizer-resize-disabled-tooltip",
        "resizer-merge-heading",
        "resizer-merge-by-gap",
        "resizer-split-by-punctuation",
        "resizer-split-by-max-length",
        "resizer-merge-button",
        "resizer-merge-disabled-tooltip",
        "resizer-merging",
        "resizer-merge-error",
        "resizer-save-error",
        // Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
        "recording-window-title",
        "recording-model-load-note",
        "recording-advanced-button",
        "recording-microphone-label",
        "recording-record-button",
        "recording-stop-button",
        "recording-realtime-warning",
        "recording-panel-placeholder",
        "recording-model-downloading",
        "recording-model-downloading-indeterminate",
        "recording-model-loading",
        "recording-error-prefix",
        "recording-error-suffix",
        "recording-thread-crashed",
        "meter-average-volume",
        "meter-queue",
        // Recording advanced-settings sheet (views/recording_advanced.rs, Task 12)
        "recording-advanced-title",
        "recording-advanced-speech-heading",
        "recording-advanced-initial-prompt-label",
        "recording-advanced-translation-heading",
        "recording-advanced-llm-enable",
        "recording-advanced-llm-tooltip",
        "recording-advanced-llm-model-label",
        "recording-advanced-llm-instructions-label",
        "recording-advanced-recording-heading",
        "recording-advanced-silence-threshold-label",
        "recording-advanced-mode-label",
        "recording-advanced-mode-append-below",
        "recording-advanced-mode-append-above",
        "recording-advanced-mode-append-and-correct",
        "recording-advanced-separator-label",
        "recording-advanced-step-label",
        "recording-advanced-hide-unconfirmed",
        "recording-advanced-export-enable",
        "recording-advanced-export-folder-label",
        "recording-advanced-export-browse",
        "recording-advanced-export-file-name-label",
        "recording-advanced-export-file-type-label",
        "recording-advanced-export-file-type-txt",
        "recording-advanced-export-file-type-csv",
        "recording-advanced-export-max-entries-label",
        "recording-advanced-upload-url-label",
        "recording-advanced-ok",
        // Presentation window + bar (views/presentation.rs, Task 12)
        "presentation-show-button",
        "presentation-show-tooltip",
        "presentation-text-size-label",
        "presentation-theme-label",
        "presentation-theme-light",
        "presentation-theme-dark",
        "presentation-theme-custom",
        "presentation-text-color-label",
        "presentation-background-color-label",
        "presentation-fullscreen-button",
        "presentation-window-title",
        // Copy bar (views/recording.rs, Task 12)
        "recording-copy-button",
        "recording-copy-tooltip",
        "recording-copy-copied",
        "recording-copy-nothing",
    ]
}

thread_local! {
    static BUNDLE: RefCell<FluentBundle<FluentResource>> = RefCell::new(build_bundle());
}

fn build_bundle() -> FluentBundle<FluentResource> {
    let resource = FluentResource::try_new(EN_FTL.to_string()).unwrap_or_else(|(_, errors)| {
        // The embedded catalog is compiled into the binary; a parse failure
        // here means `humm.ftl` itself is malformed, which is a build-time
        // defect, not a runtime condition callers can recover from. Panicking
        // at first use (rather than silently serving an empty bundle, which
        // would make every `tr()` call fall back to its own key name) makes
        // the defect impossible to miss.
        panic!("humm.ftl failed to parse: {errors:?}");
    });
    let mut bundle = FluentBundle::new(vec![langid!("en-US")]);
    bundle
        .add_resource(resource)
        .expect("humm.ftl has no duplicate message ids");
    bundle
}

/// Resolves `key` to its English text, or `key` itself if the catalog has no
/// such message (the missing-key detection contract [`all_keys_used`]'s test
/// relies on: an unregistered/misspelled key is visibly wrong in the UI
/// rather than panicking).
pub fn tr(key: &str) -> String {
    tr_args(key, &[])
}

/// Like [`tr`], but resolves Fluent placeables (`{ $name }`) from `args`
/// (name, value pairs).
pub fn tr_args(key: &str, args: &[(&str, &str)]) -> String {
    BUNDLE.with(|bundle| {
        let bundle = bundle.borrow();
        let Some(message) = bundle.get_message(key) else {
            return key.to_string();
        };
        let Some(pattern) = message.value() else {
            return key.to_string();
        };
        let fluent_args = if args.is_empty() {
            None
        } else {
            let mut built = FluentArgs::new();
            for (name, value) in args {
                built.set(*name, FluentValue::from(*value));
            }
            Some(built)
        };
        let mut errors = Vec::new();
        bundle
            .format_pattern(pattern, fluent_args.as_ref(), &mut errors)
            .into_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_key_resolves_to_something_other_than_itself() {
        for key in all_keys_used() {
            let resolved = tr(key);
            assert_ne!(
                &resolved, key,
                "key {key:?} resolved to its own name — missing from humm.ftl"
            );
            assert!(!resolved.is_empty(), "key {key:?} resolved to empty text");
        }
    }

    #[test]
    fn hero_drop_matches_the_buzz_notes_wording() {
        assert_eq!(tr("hero-drop"), "Drop media here, or ⌘O");
    }

    #[test]
    fn hero_import_button_matches_the_buzz_notes_wording() {
        assert_eq!(tr("hero-import-button"), "Import files…");
    }

    #[test]
    fn url_dialog_invalid_matches_the_buzz_message_byte_exact() {
        assert_eq!(tr("url-dialog-invalid"), "The URL you entered is invalid.");
    }

    #[test]
    fn unknown_key_resolves_to_itself() {
        assert_eq!(tr("no-such-key"), "no-such-key");
    }

    #[test]
    fn recording_advanced_max_entries_label_has_the_buzz_embedded_newline() {
        // Buzz: QLabel(_("Limit export entries\n(0 = export all):")) --
        // `advanced_settings_dialog.py:242`. Fluent's own multi-line value
        // syntax (indented continuation lines) joins with `\n`.
        assert_eq!(
            tr("recording-advanced-export-max-entries-label"),
            "Limit export entries\n(0 = export all):"
        );
    }

    #[test]
    fn recording_advanced_ok_is_title_case_not_all_caps() {
        // Buzz's advanced-dialog button box explicitly relabels its Ok
        // button to "Ok" (`_setup_button_box`, `:261`) -- distinct from the
        // generic all-caps "OK" every other humm dialog uses (`dialog-ok`).
        assert_eq!(tr("recording-advanced-ok"), "Ok");
        assert_ne!(tr("recording-advanced-ok"), tr("dialog-ok"));
    }

    #[test]
    fn presentation_and_copy_strings_are_buzz_byte_exact() {
        assert_eq!(tr("presentation-show-tooltip"), "Show in new window");
        assert_eq!(
            tr("presentation-window-title"),
            "Live Transcript Presentation"
        );
        assert_eq!(tr("recording-copy-button"), "Copy");
        assert_eq!(
            tr("recording-copy-tooltip"),
            "Copy transcription to clipboard"
        );
        assert_eq!(tr("recording-copy-copied"), "Copied!");
        assert_eq!(tr("recording-copy-nothing"), "Nothing to copy!");
    }

    #[test]
    fn prefs_general_strings_are_buzz_byte_exact_with_the_product_name_swap() {
        // Buzz `general_preferences_widget.py:261-267`/`:349`: identical
        // wording, verbatim.
        assert_eq!(tr("prefs-general-invalid-key-title"), "Invalid API key");
        assert_eq!(
            tr("prefs-general-invalid-key-body"),
            "API supports only base64 characters (A-Za-z0-9+/=_-). Other characters in API key may cause errors."
        );
        // `main_window.py:330`.
        assert_eq!(
            tr("prefs-general-key-save-error-body"),
            "Unable to save OpenAI API key to keyring"
        );
        // `:248`/`:249` -- Buzz's own text names "Buzz"; humm's substitutes
        // its own product name (design spec §7.1).
        assert_eq!(tr("prefs-general-test-title"), "OpenAI API Key Test");
        assert_eq!(
            tr("prefs-general-test-success-body"),
            "Your API key is valid. humm will use this key to perform Whisper API transcriptions and AI translations."
        );
    }

    #[test]
    fn tr_args_substitutes_named_placeables() {
        // No current catalog key uses a placeable; this exercises the
        // substitution machinery directly against a key that does, so the
        // mechanism is proven before the first real user of it lands.
        thread_local! {
            static SCRATCH: RefCell<FluentBundle<FluentResource>> = RefCell::new({
                let resource = FluentResource::try_new(
                    "greeting = Hello, { $name }!".to_string(),
                )
                .expect("valid ftl");
                let mut bundle = FluentBundle::new(vec![langid!("en-US")]);
                bundle.add_resource(resource).expect("no dup ids");
                bundle
            });
        }
        let rendered = SCRATCH.with(|bundle| {
            let bundle = bundle.borrow();
            let message = bundle.get_message("greeting").expect("message exists");
            let pattern = message.value().expect("has value");
            let mut args = FluentArgs::new();
            args.set("name", FluentValue::from("humm"));
            let mut errors = Vec::new();
            bundle
                .format_pattern(pattern, Some(&args), &mut errors)
                .into_owned()
        });
        // Fluent wraps substituted values in bidi-isolation marks by default
        // (`useIsolating`), so this checks substring containment rather than
        // an exact match against those (version-sensitive) control chars.
        assert!(rendered.starts_with("Hello, "));
        assert!(rendered.contains("humm"));
        assert!(rendered.ends_with('!'));
    }
}
