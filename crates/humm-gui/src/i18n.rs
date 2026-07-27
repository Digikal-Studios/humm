//! Fluent-backed string catalog, locale registry, and negotiation (F-008a).
//!
//! Every user-visible string in `humm-gui` is a key resolved through
//! [`tr`]/[`tr_args`] rather than an inline literal — Buzz's gettext catalog
//! (`buzz/locale.py`) ported to Fluent (ADR-014). This module owns three
//! things:
//!
//! - [`AVAILABLE_LOCALES`]: the registry of embedded catalogs — `en_US`
//!   plus the `de_DE`/`es_ES`/`zh_CN` starter set (F-008b, design spec §9.2),
//!   in Buzz's own `ui_locales` order. Adding one of Buzz's remaining 11
//!   locales is one array element + one `include_str!` + one catalog file:
//!   no code changes here, and the per-locale completeness tests at the
//!   bottom of this module pick the new entry up automatically.
//! - [`negotiate`]/[`negotiate_in`]: Buzz's own gettext fallback chain
//!   (`buzz/locale.py:15-21`) reimplemented over the registry — exact code
//!   match, then language-prefix match, then `en_US`.
//! - [`set_locale`]: live, process-wide locale switching. A global
//!   generation counter plus an active-locale index; each thread's
//!   `thread_local` bundle lazily rebuilds the next time it's asked to
//!   resolve a key, comparing its own cached generation against the shared
//!   counter. This is what makes switching visible from egui's
//!   deferred-viewport render threads (each owns its own bundle/thread) as
//!   well as the main UI thread, without any thread ever reaching into
//!   another's state directly.
//!
//! [`FluentBundle`] is not `Sync` (its default memoizer uses interior
//! mutability that isn't thread-safe), so it cannot live in a plain `static`
//! shared across threads — hence the thread-local-plus-generation design
//! above, rather than a single shared bundle behind a lock.

use std::cell::RefCell;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

use fluent::{FluentArgs, FluentBundle, FluentResource, FluentValue};
use humm_core::settings::Settings;

/// One embedded UI-locale catalog: a Buzz locale code, the BCP-47 language
/// tag Fluent/`unic-langid` needs, and the catalog source itself.
///
/// Buzz's own vocabulary is the 15 codes in `ui_locales`
/// (`general_preferences_widget.py:34-50`) — humm's registry uses the same
/// codes as its wire values (parity with `settings.ui.locale`).
#[derive(Clone, Copy)]
pub struct LocaleEntry {
    /// The Buzz UI-locale code (e.g. `"en_US"`, `"de_DE"`, or a bare
    /// language code like `"nl"` for the handful of Buzz locales with no
    /// region) — the value stored in `settings.ui.locale` and passed to
    /// [`set_locale`].
    pub buzz_code: &'static str,
    /// The BCP-47 language tag `unic_langid`/Fluent resolve plural rules and
    /// bidi direction from (e.g. `"en-US"`) — not always a trivial
    /// underscore-to-hyphen rewrite of `buzz_code` (Buzz's bare `"nl"`/`"ru"`
    /// codes still need a real language tag here), so it's tracked
    /// separately rather than derived.
    pub langid: &'static str,
    /// The embedded `.ftl` catalog source (`include_str!`) for this locale.
    pub ftl: &'static str,
}

impl std::fmt::Debug for LocaleEntry {
    // A hand-written impl rather than `#[derive(Debug)]` so a failed
    // assertion in a test never dumps an entire embedded catalog's text
    // (hundreds of lines) to the test output — only its size.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocaleEntry")
            .field("buzz_code", &self.buzz_code)
            .field("langid", &self.langid)
            .field("ftl", &format_args!("<{} bytes>", self.ftl.len()))
            .finish()
    }
}

/// The embedded English Fluent catalog source.
const EN_FTL: &str = include_str!("../assets/i18n/en/humm.ftl");

/// The embedded German catalog source (F-008b).
const DE_FTL: &str = include_str!("../assets/i18n/de/humm.ftl");

/// The embedded Spanish catalog source (F-008b).
const ES_FTL: &str = include_str!("../assets/i18n/es/humm.ftl");

/// The embedded Simplified Chinese catalog source (F-008b).
const ZH_CN_FTL: &str = include_str!("../assets/i18n/zh-CN/humm.ftl");

/// The locale registry: every catalog embedded into this binary — `en_US`
/// plus the F-008b starter set, listed in Buzz's own `ui_locales` order
/// (`general_preferences_widget.py:34-50`), which is also the order the
/// Preferences language combo renders. Adding one of Buzz's remaining 11
/// locales is one array element + one `include_str!` here and one catalog
/// file: the registry is deliberately the *entire* code diff a new locale
/// needs, and this module's per-locale completeness tests iterate this array,
/// so a new entry is covered the moment it lands.
///
/// Index `0` is always `en_US` — both [`negotiate_in`]'s final fallback and
/// [`ACTIVE_LOCALE_INDEX`]'s initial value depend on that.
///
/// `langid` note: `zh_CN`'s tag is the plain `zh-CN` rather than
/// `zh-Hans-CN`. Both parse, and `unic_langid` treats the script as implicit
/// for `zh-CN`; the shorter form keeps the tag a mechanical rewrite of the
/// Buzz code, and Fluent only consults it for plural rules and bidi
/// direction, neither of which differs between the two spellings.
pub static AVAILABLE_LOCALES: &[LocaleEntry] = &[
    LocaleEntry {
        buzz_code: "en_US",
        langid: "en-US",
        ftl: EN_FTL,
    },
    LocaleEntry {
        buzz_code: "de_DE",
        langid: "de-DE",
        ftl: DE_FTL,
    },
    LocaleEntry {
        buzz_code: "es_ES",
        langid: "es-ES",
        ftl: ES_FTL,
    },
    LocaleEntry {
        buzz_code: "zh_CN",
        langid: "zh-CN",
        ftl: ZH_CN_FTL,
    },
];

/// Every catalog key `humm-gui` actually references from view code, asserted
/// resolvable by this module's unit test (the F-008 "matrix method": unit
/// catalog load). Extend this list in the same commit as any `tr("new-key")`
/// call — an unlisted key can silently render as its own name in the UI with
/// nothing catching it.
pub fn all_keys_used() -> &'static [&'static str] {
    &[
        // Locale display names (i18n::display_name, F-008a) -- one key per
        // AVAILABLE_LOCALES entry, present in *every* catalog so the language
        // combo stays localized whichever locale is active.
        "locale-name-en-us",
        "locale-name-de-de",
        "locale-name-es-es",
        "locale-name-zh-cn",
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
        // Task table — status-column phase text (format.rs, F-027, M8)
        "status-isolating-speech",
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
        "options-extract-speech-missing-tooltip",
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
        // Folder-watch pill (views/main_window.rs, F-030, design spec §3.3)
        "folderwatch-pill-watching",
        "folderwatch-pill-paused",
        "folderwatch-pill-pause-tip",
        "folderwatch-pill-resume-tip",
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
        "prefs-tab-shortcuts",
        "prefs-tab-folder-watch",
        "prefs-tab-postproc",
        "prefs-tab-privacy",
        // Preferences General tab, complete (views/prefs_general.rs, F-100,
        // design spec §8.1)
        "prefs-general-ui-language-label",
        "prefs-general-font-size-label",
        "prefs-general-font-size-custom-label",
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
        "prefs-general-export-template-label",
        "prefs-general-force-cpu-row-label",
        "prefs-general-force-cpu-label",
        "prefs-general-force-cpu-tooltip",
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
        "models-speed-badge",
        "models-speed-badge-tip",
        // Preferences Shortcuts tab (views/prefs_shortcuts.rs, F-005 editor +
        // ✚ conflict detection, design spec §8.3, Task 14)
        "shortcut-desc-open-record-window",
        "shortcut-desc-import-file",
        "shortcut-desc-import-url",
        "shortcut-desc-open-preferences-window",
        "shortcut-desc-view-transcript-text",
        "shortcut-desc-view-transcript-translation",
        "shortcut-desc-view-transcript-timestamps",
        "shortcut-desc-search-transcript",
        "shortcut-desc-search-next",
        "shortcut-desc-search-previous",
        "shortcut-desc-scroll-to-current-text",
        "shortcut-desc-play-pause-audio",
        "shortcut-desc-replay-current-segment",
        "shortcut-desc-toggle-playback-controls",
        "shortcut-desc-decrease-segment-start",
        "shortcut-desc-increase-segment-start",
        "shortcut-desc-decrease-segment-end",
        "shortcut-desc-increase-segment-end",
        "shortcut-desc-clear-history",
        "shortcut-desc-stop-transcription",
        "prefs-shortcuts-press",
        "prefs-shortcuts-clear-tooltip",
        "prefs-shortcuts-conflict",
        "prefs-shortcuts-reset",
        // Preferences Folder Watch tab (views/prefs_folder_watch.rs, F-030c,
        // design spec §8.4, Task 15) -- the embedded options form below this
        // tab's own rows reuses the options-sheet's `options-*` keys above,
        // no new keys needed for it.
        "prefs-fw-enable",
        "prefs-fw-input-folder-label",
        "prefs-fw-input-folder-placeholder",
        "prefs-fw-output-folder-label",
        "prefs-fw-output-folder-placeholder",
        "prefs-fw-browse",
        "prefs-fw-delete-checkbox",
        "prefs-fw-disposition-note",
        // Preferences Post-processing tab (views/prefs_postproc.rs, ADR-010,
        // design spec §8.5, Task 16)
        "prefs-postproc-header",
        "prefs-postproc-skip-title",
        "prefs-postproc-skip-enable",
        "prefs-postproc-skip-check-files",
        "prefs-postproc-skip-check-files-desc",
        "prefs-postproc-skip-check-db",
        "prefs-postproc-skip-check-db-desc",
        // Noise Reduction / DeepFilterNet (F-112, M8 Task 7).
        "prefs-postproc-dfn-title",
        "prefs-postproc-dfn-enable",
        "prefs-postproc-dfn-keep-file",
        // Enhanced language detection (F-114, M8 Task 8).
        "prefs-postproc-langdetect-title",
        "prefs-postproc-langdetect-enable",
        "prefs-postproc-langdetect-download-tiny",
        "prefs-postproc-langdetect-download-tiny-desc",
        "prefs-postproc-resizer-title",
        "prefs-postproc-resizer-enable",
        "prefs-postproc-resizer-merge-by-gap",
        "prefs-postproc-resizer-merge-gap-seconds",
        "prefs-postproc-resizer-split-by-punctuation",
        "prefs-postproc-resizer-punctuation",
        "prefs-postproc-resizer-split-by-max-length",
        "prefs-postproc-resizer-max-length",
        "prefs-postproc-summary-title",
        "prefs-postproc-summary-enable",
        "prefs-postproc-summary-api-url",
        "prefs-postproc-summary-api-key-label",
        "prefs-postproc-summary-model",
        "prefs-postproc-summary-prompt",
        "prefs-postproc-summary-save-to-notes",
        "prefs-postproc-summary-save-to-file",
        "prefs-postproc-summary-output-folder",
        "prefs-postproc-summary-key-save-error-body",
        "prefs-postproc-docx-title",
        "prefs-postproc-docx-enable",
        "prefs-postproc-docx-output-folder",
        "prefs-postproc-docx-include-timestamps",
        "prefs-postproc-output-folder-hint",
        // Transcript viewer window (views/viewer.rs, F-080)
        "viewer-view",
        "viewer-mode-text",
        "viewer-mode-translation",
        "viewer-mode-timestamps",
        "viewer-export",
        "viewer-translate",
        "viewer-resize",
        "viewer-identify-speakers",
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
        // Transcript viewer -- Translate (views/viewer.rs, F-085, Task 15,
        // design spec §7.3)
        "viewer-translate-key-required-title",
        "viewer-translate-key-required-body",
        "viewer-translate-settings-title",
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
        // Speaker-ID window (views/speaker_id.rs, speaker_id.rs, F-087, J-7)
        "speaker-id-step1-heading",
        "speaker-id-step2-heading",
        "speaker-id-ready-label",
        "speaker-id-missing-audio-label",
        "speaker-id-identify-button",
        "speaker-id-cancel-button",
        "speaker-id-cancelling",
        "speaker-id-decoding",
        "speaker-id-downloading-models",
        "speaker-id-identifying",
        "speaker-id-play-sample",
        "speaker-id-merge-checkbox",
        "speaker-id-save-button",
        "speaker-id-save-error",
        // Live-recording window (views/recording.rs, views/audio_meter.rs, F-060)
        "recording-window-title",
        "recording-model-load-note",
        "recording-advanced-button",
        "recording-microphone-label",
        "recording-record-button",
        "recording-stop-button",
        "recording-realtime-warning",
        "recording-chunk-skipped-warning",
        "recording-panel-placeholder",
        "recording-panel-translation-placeholder",
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

/// Bumped by [`set_locale`] on *every* call — not only when the negotiated
/// locale actually differs from whatever was previously active. Each
/// thread's cached bundle just compares its own last-seen value against
/// this counter and rebuilds whenever they differ (see the `BUNDLE`
/// `thread_local!` below), so an extra bump from a same-locale call costs at
/// most one redundant rebuild per thread that happens to call `tr` before
/// the next real switch — cheap, and never wrong — in exchange for not
/// having to compare against the previously *resolved* entry (an extra
/// atomic load) just to skip that one redundant rebuild.
static GENERATION: AtomicUsize = AtomicUsize::new(0);

/// Index into [`AVAILABLE_LOCALES`] for the currently active locale. An
/// index rather than a `RwLock<&'static LocaleEntry>`/`AtomicPtr` because
/// `AVAILABLE_LOCALES` is a fixed compile-time table that never shrinks,
/// grows, or reorders at runtime — so a plain `AtomicUsize`, written only by
/// [`set_locale`] (which only ever stores an index it just found *in*
/// `AVAILABLE_LOCALES`), is already sound: no value it can hold is ever
/// out of bounds. Starts at `0`, `AVAILABLE_LOCALES[0]`'s `en_US` — matching
/// this milestone's "always English" behavior with no `set_locale` call
/// required at all.
static ACTIVE_LOCALE_INDEX: AtomicUsize = AtomicUsize::new(0);

/// Pure locale-negotiation core: resolves `requested` against `locales`
/// using Buzz's own gettext fallback chain
/// (`buzz/locale.py:15-21` — `gettext.translation(..., languages=[...],
/// fallback=True)`, whose default expansion tries the full
/// `<lang>_<REGION>` code, then the bare `<lang>`, before giving up). In
/// order:
///
/// 1. Exact `buzz_code` match (case-sensitive — Buzz's own codes are
///    canonical, e.g. `"de_DE"`, so a stored `settings.ui.locale` value or a
///    combo selection always compares exactly).
/// 2. Language-prefix match: the first entry in `locales` whose own
///    language subtag (the part before `_`, or the whole code for a bare
///    language like Buzz's `"nl"`/`"ru"`) equals `requested`'s language
///    subtag, compared case-insensitively — env-derived requests
///    ([`locale_from_env_vars`]'s `LANG`/`LC_ALL` values) aren't guaranteed
///    canonical casing the way a stored Buzz code is. This single rule
///    covers both of Buzz's remaining cases: a bare language request
///    (`"de"`) and a regional request whose exact region isn't ported
///    (`"de_AT"` when only `"de_DE"` is embedded) both resolve by matching
///    on the shared `"de"` language subtag.
/// 3. `locales[0]` — production's `AVAILABLE_LOCALES[0]` is `en_US`, and
///    Buzz's own untranslated source strings *are* American English, so
///    this is also Buzz's effective fallback when no catalog matches.
///
/// Takes an explicit `locales` slice (rather than reading
/// [`AVAILABLE_LOCALES`] directly) so tests can exercise the
/// language-prefix/bare-language rules against a synthetic multi-locale
/// registry before Task 12 actually ports `de`/`es`/`zh` — [`negotiate`] is
/// the real entry point, and just delegates here with `AVAILABLE_LOCALES`.
///
/// `locales` must be non-empty (true of `AVAILABLE_LOCALES` always, since it
/// always carries at least `en_US`).
fn negotiate_in<'a>(locales: &'a [LocaleEntry], requested: Option<&str>) -> &'a LocaleEntry {
    let fallback = &locales[0];
    let Some(requested) = requested else {
        return fallback;
    };
    let requested = requested.trim();
    if requested.is_empty() {
        return fallback;
    }
    if let Some(entry) = locales.iter().find(|entry| entry.buzz_code == requested) {
        return entry;
    }
    let requested_lang = language_subtag(requested);
    if let Some(entry) = locales
        .iter()
        .find(|entry| language_subtag(entry.buzz_code).eq_ignore_ascii_case(requested_lang))
    {
        return entry;
    }
    fallback
}

/// The language subtag of a locale code — everything before the first `_`,
/// or the whole string for a bare language code with no region (Buzz's own
/// `ui_locales` table has entries with no region at all, e.g. `"nl"`/`"ru"`
/// — `general_preferences_widget.py:38,46`).
fn language_subtag(code: &str) -> &str {
    code.split('_').next().unwrap_or(code)
}

/// The real negotiation entry point: [`negotiate_in`] against
/// [`AVAILABLE_LOCALES`]. Called once at startup (`HummApp::new`, design
/// spec §9.1) with
/// `settings.ui.locale.as_deref().or(system_locale.as_deref())`, and will be
/// called again from the (Task 11) General-tab language combo whenever the
/// user picks a different entry.
pub fn negotiate(requested: Option<&str>) -> &'static LocaleEntry {
    negotiate_in(AVAILABLE_LOCALES, requested)
}

/// Switches the process-wide active locale to whatever `buzz_code`
/// negotiates to (never panics on an unknown code — [`negotiate`]'s own
/// `en_US` fallback absorbs it) and bumps [`GENERATION`] so every thread's
/// cached bundle rebuilds the next time it calls [`tr`]/[`tr_args`] —
/// including egui's deferred-viewport render threads, each of which owns
/// its own `BUNDLE` thread-local and only ever compares/rebuilds lazily,
/// never reaching across threads itself.
///
/// See [`GENERATION`]'s own doc for why this bumps on every call rather than
/// only when the resolved locale actually changes.
pub fn set_locale(buzz_code: &str) {
    let entry = negotiate(Some(buzz_code));
    if let Some(index) = AVAILABLE_LOCALES
        .iter()
        .position(|candidate| candidate.buzz_code == entry.buzz_code)
    {
        // Ordering note (review-corrected): this store's own ordering isn't
        // what publishes it — there are no earlier writes it needs to carry.
        // `GENERATION`'s `fetch_add(Release)` right below is the actual
        // publisher: program order alone guarantees this store happens
        // before that fetch_add in this thread's own execution, and a
        // Release store is a one-way barrier (nothing before it in program
        // order can be reordered past it), so any thread that Acquire-loads
        // the bumped `GENERATION` value is guaranteed to see this index
        // update too — even if this store were `Relaxed`. Kept as `Release`
        // anyway for symmetry with the store just below, not because
        // correctness depends on it.
        ACTIVE_LOCALE_INDEX.store(index, Ordering::Release);
    }
    // Release: pairs with `tr_args`'/`active_entry`'s Acquire loads, so a
    // thread that observes a bumped generation also observes every write
    // that happened-before this one (the `ACTIVE_LOCALE_INDEX` store above).
    GENERATION.fetch_add(1, Ordering::Release);
}

/// The currently active locale entry — [`ACTIVE_LOCALE_INDEX`] read through
/// [`AVAILABLE_LOCALES`]. The `min` clamp is defensive only: every write to
/// `ACTIVE_LOCALE_INDEX` goes through [`set_locale`], which only ever stores
/// an index it just found *in* `AVAILABLE_LOCALES`, so this can never
/// actually be out of range.
fn active_entry() -> &'static LocaleEntry {
    // Acquire: pairs with `set_locale`'s Release store, so this always sees
    // the index that store published, not a stale/torn value.
    let index = ACTIVE_LOCALE_INDEX.load(Ordering::Acquire);
    &AVAILABLE_LOCALES[index.min(AVAILABLE_LOCALES.len() - 1)]
}

/// [`active_entry`]'s `buzz_code` alone (review fix, M7 Task 11 follow-up) —
/// the narrowest way for a caller that just needs "what locale is actually
/// active right now" to get it, without re-deriving it. The Preferences
/// General tab's language-combo seed is exactly this case: seeding it via
/// `negotiate(settings.ui.locale.as_deref())` alone (the original, buggy
/// wiring) skips the system-locale fallback `HummApp::new`'s own startup
/// negotiation applies (`negotiate(settings.ui.locale.or(system_locale))`),
/// so whenever `settings.ui.locale` is `None` the combo could show a
/// *different* locale than the one [`set_locale`] actually activated at
/// startup. Reading the already-resolved ground truth here instead — the
/// same value [`set_locale`] last stored — sidesteps that re-derivation
/// entirely rather than threading the startup negotiation's result through
/// as a second, parallel code path that could drift from it.
pub fn active_locale() -> &'static str {
    active_entry().buzz_code
}

/// Whether Buzz's Lumii (Latvian whisper.cpp) model-size gate is open for
/// `settings` — ported from `models_preferences_widget.py:214-215`
/// (`model_size == LUMII and (model_type != WHISPER_CPP or self.ui_locale !=
/// "lv_LV")`) and its sibling in the shared size-combo widget
/// (`transcription_options_group_box.py:216,219`, used by both the options
/// sheet and the recording window); humm's single local backend *is* Buzz's
/// whisper.cpp group, so both collapse to the locale half alone: exactly
/// `"lv_LV"`.
///
/// Reads the **raw requested locale** — `settings.ui.locale` verbatim, or
/// (only when that's unset) `system_locale` — *never* the negotiated one
/// ([`active_locale`]). Buzz's own gate reads `settings.value(UI_LOCALE)`
/// straight off the stored setting, with no negotiation step at all
/// (`transcription_options_group_box.py:40`: `self.ui_locale =
/// self.settings.value(Settings.Key.UI_LOCALE, QLocale().name())`); humm's
/// [`negotiate`] exists only to resolve a request against the *subset* of
/// Buzz's 15 locale catalogs humm currently embeds, and with only
/// `en_US`/`de_DE`/`es_ES`/`zh_CN` embedded it can never actually resolve to
/// `lv_LV` — it falls back to `en_US`. Gating on [`active_locale`] instead
/// would therefore make this permanently unreachable even when a user
/// explicitly requests `lv_LV` — design spec §9.3 calls this out explicitly:
/// "the lv_LV Lumii gate reads the *setting*, so it works even before the lv
/// catalog is ported (parity with Buzz, whose gate also reads the setting,
/// not the translation files)".
///
/// `system_locale` is an **injected** value, never read here — the caller
/// derives it from [`locale_from_env_vars`] the same way `HummApp::new`'s
/// own startup negotiation does, so this function stays a pure comparison
/// with no real environment access to fight in a test.
pub fn lumii_locale_requested(settings: &Settings, system_locale: Option<&str>) -> bool {
    settings.ui.locale.as_deref().or(system_locale) == Some("lv_LV")
}

/// Serializes every test — in this module or any integration test — that
/// calls [`set_locale`] **or** asserts on translated text, against every
/// other one: the harness runs `#[test]` functions concurrently by default,
/// and [`set_locale`] mutates process-wide global state
/// ([`GENERATION`]/[`ACTIVE_LOCALE_INDEX`]) two tests could otherwise race
/// on — including a test that only *reads* a translation and never calls
/// `set_locale` itself, since it can still observe another test's in-flight
/// locale switch. **Not** `#[cfg(test)]`-gated — a Cargo integration test
/// (anything under `humm-gui/tests/*.rs`, e.g. `kittest_prefs.rs`'s language-
/// combo test) is compiled as its own separate crate linking against the
/// compiled library, so it can never reach a `#[cfg(test)]`-gated item of
/// this crate at all, regardless of visibility; this mirrors
/// `services::default_test_validate`'s own "production code, test-only in
/// practice" shape — acquiring this lock has no effect on a real running
/// process, since nothing outside a test ever calls it.
static LOCALE_TEST_LOCK: Mutex<()> = Mutex::new(());

/// RAII guard returned by [`lock_locale_for_test`]: holds [`LOCALE_TEST_LOCK`]
/// for its lifetime and resets the active locale back to `en_US` (index `0`)
/// on drop — so a test that switches locale can never leak a non-default
/// locale into whichever test runs next, regardless of test order or a panic
/// mid-test (`Drop` still runs during unwind).
// The guard's only job is to stay alive (holding the lock) until dropped —
// it's never read, only held, so the field itself is dead code by rustc's
// reckoning even though the type is very much "used".
#[allow(dead_code)]
pub struct LocaleTestGuard(std::sync::MutexGuard<'static, ()>);

impl Drop for LocaleTestGuard {
    fn drop(&mut self) {
        set_locale("en_US");
    }
}

/// Acquires [`LOCALE_TEST_LOCK`] for the calling test's whole body. Every
/// test — in this module or any integration test — that calls [`set_locale`]
/// **or** asserts on translated text (anything reached through [`tr`], even
/// indirectly — a test can observe another concurrently running test's
/// in-flight `set_locale` call just by reading a translation, without ever
/// switching locale itself) must hold one of these, bound to a `let _guard =
/// ...` for its full duration (a `let _ = ...` would drop the guard, and its
/// reset-to-`en_US` effect, immediately).
pub fn lock_locale_for_test() -> LocaleTestGuard {
    let guard = LOCALE_TEST_LOCK
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    LocaleTestGuard(guard)
}

/// Resolves `buzz_code`'s human-readable display name via its own
/// `locale-name-<code>` Fluent key (e.g. `"en_US"` → `locale-name-en-us`),
/// translated through the *currently active* bundle — mirrors Buzz's
/// `ui_locales` dict (`general_preferences_widget.py:34-50`), whose values
/// are themselves wrapped in `_()`, so the combo's own entries are localized
/// into whichever UI language is currently active rather than fixed to
/// English. Every catalog carries a `locale-name-*` key for *every* registry
/// entry — not just its own — which is what makes the combo fully localized;
/// `display_name_resolves_for_every_locale_under_every_locale` below pins
/// that whole matrix, so a new locale that forgets one of these keys fails
/// the suite rather than echoing a raw key name into the combo.
pub fn display_name(buzz_code: &str) -> String {
    let key = format!(
        "locale-name-{}",
        buzz_code.to_ascii_lowercase().replace('_', "-")
    );
    tr(&key)
}

/// Detects the process's env-derived locale, approximating Buzz's own
/// `QLocale().name()` lookup (`locale.py:15-16`) on a platform/toolkit that
/// has no such native call: `LC_ALL` wins over `LANG` (POSIX precedence),
/// and the codeset suffix (e.g. the `.UTF-8` in `en_US.UTF-8`) is dropped,
/// leaving a bare locale id (`en_US`). `None` when neither is set or both
/// are blank.
///
/// Shared by two callers: `services::fire_launch_telemetry`'s
/// `system_locale` telemetry field (promoted here from `services.rs` —
/// same function, same behavior, just relocated) and this module's own
/// startup locale negotiation (`HummApp::new`, design spec §9.1) as the
/// system-locale fallback used when `settings.ui.locale` is `None`.
///
/// Platform note (documented deviation, design spec §9.1): on macOS, GUI
/// apps launched from Finder/Dock don't inherit the user's shell
/// `LC_ALL`/`LANG` — those only apply to terminal-launched processes —
/// whereas Buzz's `QLocale()` reads the OS's System Settings › Language &
/// Region preference directly, regardless of how the app was launched.
/// humm has no native equivalent lookup yet, so a typical macOS GUI launch
/// sees both vars unset and this returns `None`, which [`negotiate`]'s own
/// `en_US` fallback then resolves — acceptable for now since `en_US` is the
/// only catalog embedded until Task 12 anyway.
pub fn locale_from_env_vars(lc_all: Option<&str>, lang: Option<&str>) -> Option<String> {
    parse_locale(lc_all).or_else(|| parse_locale(lang))
}

/// Parses one raw locale env value (e.g. `"en_US.UTF-8"`) down to its locale
/// id alone (`"en_US"`) by dropping everything from the first `.` onward
/// (the codeset suffix POSIX locale strings carry) — `None` for an absent or
/// blank value.
fn parse_locale(raw: Option<&str>) -> Option<String> {
    let trimmed = raw?.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.split('.').next().unwrap_or(trimmed).to_string())
}

fn build_bundle(entry: &LocaleEntry) -> FluentBundle<FluentResource> {
    let resource = FluentResource::try_new(entry.ftl.to_string()).unwrap_or_else(|(_, errors)| {
        // The embedded catalog is compiled into the binary; a parse failure
        // here means that locale's `.ftl` itself is malformed, which is a
        // build-time defect, not a runtime condition callers can recover
        // from. Panicking at first use (rather than silently serving an
        // empty bundle, which would make every `tr()` call fall back to its
        // own key name) makes the defect impossible to miss.
        panic!(
            "{} catalog ({}) failed to parse: {errors:?}",
            entry.buzz_code, entry.langid
        );
    });
    let langid: unic_langid::LanguageIdentifier = entry.langid.parse().unwrap_or_else(|err| {
        panic!(
            "{}'s langid {:?} is not a valid Unicode language identifier: {err}",
            entry.buzz_code, entry.langid
        );
    });
    let mut bundle = FluentBundle::new(vec![langid]);
    bundle.add_resource(resource).unwrap_or_else(|_| {
        panic!(
            "{} catalog ({}) has a duplicate message id",
            entry.buzz_code, entry.langid
        );
    });
    bundle
}

thread_local! {
    /// `(generation, bundle)` — the [`GENERATION`] value this thread's
    /// bundle was built at, alongside the bundle itself. [`tr_args`]
    /// compares this against the live `GENERATION` counter on every call and
    /// rebuilds from [`active_entry`] whenever they differ. This is how a
    /// [`set_locale`] call made on one thread (typically the UI thread)
    /// becomes visible on every other thread that calls `tr`/`tr_args`
    /// (egui's deferred-viewport render threads included) — each thread
    /// only ever reads its own cache and the shared counter, never another
    /// thread's bundle directly.
    static BUNDLE: RefCell<(usize, FluentBundle<FluentResource>)> = RefCell::new({
        // Acquire: pairs with `set_locale`'s Release store/fetch_add (see
        // that function's own doc) -- this thread's first-ever bundle build
        // must see whichever locale is already active, not a stale one.
        let generation = GENERATION.load(Ordering::Acquire);
        (generation, build_bundle(active_entry()))
    });
}

/// Test-only peek at the calling thread's cached bundle generation — lets
/// the `set_locale_*` tests below observe the lazy per-thread rebuild
/// directly rather than only inferring it indirectly through resolved text
/// (which, with only `en_US` ever registered, never itself changes).
#[cfg(test)]
fn thread_bundle_generation() -> usize {
    BUNDLE.with(|cell| cell.borrow().0)
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
    BUNDLE.with(|cell| {
        let mut cached = cell.borrow_mut();
        // Acquire: pairs with `set_locale`'s Release store/fetch_add, so a
        // rebuild triggered by an observed generation bump also sees the
        // matching `ACTIVE_LOCALE_INDEX` write, not a stale one.
        let current_generation = GENERATION.load(Ordering::Acquire);
        if cached.0 != current_generation {
            *cached = (current_generation, build_bundle(active_entry()));
        }
        let bundle = &cached.1;
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
    use std::collections::BTreeSet;
    use unic_langid::langid;

    fn locale(buzz_code: &'static str, langid: &'static str) -> LocaleEntry {
        LocaleEntry {
            buzz_code,
            langid,
            ftl: "",
        }
    }

    /// The message ids a `.ftl` source declares.
    ///
    /// A deliberately narrow scanner rather than a real Fluent parse:
    /// `fluent-syntax` is only a transitive dependency here, and pulling it in
    /// directly just to enumerate ids would add a dependency for a test. The
    /// scanner is sound for the catalogs this crate owns, which use only
    /// plain messages — in valid Fluent a message declaration is always at
    /// column 0 and every continuation line is indented, so "unindented,
    /// non-comment, contains `=`" identifies exactly the declarations.
    ///
    /// It is not trusted blindly: [`every_locale_has_exactly_the_en_key_set`]
    /// cross-checks its output against [`all_keys_used`] (an independently
    /// maintained list), and [`every_key_resolves_in_every_locale`] feeds
    /// every scanned key back through Fluent itself — so an under-read or a
    /// bogus id fails the suite rather than weakening it.
    fn catalog_keys(ftl: &str) -> BTreeSet<String> {
        ftl.lines()
            .filter_map(|line| message_id(line).map(str::to_string))
            .collect()
    }

    /// The message id a line declares, if it declares one — `None` for
    /// comments, blank lines, and the indented continuation lines of a
    /// multi-line value. The shared shape both catalog scanners key off.
    fn message_id(line: &str) -> Option<&str> {
        if line.starts_with('#') || line.starts_with(char::is_whitespace) {
            return None;
        }
        let (id, _) = line.split_once('=')?;
        let id = id.trim_end();
        let is_message_id = !id.is_empty()
            && id
                .chars()
                .all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-');
        is_message_id.then_some(id)
    }

    /// The message ids whose value carries at least one `{ $… }` placeable.
    /// Continuation lines are attributed to the message they belong to, so a
    /// multi-line value with a placeable on its second line still counts.
    fn catalog_keys_with_placeables(ftl: &str) -> BTreeSet<String> {
        let mut found = BTreeSet::new();
        let mut current: Option<&str> = None;
        for line in ftl.lines() {
            if line.starts_with('#') {
                continue;
            }
            if line.starts_with(char::is_whitespace) {
                if line.contains("{ $")
                    && let Some(id) = current
                {
                    found.insert(id.to_string());
                }
                continue;
            }
            match message_id(line) {
                Some(id) => {
                    if line.contains("{ $") {
                        found.insert(id.to_string());
                    }
                    current = Some(id);
                }
                None => current = None,
            }
        }
        found
    }

    #[test]
    fn every_registered_key_resolves_to_something_other_than_itself() {
        let _guard = lock_locale_for_test();
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
        let _guard = lock_locale_for_test();
        assert_eq!(tr("hero-drop"), "Drop media here, or ⌘O");
    }

    #[test]
    fn hero_import_button_matches_the_buzz_notes_wording() {
        let _guard = lock_locale_for_test();
        assert_eq!(tr("hero-import-button"), "Import files…");
    }

    #[test]
    fn url_dialog_invalid_matches_the_buzz_message_byte_exact() {
        let _guard = lock_locale_for_test();
        assert_eq!(tr("url-dialog-invalid"), "The URL you entered is invalid.");
    }

    #[test]
    fn unknown_key_resolves_to_itself() {
        let _guard = lock_locale_for_test();
        assert_eq!(tr("no-such-key"), "no-such-key");
    }

    #[test]
    fn recording_advanced_max_entries_label_has_the_buzz_embedded_newline() {
        let _guard = lock_locale_for_test();
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
        let _guard = lock_locale_for_test();
        // Buzz's advanced-dialog button box explicitly relabels its Ok
        // button to "Ok" (`_setup_button_box`, `:261`) -- distinct from the
        // generic all-caps "OK" every other humm dialog uses (`dialog-ok`).
        assert_eq!(tr("recording-advanced-ok"), "Ok");
        assert_ne!(tr("recording-advanced-ok"), tr("dialog-ok"));
    }

    #[test]
    fn presentation_and_copy_strings_are_buzz_byte_exact() {
        let _guard = lock_locale_for_test();
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
        let _guard = lock_locale_for_test();
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
    fn prefs_general_m7_strings_are_buzz_byte_exact_with_the_product_name_swap() {
        let _guard = lock_locale_for_test();
        // M7 Task 11 (F-100) additions -- `general_preferences_widget.py:91`
        // (`_("Ui Language")`), `:99` (`_("Font Size")`), `:148`
        // (`_("Default export file name")`), `:218` (checkbox text) / `:223`
        // (row label, `_("Disable GPU")`).
        assert_eq!(tr("prefs-general-ui-language-label"), "Ui Language");
        assert_eq!(tr("prefs-general-font-size-label"), "Font Size");
        assert_eq!(
            tr("prefs-general-export-template-label"),
            "Default export file name"
        );
        assert_eq!(tr("prefs-general-force-cpu-row-label"), "Disable GPU");
        assert_eq!(
            tr("prefs-general-force-cpu-label"),
            "Use only CPU and disable GPU acceleration"
        );
        // `:221` -- Buzz names "Buzz"; humm substitutes its own product name
        // (design spec §8.1, same swap as the OpenAI Test success body).
        assert_eq!(
            tr("prefs-general-force-cpu-tooltip"),
            "Set this if larger models do not fit your GPU memory and humm crashes"
        );
    }

    #[test]
    fn viewer_translate_key_required_strings_are_buzz_byte_exact() {
        let _guard = lock_locale_for_test();
        // Buzz `on_translate_button_clicked`
        // (`transcription_viewer_widget.py:1303-1306`).
        assert_eq!(
            tr("viewer-translate-key-required-title"),
            "API Key Required"
        );
        assert_eq!(
            tr("viewer-translate-key-required-body"),
            "Please enter OpenAI API Key in preferences"
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

    // -- negotiate_in / negotiate --------------------------------------------

    #[test]
    fn negotiate_exact_match() {
        let registry = [
            locale("en_US", "en-US"),
            locale("de_DE", "de-DE"),
            locale("zh_CN", "zh-CN"),
        ];
        assert_eq!(negotiate_in(&registry, Some("zh_CN")).buzz_code, "zh_CN");
    }

    #[test]
    fn negotiate_language_prefix_from_regional_request() {
        let registry = [locale("en_US", "en-US"), locale("de_DE", "de-DE")];
        // "de_AT" isn't itself registered, but its language ("de") matches
        // the registered "de_DE" -- Buzz's own fallback for an unavailable
        // regional variant (`locale.py:15-21`).
        assert_eq!(negotiate_in(&registry, Some("de_AT")).buzz_code, "de_DE");
    }

    #[test]
    fn negotiate_bare_language_request() {
        let registry = [locale("en_US", "en-US"), locale("de_DE", "de-DE")];
        assert_eq!(negotiate_in(&registry, Some("de")).buzz_code, "de_DE");
    }

    #[test]
    fn negotiate_unknown_falls_back_en() {
        let registry = [locale("en_US", "en-US"), locale("de_DE", "de-DE")];
        assert_eq!(negotiate_in(&registry, Some("xx_YY")).buzz_code, "en_US");
        assert_eq!(negotiate_in(&registry, None).buzz_code, "en_US");
    }

    #[test]
    fn negotiate_case_insensitive_language() {
        let registry = [locale("en_US", "en-US"), locale("de_DE", "de-DE")];
        assert_eq!(negotiate_in(&registry, Some("DE_at")).buzz_code, "de_DE");
        assert_eq!(negotiate_in(&registry, Some("DE")).buzz_code, "de_DE");
    }

    /// Carried from the Task 10 review: an empty (or blank-after-trim)
    /// requested locale must fall back to `en_US` exactly like `None`
    /// ([`negotiate_in`]'s own `requested.trim().is_empty()` branch), not
    /// panic or match some entry by an empty-string coincidence.
    #[test]
    fn negotiate_in_empty_string_falls_back_en() {
        let registry = [locale("en_US", "en-US"), locale("de_DE", "de-DE")];
        assert_eq!(negotiate_in(&registry, Some("")).buzz_code, "en_US");
        assert_eq!(negotiate_in(&registry, Some("   ")).buzz_code, "en_US");
    }

    // -- lumii_locale_requested -------------------------------------------------

    #[test]
    fn lumii_locale_requested_true_for_explicit_lv_lv_setting() {
        let mut settings = Settings::default();
        settings.ui.locale = Some("lv_LV".to_string());
        assert!(lumii_locale_requested(&settings, None));
    }

    #[test]
    fn lumii_locale_requested_true_via_system_locale_fallback() {
        // `settings.ui.locale` unset — Buzz's own default
        // (`self.settings.value(Settings.Key.UI_LOCALE, QLocale().name())`)
        // falls back to the system locale, which this test injects directly
        // rather than touching the real process environment.
        let settings = Settings::default();
        assert!(lumii_locale_requested(&settings, Some("lv_LV")));
    }

    #[test]
    fn lumii_locale_requested_setting_wins_over_system_locale_fallback() {
        let mut settings = Settings::default();
        settings.ui.locale = Some("en_US".to_string());
        assert!(!lumii_locale_requested(&settings, Some("lv_LV")));
    }

    #[test]
    fn lumii_locale_requested_false_when_neither_setting_nor_system_is_lv_lv() {
        let settings = Settings::default();
        assert!(!lumii_locale_requested(&settings, None));
        assert!(!lumii_locale_requested(&settings, Some("en_US")));
    }

    /// The gate's whole reason for existing (design spec §9.3): with only
    /// `en_US`/`de_DE`/`es_ES`/`zh_CN` embedded, [`negotiate`] can never
    /// resolve `"lv_LV"` to itself — proving [`active_locale`] would be the
    /// wrong thing to gate on, and [`lumii_locale_requested`]'s raw-request
    /// read is what actually makes the gate reachable pre-lv-catalog.
    #[test]
    fn negotiate_never_resolves_lv_lv_while_lumii_locale_requested_still_does() {
        assert_ne!(negotiate(Some("lv_LV")).buzz_code, "lv_LV");
        let mut settings = Settings::default();
        settings.ui.locale = Some("lv_LV".to_string());
        assert!(lumii_locale_requested(&settings, None));
    }

    // -- set_locale / generation ----------------------------------------------

    #[test]
    fn set_locale_rebuilds_bundle_on_same_thread() {
        let _guard = lock_locale_for_test();
        let before = GENERATION.load(Ordering::SeqCst);
        set_locale("en_US");
        let after_first_call = GENERATION.load(Ordering::SeqCst);
        assert!(
            after_first_call > before,
            "set_locale must bump the generation counter"
        );
        // Documented semantics (see `GENERATION`'s own doc comment): a
        // second call bumps again even though it resolves to the same
        // locale already active.
        set_locale("en_US");
        let after_second_call = GENERATION.load(Ordering::SeqCst);
        assert!(
            after_second_call > after_first_call,
            "a second set_locale call bumps the generation again, per-call, \
             not per-distinct-value"
        );
        // Same-thread lazy rebuild: this thread's cached bundle must catch
        // up to the latest generation the moment `tr` is next called.
        let _ = tr("hero-drop");
        assert_eq!(
            thread_bundle_generation(),
            after_second_call,
            "tr() must rebuild this thread's cached bundle to the latest generation"
        );
    }

    #[test]
    fn set_locale_visible_from_second_thread() {
        let _guard = lock_locale_for_test();
        let (ready_tx, ready_rx) = std::sync::mpsc::channel::<()>();
        let (proceed_tx, proceed_rx) = std::sync::mpsc::channel::<()>();
        let (result_tx, result_rx) = std::sync::mpsc::channel::<(String, usize, usize)>();

        let worker = std::thread::spawn(move || {
            // Touch `tr` first, priming this thread's own bundle/generation
            // before the main thread ever calls `set_locale`.
            let _ = tr("hero-drop");
            let before_generation = thread_bundle_generation();
            ready_tx
                .send(())
                .expect("main thread is waiting to receive ready");
            proceed_rx
                .recv()
                .expect("main thread sends proceed after its own set_locale call");
            let after_text = tr("hero-drop");
            let after_generation = thread_bundle_generation();
            result_tx
                .send((after_text, before_generation, after_generation))
                .expect("main thread is waiting to receive the result");
        });

        ready_rx
            .recv()
            .expect("worker thread signals ready before any set_locale call");
        let global_before = GENERATION.load(Ordering::SeqCst);
        // Switch to a *different* catalog, not back to `en_US`: with the
        // F-008b locales registered, the worker's post-switch `tr` result is
        // now a content-level proof of cross-thread visibility, not just
        // generation bookkeeping (see the closing assertion).
        set_locale("de_DE");
        let global_after = GENERATION.load(Ordering::SeqCst);
        proceed_tx
            .send(())
            .expect("worker thread is waiting to proceed");
        worker.join().expect("worker thread panicked");
        let (after_text, before_generation, after_generation) =
            result_rx.recv().expect("worker thread sent its result");

        assert!(global_after > global_before);
        // The second thread's own cached generation must have moved past
        // what it was primed with, proving the generation bump `set_locale`
        // made on the *main* thread became visible on the *worker* thread
        // purely through that thread's own next `tr` call -- no direct
        // cross-thread bundle access ever happens, which is exactly the
        // deferred-viewport render-thread contract this module must uphold.
        assert!(
            after_generation > before_generation,
            "the worker thread's cached generation must advance after the \
             main thread's set_locale call"
        );
        assert!(after_generation >= global_after);
        // The payoff of switching to a real second catalog above: the worker
        // thread resolved the *German* string, from a bundle it rebuilt
        // itself, after a `set_locale` call made on another thread entirely.
        // That is the deferred-viewport render-thread contract proven on
        // content, not just on the generation counter.
        assert_eq!(after_text, "Medien hier ablegen oder ⌘O");
    }

    // -- per-locale catalog completeness (F-008b) -------------------------------

    /// Global Constraint 5's enforcement: every embedded catalog declares
    /// *exactly* the `en` key set — both directions, so neither a forgotten
    /// key nor a stale one left behind after an `en` rename can ship.
    #[test]
    fn every_locale_has_exactly_the_en_key_set() {
        let en_keys = catalog_keys(EN_FTL);
        // Anchor `catalog_keys` against an independently maintained list: if
        // the scanner under-read the source, this fails here rather than
        // silently letting every locale "agree" on a truncated key set.
        for key in all_keys_used() {
            assert!(
                en_keys.contains(*key),
                "all_keys_used() lists {key:?}, but the en catalog does not declare it"
            );
        }
        for entry in AVAILABLE_LOCALES {
            let keys = catalog_keys(entry.ftl);
            let missing: Vec<_> = en_keys.difference(&keys).collect();
            let extra: Vec<_> = keys.difference(&en_keys).collect();
            assert!(
                missing.is_empty() && extra.is_empty(),
                "{} catalog's key set differs from en — missing: {missing:?}, extra: {extra:?}",
                entry.buzz_code
            );
        }
    }

    /// Every key resolves to real text in every locale — never an empty
    /// value, never the key echoing back as its own name (`tr`'s missing-key
    /// fallback). Run through the live bundle, so this also proves each
    /// catalog parses and each `langid` is a valid language identifier
    /// (`build_bundle` panics otherwise).
    #[test]
    fn every_key_resolves_in_every_locale() {
        let _guard = lock_locale_for_test();
        let en_keys = catalog_keys(EN_FTL);
        for entry in AVAILABLE_LOCALES {
            set_locale(entry.buzz_code);
            for key in &en_keys {
                let resolved = tr(key);
                assert_ne!(
                    &resolved, key,
                    "{}: key {key:?} resolved to its own name — missing from that catalog",
                    entry.buzz_code
                );
                assert!(
                    !resolved.trim().is_empty(),
                    "{}: key {key:?} resolved to empty text",
                    entry.buzz_code
                );
            }
        }
    }

    /// Placeables survive translation in every locale: each argument's value
    /// lands in the rendered string, and no unsubstituted `{ $… }` syntax is
    /// left behind — which is exactly what a mistyped or dropped placeable
    /// name in a translated value looks like at runtime, since Fluent
    /// resolves an unknown argument to the literal placeable text rather than
    /// failing.
    ///
    /// Table-driven over *every* placeable-bearing key, cross-checked against
    /// the catalog so the table cannot silently fall behind it, and run under
    /// every locale in [`AVAILABLE_LOCALES`].
    #[test]
    fn every_locale_substitutes_placeables() {
        let _guard = lock_locale_for_test();
        // Argument values are deliberately distinctive: a `contains` check
        // against them can't pass by coincidence on surrounding prose.
        let cases: &[(&str, &[(&str, &str)])] = &[
            ("folderwatch-pill-watching", &[("name", "my-clips")]),
            ("viewer-find-prev-tooltip", &[("shortcut", "Shift+Cmd+G")]),
            ("viewer-find-next-tooltip", &[("shortcut", "Cmd+G")]),
            ("export-written", &[("path", "/tmp/humm-out.srt")]),
            // Two arguments in one value, one adjacent to a literal `×` — the
            // hardest placeable in the catalog to reposition correctly when
            // the sentence is reordered for another language.
            (
                "recording-realtime-warning",
                &[("size", "Large"), ("ratio", "0.4")],
            ),
            (
                "recording-chunk-skipped-warning",
                &[("detail", "inference failed: boom")],
            ),
            ("recording-model-downloading", &[("percent", "42")]),
            ("models-speed-badge", &[("ratio", "2.5")]),
            ("prefs-shortcuts-conflict", &[("other", "Import File")]),
        ];

        // Drift guard: a new `tr_args` key added to the catalog without a row
        // here would otherwise ship with its placeables untested in every
        // locale. Compared against `en`, which the key-set test already pins
        // as the shape every other catalog shares.
        let covered: BTreeSet<String> = cases.iter().map(|(key, _)| (*key).to_string()).collect();
        assert_eq!(
            covered,
            catalog_keys_with_placeables(EN_FTL),
            "the placeable test table has drifted from the en catalog — add a \
             row for each newly placeable-bearing key"
        );

        for entry in AVAILABLE_LOCALES {
            set_locale(entry.buzz_code);
            for (key, args) in cases {
                let rendered = tr_args(key, args);
                for (name, value) in *args {
                    assert!(
                        rendered.contains(value),
                        "{}: {key} dropped argument {name:?} ({value:?}) — got {rendered:?}",
                        entry.buzz_code
                    );
                }
                assert!(
                    !rendered.contains("{$") && !rendered.contains("{ $"),
                    "{}: {key} left an unsubstituted placeable — got {rendered:?}",
                    entry.buzz_code
                );
            }
        }
    }

    // -- display_name -----------------------------------------------------------

    #[test]
    fn display_name_en() {
        let _guard = lock_locale_for_test();
        assert_eq!(display_name("en_US"), "English");
    }

    /// The whole language-combo matrix: under every active locale, every
    /// registered locale's display name resolves — Buzz's `ui_locales` values
    /// are themselves `_()`-wrapped, so each catalog names *all* the
    /// languages, not just its own.
    #[test]
    fn display_name_resolves_for_every_locale_under_every_locale() {
        let _guard = lock_locale_for_test();
        for active in AVAILABLE_LOCALES {
            set_locale(active.buzz_code);
            for named in AVAILABLE_LOCALES {
                let name = display_name(named.buzz_code);
                assert!(
                    !name.starts_with("locale-name-"),
                    "under {}, display_name({:?}) echoed its own key — got {name:?}",
                    active.buzz_code,
                    named.buzz_code
                );
                assert!(
                    !name.trim().is_empty(),
                    "under {}, display_name({:?}) resolved to empty text",
                    active.buzz_code,
                    named.buzz_code
                );
            }
        }
    }

    /// Spot-check that switching locale actually changes rendered text, in
    /// each embedded catalog, on a key with no shared proper nouns to hide
    /// behind — the end-to-end sanity the completeness tests above deliberately
    /// do not assert (they check shape, not content).
    #[test]
    fn set_locale_switches_rendered_text_per_catalog() {
        let _guard = lock_locale_for_test();
        for (code, expected) in [
            ("en_US", "Preferences"),
            ("de_DE", "Einstellungen"),
            ("es_ES", "Preferencias"),
            ("zh_CN", "偏好设置"),
        ] {
            set_locale(code);
            assert_eq!(tr("prefs-title"), expected, "prefs-title under {code}");
        }
    }
}
