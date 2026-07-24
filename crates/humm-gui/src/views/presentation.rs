//! The live-presentation window (F-065, spec §2.6): the recording window's
//! "Presentation options bar" (embedded, [`bar_ui`]) plus the separate,
//! big-type presentation viewport's own content ([`window_ui`]) — a
//! deferred `egui` viewport in `app.rs`'s `show_presentation_viewport`,
//! mirroring the recording/viewer/resizer windows' own shape.
//!
//! Ported from Buzz `buzz/widgets/recording_transcriber_widget.py:267-482`
//! (the bar: `create_presentation_options_bar`, `on_show_presentation_clicked`,
//! `on_text_size_changed`, `on_theme_changed`, `on_text/bg_color_clicked`,
//! `on_fullscreen_clicked`) and `buzz/widgets/presentation_window.py` (the
//! window: size/title/theme colors, `apply_styling`'s CSS-file override,
//! `keyPressEvent`'s Esc-exits-fullscreen).
//!
//! **Mechanism deviations (spec §2.6, documented):**
//! - Buzz's `QColorDialog` becomes egui's inline `color_edit_button_srgb`
//!   popup — same capability (pick an RGB color, persist as hex), different
//!   widget.
//! - Buzz renders the transcript via `QTextBrowser::setHtml` (a tiny
//!   generated `<style>` block or a user CSS **file**, both HTML/CSS); egui
//!   has no HTML/CSS renderer, so [`window_ui`] paints the text directly
//!   with `egui::RichText` color/size, and [`load_css_override`]/
//!   [`parse_presentation_css`] implement a tiny, tolerant **subset** parser
//!   for the three properties Buzz's own generated stylesheet actually uses
//!   (`color`, `background-color`, `font-size`) rather than embedding an
//!   HTML/CSS engine. The override file's name is `presentation_style.css`
//!   at humm's config dir (`HUMM_SURFACE.md` §6) — Buzz's own
//!   `presentation_window_style.css` at the OS cache dir; humm's config dir
//!   is already the one settings-adjacent, user-discoverable directory this
//!   port uses for every other per-user file, so the override lives there
//!   instead (documented ledger entry, not a hidden divergence).
//! - Buzz's `toggle_fullscreen`/`isFullScreen` become
//!   `ViewportCommand::Fullscreen`/`ctx.input(|i| i.viewport().fullscreen)` —
//!   `app.rs`'s `show_presentation_viewport` owns those calls (they must run
//!   from inside the *presentation* viewport's own deferred-render closure,
//!   not the recording window's), so this module only exposes the click
//!   intent ([`PresentationVm::fullscreen_toggle_requested`]) and the pure
//!   style/CSS logic below.

use std::path::Path;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use humm_core::settings::{PresentationSettings, Settings};

use crate::i18n::tr;

// -- Theme + color model -----------------------------------------------------

/// The presentation window's theme (Buzz `theme_combo`'s three items,
/// `recording_transcriber_widget.py:302-312`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PresentationTheme {
    /// Black text on white — the default.
    Light,
    /// White text on black.
    Dark,
    /// User-chosen [`PresentationVm::text_color`]/[`PresentationVm::background_color`].
    Custom,
}

impl PresentationTheme {
    /// Parses a persisted theme string, falling back to [`Self::Light`] for
    /// anything unrecognized — Buzz's own fallback
    /// (`{"light": 0, "dark": 1, "custom": 2}.get(saved_theme, 0)`,
    /// `:309` / `presentation_window.py:64-79`'s own `if/elif/else` chain,
    /// which also lands on light for anything that isn't exactly `"dark"`).
    pub fn parse_setting(s: &str) -> Self {
        match s {
            "dark" => PresentationTheme::Dark,
            "custom" => PresentationTheme::Custom,
            _ => PresentationTheme::Light,
        }
    }

    /// The string this theme persists as (inverse of [`Self::parse_setting`]).
    pub fn as_str(self) -> &'static str {
        match self {
            PresentationTheme::Light => "light",
            PresentationTheme::Dark => "dark",
            PresentationTheme::Custom => "custom",
        }
    }
}

/// Validates `s` as a `#RRGGBB` hex color (case-insensitive, optional
/// surrounding whitespace), returning it normalized to lowercase. `None` for
/// anything else — 3-digit hex, named CSS colors, `rgb()`, and other CSS
/// color syntaxes are out of scope (documented; Buzz itself only ever
/// produces/consumes `QColor::name()`'s own `#rrggbb` form here).
pub fn valid_hex_color(s: &str) -> Option<String> {
    let trimmed = s.trim();
    let hex = trimmed.strip_prefix('#')?;
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(format!("#{}", hex.to_ascii_lowercase()))
}

/// Parses a validated `#RRGGBB` string into RGB bytes. `None` for anything
/// [`valid_hex_color`] would also reject.
pub fn parse_hex_color(s: &str) -> Option<[u8; 3]> {
    let hex = valid_hex_color(s)?;
    let bytes = hex.as_bytes();
    let r = u8::from_str_radix(std::str::from_utf8(&bytes[1..3]).ok()?, 16).ok()?;
    let g = u8::from_str_radix(std::str::from_utf8(&bytes[3..5]).ok()?, 16).ok()?;
    let b = u8::from_str_radix(std::str::from_utf8(&bytes[5..7]).ok()?, 16).ok()?;
    Some([r, g, b])
}

/// Formats RGB bytes as a lowercase `#rrggbb` string (Buzz `QColor::name()`'s
/// own lowercase convention).
pub fn hex_from_rgb([r, g, b]: [u8; 3]) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

/// Resolves `theme` (+ the stored custom colors) to the `(text, background)`
/// hex pair the window paints (Buzz `load_settings`,
/// `presentation_window.py:49-81`). Light/Dark are fixed; Custom uses the
/// stored hex strings if valid, else falls back to
/// [`PresentationSettings::DEFAULT_TEXT_COLOR`]/`DEFAULT_BACKGROUND_COLOR` —
/// a humm ✚ improvement over Buzz, which has no fallback and would simply
/// hand an unparseable string straight to Qt's CSS engine (silently
/// rendering default black-on-white in practice, since Qt ignores an invalid
/// `color:`/`background-color:` declaration) — validating explicitly here
/// keeps the *reason* visible instead of an implicit browser-engine quirk.
pub fn resolve_theme_colors(
    theme: PresentationTheme,
    text_color: &str,
    background_color: &str,
) -> (String, String) {
    match theme {
        PresentationTheme::Light => ("#000000".to_string(), "#ffffff".to_string()),
        PresentationTheme::Dark => ("#ffffff".to_string(), "#000000".to_string()),
        PresentationTheme::Custom => (
            valid_hex_color(text_color)
                .unwrap_or_else(|| PresentationSettings::DEFAULT_TEXT_COLOR.to_string()),
            valid_hex_color(background_color)
                .unwrap_or_else(|| PresentationSettings::DEFAULT_BACKGROUND_COLOR.to_string()),
        ),
    }
}

// -- CSS-subset override (spec §2.6, HUMM_SURFACE §6) ------------------------

/// The subset of a `presentation_style.css` file this port understands:
/// `color`, `background-color`, `font-size` (a trailing `pt` unit, matching
/// Buzz's own generated stylesheet, `presentation_window.py:102-111`).
/// Every field is independently optional — a file overriding only one
/// property leaves the theme's own value for the other two.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CssOverride {
    /// `color: #RRGGBB;` — text color override.
    pub color: Option<String>,
    /// `background-color: #RRGGBB;` — background color override.
    pub background_color: Option<String>,
    /// `font-size: NNpt;` — font size override, in points.
    pub font_size_pt: Option<u32>,
}

/// Parses `css` for `color`/`background-color`/`font-size` declarations,
/// tolerantly: `{`/`}` are treated as declaration separators (so a `body {
/// ... }` block parses the same as a bare declaration list), each `;`-
/// separated segment is split on its first `:`, and any segment that isn't a
/// recognized property (or whose value doesn't parse — an invalid hex color,
/// a `font-size` with no `pt` suffix) is silently skipped and logged once as
/// ignored — Buzz's own default stylesheet's `font-family`/`padding`/`margin`
/// declarations all fall into this bucket by design.
///
/// Returns `None` ("malformed" — spec: "parse errors → theme unchanged") when
/// **nothing** recognized was found at all (no `color`/`background-color`/
/// `font-size` declaration present, valid or not) — e.g. a file containing
/// only unrelated CSS, or no colon-separated declarations whatsoever. Returns
/// `Some` (possibly with only one or two of the three fields set) as soon as
/// at least one of the three properties parsed successfully.
pub fn parse_presentation_css(css: &str) -> Option<CssOverride> {
    let normalized = css.replace(['{', '}'], ";");
    let mut result = CssOverride::default();
    let mut recognized_any = false;
    let mut ignored_any = false;

    for segment in normalized.split(';') {
        let segment = segment.trim();
        if segment.is_empty() {
            continue;
        }
        let Some((prop, value)) = segment.split_once(':') else {
            // A bare selector token (e.g. "body") -- not a declaration at
            // all, not counted as junk.
            continue;
        };
        let prop = prop.trim().to_ascii_lowercase();
        let value = value.trim();
        match prop.as_str() {
            "color" => match valid_hex_color(value) {
                Some(hex) => {
                    result.color = Some(hex);
                    recognized_any = true;
                }
                None => ignored_any = true,
            },
            "background-color" => match valid_hex_color(value) {
                Some(hex) => {
                    result.background_color = Some(hex);
                    recognized_any = true;
                }
                None => ignored_any = true,
            },
            "font-size" => match parse_pt_size(value) {
                Some(pt) => {
                    result.font_size_pt = Some(pt);
                    recognized_any = true;
                }
                None => ignored_any = true,
            },
            _ => ignored_any = true,
        }
    }

    if ignored_any {
        log::warn!("presentation_style.css: ignoring unrecognized or invalid declaration(s)");
    }
    if recognized_any { Some(result) } else { None }
}

/// Parses a `font-size` value of the form `<number>pt` (case-insensitive
/// suffix), rounding to the nearest point. `None` for anything else —
/// missing/wrong unit, or a non-numeric magnitude.
fn parse_pt_size(value: &str) -> Option<u32> {
    let trimmed = value.trim();
    if trimmed.len() < 3 {
        return None;
    }
    let split_at = trimmed.len() - 2;
    if !trimmed.is_char_boundary(split_at) {
        return None;
    }
    let (digits, suffix) = trimmed.split_at(split_at);
    if !suffix.eq_ignore_ascii_case("pt") {
        return None;
    }
    let magnitude: f64 = digits.trim().parse().ok()?;
    if !magnitude.is_finite() || magnitude < 0.0 {
        return None;
    }
    Some(magnitude.round() as u32)
}

/// Reads and parses `<config_dir>/presentation_style.css` (module doc's
/// filename deviation). A missing file is silent (`None`, no log — Buzz's own
/// `os.path.exists` guard); an unreadable file logs a warning and returns
/// `None` (Buzz's `except Exception: log.warning`, `presentation_window.py:99-100`);
/// a readable file is handed to [`parse_presentation_css`].
pub fn load_css_override() -> Option<CssOverride> {
    let path = humm_core::paths::config_dir().join("presentation_style.css");
    if !path.exists() {
        return None;
    }
    match std::fs::read_to_string(&path) {
        Ok(content) => parse_presentation_css(&content),
        Err(err) => {
            log::warn!("could not read presentation_style.css: {err}");
            None
        }
    }
}

/// The fully-resolved style the presentation window paints with this frame:
/// [`resolve_theme_colors`] overridden field-by-field by `css_override`, if
/// any (spec: "apply them over the theme").
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveStyle {
    /// Text color, `#rrggbb`.
    pub text_color: String,
    /// Background color, `#rrggbb`.
    pub background_color: String,
    /// Font size in points.
    pub font_size_pt: u32,
}

/// Combines `vm`'s theme/size with an optional [`CssOverride`] into the
/// [`EffectiveStyle`] [`window_ui`] paints.
pub fn effective_style(vm: &PresentationVm, css_override: Option<&CssOverride>) -> EffectiveStyle {
    let (mut text_color, mut background_color) =
        resolve_theme_colors(vm.theme, &vm.text_color, &vm.background_color);
    let mut font_size_pt = vm.text_size;
    if let Some(css) = css_override {
        if let Some(color) = &css.color {
            text_color = color.clone();
        }
        if let Some(color) = &css.background_color {
            background_color = color.clone();
        }
        if let Some(pt) = css.font_size_pt {
            font_size_pt = pt;
        }
    }
    EffectiveStyle {
        text_color,
        background_color,
        font_size_pt,
    }
}

// -- View-model ---------------------------------------------------------------

/// The presentation window's per-recording-session state (owned by
/// [`crate::recording::RecordingVm::presentation`]): whether the window is
/// open, one-shot focus/fullscreen-toggle intents for `app.rs`'s deferred
/// viewport closure to consume, and the persisted display preferences
/// (Buzz: `PresentationWindow.__init__`/`load_settings`, read once at
/// recording-window-open time here rather than at first "Show" click — see
/// [`Self::from_settings`]'s own doc).
#[derive(Debug, Clone)]
pub struct PresentationVm {
    /// Whether the presentation viewport should currently be shown.
    pub open: bool,
    /// Set by a "Show" click while already open (Buzz `raise_`/`activateWindow`,
    /// `:412-415`); consumed by `app.rs`'s viewport closure.
    pub focus_requested: bool,
    /// Set by a Fullscreen click; consumed by `app.rs`'s viewport closure,
    /// which alone can read/toggle *that* viewport's own fullscreen state
    /// (module doc's mechanism-deviation note).
    pub fullscreen_toggle_requested: bool,
    /// Font size in points, range [`PresentationSettings::MIN_TEXT_SIZE`]..=
    /// [`PresentationSettings::MAX_TEXT_SIZE`].
    pub text_size: u32,
    /// The selected theme.
    pub theme: PresentationTheme,
    /// The stored custom text color (meaningful only under
    /// [`PresentationTheme::Custom`]).
    pub text_color: String,
    /// The stored custom background color (ditto).
    pub background_color: String,
    /// When any of `text_size`/`text_color`/`background_color` was last
    /// edited, for the shared ~100 ms debounced save
    /// ([`drive_pending_save_debounce`]); `None` while no save is pending.
    /// One shared timestamp for all three (rather than one per field) is
    /// deliberate — `color_edit_button_srgb`'s popup fires `.changed()`
    /// continuously while a slider inside it is being dragged, so without
    /// debouncing a single drag would flood dozens of synchronous disk
    /// writes on the UI thread; a shared timer collapses any burst of edits
    /// across all three fields into one write, same as Buzz's own
    /// `QTimer.singleShot`-per-field debounce converges to one final value.
    pub pending_save_last_edit: Option<Instant>,
    /// Cached result of the last [`load_css_override`] call — read every
    /// frame by `app.rs::show_presentation_viewport`, but only
    /// *(re)computed* when [`Self::css_dirty`] is set (see that field's own
    /// doc for the trigger set). Avoids an `exists`+`read`+`parse` disk
    /// round-trip on every presentation-viewport frame (~30/s while
    /// recording).
    pub css_override: Option<CssOverride>,
    /// Whether [`Self::css_override`] needs recomputing before this frame's
    /// render — the Buzz-equivalent triggers for `PresentationWindow.load_settings()`
    /// (which re-reads the CSS file as a side effect of `apply_styling`,
    /// `presentation_window.py:81`): the window opening for the first time
    /// ([`Self::from_settings`]'s own initial `true`, and `bar_ui`'s Show-click
    /// open transition), a theme change, and the debounced text-size/color
    /// save firing ([`drive_pending_save_debounce`]) — i.e. every point that
    /// persists a `settings.presentation` field. `app.rs`'s viewport closure
    /// consumes this (reload + clear) once per dirty frame; every other
    /// frame reuses [`Self::css_override`] unchanged.
    pub css_dirty: bool,
}

impl PresentationVm {
    /// Builds the initial state from `ps` (`settings.presentation`), read
    /// once when the recording window opens (`RecordingVm::open`) — Buzz
    /// reads the same settings once, in `RecordingTranscriberWidget.__init__`
    /// (`:80-…`, the bar) and again in `PresentationWindow.__init__` (only
    /// once the window is actually shown) — humm folds both into this one
    /// snapshot since the bar and the window always agree on these values by
    /// construction (`bar_ui` is the only writer, and it always persists
    /// through to `settings.presentation` on every change). `css_dirty`
    /// starts `true` so the presentation viewport's first render loads the
    /// CSS override at least once, matching Buzz's own first-`load_settings`
    /// call in `PresentationWindow.__init__`.
    pub fn from_settings(ps: &PresentationSettings) -> Self {
        PresentationVm {
            open: false,
            focus_requested: false,
            fullscreen_toggle_requested: false,
            text_size: ps.text_size,
            theme: PresentationTheme::parse_setting(&ps.theme),
            text_color: ps.text_color.clone(),
            background_color: ps.background_color.clone(),
            pending_save_last_edit: None,
            css_override: None,
            css_dirty: true,
        }
    }

    /// Unconditionally persists any pending debounced text-size/color edit
    /// into `settings` and clears the pending timer — the flush every place
    /// that's about to stop rendering [`bar_ui`] (and so stop ever driving
    /// [`drive_pending_save_debounce`] again) must call first, or the edit
    /// is silently lost (review fix): `RecordingVm::finish_stopping` (the
    /// bar hides with `presentation_bar_visible`) and
    /// `app.rs::harvest_recording_close` (the whole window, bar included,
    /// closes). Mirrors [`drive_pending_save_debounce`]'s own three-field
    /// write, minus the debounce wait; also sets [`Self::css_dirty`], the
    /// same as a normal debounce fire, so a presentation window that stays
    /// open across the flush (`finish_stopping`'s case — only the *bar*
    /// disappears, not necessarily the window) picks up the change on its
    /// next render. Returns `true` iff something was actually pending (and
    /// therefore `settings` was mutated) — the caller decides whether that
    /// warrants a `Settings::save` call; a no-op call must not force one.
    pub fn flush_pending_save(&mut self, settings: &mut PresentationSettings) -> bool {
        if self.pending_save_last_edit.take().is_none() {
            return false;
        }
        settings.text_size = self.text_size;
        settings.text_color = self.text_color.clone();
        settings.background_color = self.background_color.clone();
        self.css_dirty = true;
        true
    }
}

/// Locks `settings`, applies `f` to `settings.presentation`, and saves —
/// [`crate::views::recording_advanced`]'s `persist_recording`, scoped to the
/// presentation section instead.
fn persist_presentation(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    f: impl FnOnce(&mut PresentationSettings),
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.presentation);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist presentation settings: {err}");
    }
}

fn theme_label(theme: PresentationTheme) -> String {
    match theme {
        PresentationTheme::Light => tr("presentation-theme-light"),
        PresentationTheme::Dark => tr("presentation-theme-dark"),
        PresentationTheme::Custom => tr("presentation-theme-custom"),
    }
}

/// The shared save-debounce window for text size and both custom colors
/// (Buzz `QTimer.singleShot(100, ...)`, text size only, `:417-425` — colors
/// save on `QColorDialog` OK with no debounce at all in Buzz, since a modal
/// color-picker dialog can't fire continuous `.changed()` events the way
/// egui's inline `color_edit_button_srgb` popup does; debouncing colors too
/// is this port's own necessary addition, review finding). A real
/// single-fire debounce (only the *last* edit within the window is saved),
/// rather than Buzz's own fire-every-timer behavior for text size, which
/// converges to the same final persisted value with fewer writes
/// (documented — not a behavior bug, an equivalent, more idiomatic port of
/// "debounced").
const PENDING_SAVE_DEBOUNCE_MS: u64 = 100;

/// Whether `elapsed` since the last text-size/color edit has crossed the
/// debounce window — the pure decision [`drive_pending_save_debounce`]
/// wraps with a real clock (mirrors `viewer.rs`'s `debounce_decision`).
pub fn pending_save_debounce_ready(elapsed: Duration) -> bool {
    elapsed >= Duration::from_millis(PENDING_SAVE_DEBOUNCE_MS)
}

/// Checks the pending text-size/color debounce every frame the bar renders:
/// reschedules a repaint while waiting, and once the window has elapsed,
/// persists `text_size`/`text_color`/`background_color` together in one
/// write (whichever changed — writing all three unconditionally is harmless
/// idempotent overhead, and avoids tracking per-field dirtiness for what is,
/// in practice, always a single in-flight edit) and marks
/// [`PresentationVm::css_dirty`] (a settings save is one of the CSS-reload
/// triggers that field's own doc lists).
fn drive_pending_save_debounce(
    ui: &egui::Ui,
    vm: &mut PresentationVm,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    let Some(last_edit) = vm.pending_save_last_edit else {
        return;
    };
    let elapsed = Instant::now().saturating_duration_since(last_edit);
    if !pending_save_debounce_ready(elapsed) {
        let remaining = Duration::from_millis(PENDING_SAVE_DEBOUNCE_MS).saturating_sub(elapsed);
        ui.ctx().request_repaint_after(remaining);
        return;
    }
    vm.pending_save_last_edit = None;
    let text_size = vm.text_size;
    let text_color = vm.text_color.clone();
    let background_color = vm.background_color.clone();
    persist_presentation(settings, settings_path, |p| {
        p.text_size = text_size;
        p.text_color = text_color;
        p.background_color = background_color;
    });
    vm.css_dirty = true;
}

// -- Render: the recording window's embedded bar -----------------------------

/// Renders the presentation options bar (Buzz `create_presentation_options_bar`,
/// `:267-341`) — visible only while recording (caller's own
/// `vm.presentation_bar_visible` gate, `views/recording.rs`).
pub fn bar_ui(
    ui: &mut egui::Ui,
    vm: &mut PresentationVm,
    settings: &Mutex<Settings>,
    settings_path: &Path,
) {
    ui.horizontal_wrapped(|ui| {
        let show_clicked = ui
            .button(tr("presentation-show-button"))
            .on_hover_text(tr("presentation-show-tooltip"))
            .clicked();
        if show_clicked {
            if vm.open {
                vm.focus_requested = true;
            } else {
                vm.open = true;
                // Window-opening trigger for the CSS-reload cache (field doc).
                vm.css_dirty = true;
            }
        }

        ui.label(tr("presentation-text-size-label"));
        let mut size = vm.text_size;
        if ui
            .add(
                egui::DragValue::new(&mut size).range(
                    PresentationSettings::MIN_TEXT_SIZE..=PresentationSettings::MAX_TEXT_SIZE,
                ),
            )
            .changed()
        {
            vm.text_size = size;
            vm.pending_save_last_edit = Some(Instant::now());
        }

        ui.label(tr("presentation-theme-label"));
        let mut theme = vm.theme;
        egui::ComboBox::from_id_salt("humm-presentation-theme")
            .selected_text(theme_label(theme))
            .show_ui(ui, |ui| {
                for candidate in [
                    PresentationTheme::Light,
                    PresentationTheme::Dark,
                    PresentationTheme::Custom,
                ] {
                    ui.selectable_value(&mut theme, candidate, theme_label(candidate));
                }
            });
        if theme != vm.theme {
            vm.theme = theme;
            let theme_str = theme.as_str().to_string();
            persist_presentation(settings, settings_path, |p| p.theme = theme_str);
            // Theme-change trigger for the CSS-reload cache (field doc) —
            // matches Buzz's own `load_settings()` call in `on_theme_changed`.
            vm.css_dirty = true;
        }

        if vm.theme == PresentationTheme::Custom {
            // Both color pickers only *stage* the edit (update the live VM
            // field + arm the shared debounce timer) rather than persisting
            // synchronously — `color_edit_button_srgb`'s popup fires
            // `.changed()` continuously while its internal slider is
            // dragged, so a direct `persist_presentation` call here would
            // flood the UI thread with disk writes for the duration of one
            // drag (review finding). `drive_pending_save_debounce` below
            // performs the actual, debounced write.
            ui.label(tr("presentation-text-color-label"));
            let mut rgb = parse_hex_color(&vm.text_color).unwrap_or([0, 0, 0]);
            if ui
                .color_edit_button_srgb(&mut rgb)
                .on_hover_text(tr("presentation-text-color-label"))
                .changed()
            {
                vm.text_color = hex_from_rgb(rgb);
                vm.pending_save_last_edit = Some(Instant::now());
            }

            ui.label(tr("presentation-background-color-label"));
            let mut bg_rgb = parse_hex_color(&vm.background_color).unwrap_or([255, 255, 255]);
            if ui
                .color_edit_button_srgb(&mut bg_rgb)
                .on_hover_text(tr("presentation-background-color-label"))
                .changed()
            {
                vm.background_color = hex_from_rgb(bg_rgb);
                vm.pending_save_last_edit = Some(Instant::now());
            }
        }

        let fullscreen_enabled = vm.open;
        if ui
            .add_enabled(
                fullscreen_enabled,
                egui::Button::new(tr("presentation-fullscreen-button")),
            )
            .clicked()
        {
            vm.fullscreen_toggle_requested = true;
        }
    });

    drive_pending_save_debounce(ui, vm, settings, settings_path);
}

// -- Render: the presentation viewport's own content --------------------------

/// Renders the presentation viewport's content: `panel_text` painted at
/// `style`'s size/colors over a `style.background_color`-filled panel (Buzz
/// `update_transcripts`'s `setHtml`, `presentation_window.py:113-136`).
/// Builds its own [`egui::CentralPanel`] (rather than nesting inside the
/// caller's own panel, unlike most other view functions in this crate)
/// because the colored fill must be set on the panel's `Frame` at creation —
/// `app.rs`'s `show_presentation_viewport` calls this directly with the
/// viewport's own root `ui` (the `&mut egui::Ui` `show_viewport_deferred`'s
/// callback receives — `egui::Ui: Deref<Target = Context>`, which is why
/// every deferred-viewport closure elsewhere in this crate can call
/// `ctx.input(...)`/`ctx.send_viewport_cmd(...)` on what is actually a
/// `&mut Ui`). Auto-scrolls to the bottom on every call
/// (`ScrollArea::stick_to_bottom`) — the idiomatic egui equivalent of Buzz's
/// explicit `moveCursor(End)` after each wholesale-replace update.
pub fn window_ui(ui: &mut egui::Ui, panel_text: &str, style: &EffectiveStyle) {
    let bg = parse_hex_color(&style.background_color).unwrap_or([255, 255, 255]);
    let fg = parse_hex_color(&style.text_color).unwrap_or([0, 0, 0]);
    let bg_color = egui::Color32::from_rgb(bg[0], bg[1], bg[2]);
    let fg_color = egui::Color32::from_rgb(fg[0], fg[1], fg[2]);

    egui::CentralPanel::default()
        .frame(egui::Frame::default().fill(bg_color).inner_margin(20.0))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .stick_to_bottom(true)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(panel_text)
                            .color(fg_color)
                            .size(style.font_size_pt as f32),
                    );
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- hex color validation ---------------------------------------------------

    #[test]
    fn valid_hex_color_accepts_and_normalizes() {
        assert_eq!(valid_hex_color("#FF0000"), Some("#ff0000".to_string()));
        assert_eq!(valid_hex_color("  #00ff00  "), Some("#00ff00".to_string()));
    }

    #[test]
    fn valid_hex_color_rejects_junk() {
        assert_eq!(valid_hex_color("ff0000"), None, "missing #");
        assert_eq!(valid_hex_color("#fff"), None, "3-digit not supported");
        assert_eq!(valid_hex_color("#gggggg"), None, "non-hex digits");
        assert_eq!(valid_hex_color("red"), None, "named colors not supported");
    }

    #[test]
    fn hex_rgb_round_trip() {
        let rgb = parse_hex_color("#1a2b3c").unwrap();
        assert_eq!(rgb, [0x1a, 0x2b, 0x3c]);
        assert_eq!(hex_from_rgb(rgb), "#1a2b3c");
    }

    // -- theme -> color resolution -----------------------------------------------

    #[test]
    fn light_theme_is_black_on_white() {
        assert_eq!(
            resolve_theme_colors(PresentationTheme::Light, "#123456", "#654321"),
            ("#000000".to_string(), "#ffffff".to_string())
        );
    }

    #[test]
    fn dark_theme_is_white_on_black() {
        assert_eq!(
            resolve_theme_colors(PresentationTheme::Dark, "#123456", "#654321"),
            ("#ffffff".to_string(), "#000000".to_string())
        );
    }

    #[test]
    fn custom_theme_uses_the_stored_colors() {
        assert_eq!(
            resolve_theme_colors(PresentationTheme::Custom, "#123456", "#654321"),
            ("#123456".to_string(), "#654321".to_string())
        );
    }

    #[test]
    fn custom_theme_falls_back_to_defaults_on_bad_hex() {
        assert_eq!(
            resolve_theme_colors(PresentationTheme::Custom, "not-a-color", "also-bad"),
            (
                PresentationSettings::DEFAULT_TEXT_COLOR.to_string(),
                PresentationSettings::DEFAULT_BACKGROUND_COLOR.to_string()
            )
        );
    }

    #[test]
    fn theme_from_str_falls_back_to_light_for_unknown_values() {
        assert_eq!(
            PresentationTheme::parse_setting("light"),
            PresentationTheme::Light
        );
        assert_eq!(
            PresentationTheme::parse_setting("dark"),
            PresentationTheme::Dark
        );
        assert_eq!(
            PresentationTheme::parse_setting("custom"),
            PresentationTheme::Custom
        );
        assert_eq!(
            PresentationTheme::parse_setting("bogus"),
            PresentationTheme::Light
        );
        assert_eq!(
            PresentationTheme::parse_setting(""),
            PresentationTheme::Light
        );
    }

    // -- CSS-subset parser --------------------------------------------------------

    #[test]
    fn parses_buzzs_own_default_stylesheet_shape() {
        let css = r#"
            body {
                color: #112233;
                background-color: #445566;
                font-size: 24pt;
                font-family: Arial, sans-serif;
                padding: 0;
                margin: 20px;
            }
        "#;
        let parsed = parse_presentation_css(css).expect("recognized declarations present");
        assert_eq!(parsed.color, Some("#112233".to_string()));
        assert_eq!(parsed.background_color, Some("#445566".to_string()));
        assert_eq!(parsed.font_size_pt, Some(24));
    }

    #[test]
    fn ignores_junk_properties_but_keeps_the_recognized_ones() {
        let css = "color: #000000; some-nonsense-property: whatever; font-size: 12pt;";
        let parsed = parse_presentation_css(css).unwrap();
        assert_eq!(parsed.color, Some("#000000".to_string()));
        assert_eq!(parsed.background_color, None);
        assert_eq!(parsed.font_size_pt, Some(12));
    }

    #[test]
    fn a_partial_override_leaves_the_other_fields_none() {
        let parsed = parse_presentation_css("font-size: 30pt;").unwrap();
        assert_eq!(parsed.color, None);
        assert_eq!(parsed.background_color, None);
        assert_eq!(parsed.font_size_pt, Some(30));
    }

    #[test]
    fn a_malformed_file_with_nothing_recognized_yields_none() {
        assert_eq!(parse_presentation_css("this is not css at all"), None);
        assert_eq!(parse_presentation_css(""), None);
        assert_eq!(
            parse_presentation_css("font-family: Arial; padding: 0;"),
            None,
            "only unrecognized properties present"
        );
    }

    #[test]
    fn an_invalid_value_for_a_recognized_property_is_ignored_not_fatal() {
        // "color" is recognized but its value is junk -- ignored, and since
        // nothing else is recognized either, the whole thing is None.
        assert_eq!(parse_presentation_css("color: not-a-color;"), None);
    }

    #[test]
    fn font_size_requires_the_pt_suffix() {
        assert_eq!(parse_pt_size("24pt"), Some(24));
        assert_eq!(parse_pt_size("24PT"), Some(24));
        assert_eq!(parse_pt_size("24.6pt"), Some(25), "rounds to nearest point");
        assert_eq!(parse_pt_size("24px"), None, "wrong unit");
        assert_eq!(parse_pt_size("24"), None, "missing unit");
        assert_eq!(parse_pt_size("pt"), None, "no magnitude");
    }

    // -- effective_style: theme + CSS override composition -----------------------

    #[test]
    fn effective_style_with_no_css_override_is_just_the_theme() {
        let vm = PresentationVm::from_settings(&PresentationSettings::default());
        let style = effective_style(&vm, None);
        assert_eq!(style.text_color, "#000000");
        assert_eq!(style.background_color, "#ffffff");
        assert_eq!(style.font_size_pt, PresentationSettings::DEFAULT_TEXT_SIZE);
    }

    #[test]
    fn effective_style_applies_the_css_override_on_top_of_the_theme() {
        let vm = PresentationVm::from_settings(&PresentationSettings::default());
        let css = CssOverride {
            color: Some("#ff0000".to_string()),
            background_color: None,
            font_size_pt: Some(40),
        };
        let style = effective_style(&vm, Some(&css));
        assert_eq!(style.text_color, "#ff0000", "overridden");
        assert_eq!(
            style.background_color, "#ffffff",
            "theme default kept -- not overridden"
        );
        assert_eq!(style.font_size_pt, 40, "overridden");
    }

    // -- text-size/color save debounce --------------------------------------------

    #[test]
    fn debounce_not_ready_before_100ms() {
        assert!(!pending_save_debounce_ready(Duration::from_millis(99)));
    }

    #[test]
    fn debounce_ready_at_or_after_100ms() {
        assert!(pending_save_debounce_ready(Duration::from_millis(100)));
        assert!(pending_save_debounce_ready(Duration::from_millis(500)));
    }

    // -- PresentationVm::from_settings ---------------------------------------------

    #[test]
    fn from_settings_prefills_from_presentation_settings() {
        let ps = PresentationSettings {
            text_size: 40,
            theme: "dark".to_string(),
            text_color: "#111111".to_string(),
            background_color: "#222222".to_string(),
        };
        let vm = PresentationVm::from_settings(&ps);
        assert!(!vm.open);
        assert_eq!(vm.text_size, 40);
        assert_eq!(vm.theme, PresentationTheme::Dark);
        assert_eq!(vm.text_color, "#111111");
        assert_eq!(vm.background_color, "#222222");
        assert_eq!(vm.pending_save_last_edit, None);
        assert_eq!(vm.css_override, None);
        assert!(
            vm.css_dirty,
            "the first presentation-viewport render must load the CSS override once"
        );
    }

    // -- PresentationVm::flush_pending_save (review fix) --------------------------

    #[test]
    fn flush_pending_save_persists_a_pending_edit_regardless_of_elapsed_time() {
        // The whole point of the flush is that it does NOT wait out the
        // ~100ms debounce -- an edit staged an instant ago (injected clock:
        // `Instant::now()`, never advanced) must still be persisted
        // immediately, since `bar_ui` (the only thing that would otherwise
        // drive the debounce to completion) is about to stop rendering.
        let mut vm = PresentationVm::from_settings(&PresentationSettings::default());
        vm.text_size = 42;
        vm.text_color = "#123456".to_string();
        vm.background_color = "#654321".to_string();
        vm.pending_save_last_edit = Some(Instant::now());
        vm.css_dirty = false; // simulate "already reloaded this frame"

        let mut settings = PresentationSettings::default();
        let flushed = vm.flush_pending_save(&mut settings);

        assert!(flushed, "a pending edit must report that it was flushed");
        assert_eq!(settings.text_size, 42);
        assert_eq!(settings.text_color, "#123456");
        assert_eq!(settings.background_color, "#654321");
        assert_eq!(
            vm.pending_save_last_edit, None,
            "the pending timer must clear so a later real debounce fire doesn't double-save"
        );
        assert!(
            vm.css_dirty,
            "a flushed save is a settings change -- same CSS-reload trigger as a normal debounce fire"
        );
    }

    #[test]
    fn flush_pending_save_is_a_noop_when_nothing_is_pending() {
        let mut vm = PresentationVm::from_settings(&PresentationSettings::default());
        vm.css_dirty = false;
        let mut settings = PresentationSettings {
            text_size: 99,
            ..PresentationSettings::default()
        };

        let flushed = vm.flush_pending_save(&mut settings);

        assert!(!flushed);
        assert_eq!(settings.text_size, 99, "untouched -- nothing was pending");
        assert!(!vm.css_dirty, "a no-op flush must not force a CSS reload");
    }
}
