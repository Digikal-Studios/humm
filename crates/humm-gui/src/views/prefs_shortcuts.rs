//! Preferences → Shortcuts tab (F-005 editor half, design spec §8.3, Task
//! 14): one capture row per [`ShortcutAction::ALL`], a Reset-to-Defaults
//! button, and non-blocking conflict warnings.
//!
//! Ported from Buzz:
//! - `buzz/widgets/preferences_dialog/shortcuts_editor_preferences_widget.py`
//!   — one form row per `Shortcut` with a `SequenceEdit`, `keySequenceChanged`
//!   → `shortcuts.set` + a changed-signal emit (`:39-44`, live re-apply), and
//!   `reset_to_defaults` (`:46-56`, `shortcuts.clear()` then repaints every
//!   row from the now-cleared store).
//! - `buzz/widgets/sequence_edit.py` — `keyPressEvent`'s two special cases:
//!   Esc is intercepted and forwarded to the parent (`:17-23`) — closing
//!   Buzz's whole Preferences `QDialog` (that file's own comment: "the user
//!   can't set a shortcut that contains the Esc key" is a side effect of
//!   Qt's default modal-dialog behavior, where Escape rejects the dialog,
//!   deliberately not suppressed here). humm's Preferences window is a
//!   non-modal, write-through window with no Esc-closes-the-window semantics
//!   anywhere else in it, so there is no dialog for Esc to close — here Esc
//!   cancels only the in-flight row capture ([`resolve_capture_event`]'s
//!   `Cancel` outcome). **This is a deliberate adaptation, not the same
//!   mechanism**: the one invariant the two share is that Esc can never
//!   itself become a bound sequence. Buzz's second special case, ignoring a
//!   bare modifier press (`:26-33`), *does* have an egui equivalent to port
//!   — egui 0.35 defines physical-position modifier variants
//!   (`Key::ShiftLeft`/`ShiftRight`, `ControlLeft`/`ControlRight`,
//!   `AltLeft`/`AltRight`, `SuperLeft`/`SuperRight`, `key.rs:199-220`),
//!   emitted as physical `Event::Key` presses just like Buzz's
//!   `Qt.Key.Key_Control` etc. The guard is implemented implicitly:
//!   [`shortcuts::key_token`]'s vocabulary (letters, digits, comma, Return,
//!   the two arrow keys) simply excludes every one of those modifier
//!   variants, so `resolve_capture_event`'s `shortcuts::key_token(key)?`
//!   early-return already rejects a bare modifier press — an intentional
//!   guard equivalent to Buzz's, not an accident of egui lacking the
//!   concept (tested explicitly below, one case per modifier key).
//! - `buzz/settings/shortcuts.py` — `set`/`clear`: `set` stores only the
//!   override ([`apply_override`] additionally keeps the map *minimal* by
//!   removing an override that has been set back to the action's own
//!   default — a humm-only refinement, not a Buzz behavior); `clear` wipes
//!   the whole map ([`commit_reset_all`]).
//!
//! ## Live re-apply: the generation handshake
//!
//! Every commit in this module ([`commit_capture`]/[`commit_clear`]/
//! [`commit_reset_all`]) writes `settings.shortcuts`, saves, and bumps
//! `shortcuts_generation`. `HummApp` compares that counter against its own
//! last-seen value once per frame and rebuilds `Arc<ShortcutMap>` when it has
//! moved (`app.rs`'s own doc comment on the field) — every consumer (main
//! window, viewer, search) already re-reads that `Arc` fresh every frame, so
//! the rebuilt map reaches all of them on the very next frame with no
//! per-consumer wiring. This module's own display (`current_map`, passed in
//! fresh each call) therefore lags a click by at most one frame *except* for
//! the row actually being edited, whose field is rendered from the
//! just-computed [`shortcuts::serialize_shortcut`] value directly (no lag at
//! all) — see [`ui`]'s own doc comment for the row-loop details.
//!
//! ## Focus and multi-viewport isolation
//!
//! The Preferences window is a separate *immediate* egui viewport
//! (`app.rs::HummApp::show_preferences_viewport`); egui delivers raw input
//! per-viewport (each OS window gets its own `InputState`), so a keystroke
//! captured here is never also visible to the main window's
//! `handle_global_shortcuts` or a transcript viewer's own shortcut
//! consumption — both read `ui.input()`/`ui.input_mut()` scoped to *their*
//! viewport, which this window's keystrokes never reach. This module's own
//! event-consumption ([`consume_capture_input`]) exists only to stop a
//! recognized keypress from *also* doing something else inside this same
//! window (e.g. Space/Enter activating the focused button by egui's own
//! default rules), not to prevent cross-window leakage, which the viewport
//! boundary already rules out.
//!
//! While capturing, the row's field calls [`egui::Response::request_focus`]
//! so it visibly holds keyboard focus; clicking a *different* row's field
//! switches capture to that row outright (handled by collecting every row's
//! click before applying any state change, so the switch is correct
//! regardless of egui's own focus-transition timing — see [`ui`]'s doc). If
//! the user instead clicks something else entirely (a checkbox, another
//! tab), the capturing field's own [`egui::Response::lost_focus`] transition
//! cancels capture — best-effort, since (unlike the deterministic
//! click-a-different-row case) egui only reports that transition once focus
//! has actually moved.

use std::collections::HashMap;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use egui::{Key, KeyboardShortcut, Modifiers};
use humm_core::settings::Settings;

use crate::i18n::{tr, tr_args};
use crate::shortcuts::{self, ShortcutAction, ShortcutMap};

/// A pale amber, used only for the conflict-warning glyph/caption — matches
/// no other warning color in this crate (a fresh choice: conflicts are new
/// with this tab), deliberately distinct from [`egui::Color32::RED`]
/// (`views/url_dialog.rs`'s validation-error color) since a shortcut
/// conflict is explicitly non-blocking (design spec §8.3: "the assignment
/// stands"), not an error.
const CONFLICT_COLOR: egui::Color32 = egui::Color32::from_rgb(0xCC, 0x8A, 0x00);

/// The width reserved for a row's sequence-capture field, wide enough for
/// the longest formatted sequence this module ever shows
/// (`⌃⇧←`-style Mac symbols or `Ctrl+Shift+Left`-style Windows/Linux names)
/// without visibly reflowing between rows.
const FIELD_WIDTH: f32 = 150.0;

/// The Shortcuts tab's own live state, held in
/// [`crate::views::preferences::PreferencesState`]. `Default` starts with no
/// row capturing — the tab's rest state whenever the Preferences window
/// (re)opens (`PreferencesState::new` never restores an in-flight capture
/// across window closes, matching every other tab's own "rebuilt fresh on
/// open" contract).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShortcutsTabState {
    /// The action whose row is currently waiting for a keypress, if any.
    pub capturing: Option<ShortcutAction>,
}

// -- Pure decision functions (kittest/unit-testable without an egui::Ui) -----

/// Sets or clears `action`'s entry in `overrides`, keeping the map *minimal*
/// (Buzz `Shortcuts.set` stores the override unconditionally; this is
/// humm's own refinement — module doc): removes the entry when `sequence`
/// equals the action's built-in default instead of storing a redundant
/// explicit override that [`ShortcutMap::from_settings`] would have resolved
/// to the same value anyway.
pub fn apply_override(
    overrides: &mut HashMap<String, String>,
    action: ShortcutAction,
    sequence: &str,
) {
    if sequence == action.default_sequence() {
        overrides.remove(action.settings_key());
    } else {
        overrides.insert(action.settings_key().to_string(), sequence.to_string());
    }
}

/// Locks `settings`, applies `f` to `settings.shortcuts`, and saves —
/// `views/prefs_general.rs`'s `persist_openai` shape, one section over.
fn persist_shortcuts(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    f: impl FnOnce(&mut HashMap<String, String>),
) {
    let mut guard = settings
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard.shortcuts);
    if let Err(err) = guard.save(settings_path) {
        log::warn!("could not persist the shortcut settings: {err}");
    }
}

/// Bumps `generation` — single-threaded by construction (both the write here
/// and the read in `app.rs`'s per-frame rebuild check happen on the main UI
/// thread: the Preferences window is an *immediate* viewport, rendered
/// inline on the same thread as the rest of the frame, never on a separate
/// render thread the way a deferred viewport might be), so `Relaxed` costs
/// nothing extra over a stronger ordering here would buy.
fn bump_generation(generation: &AtomicUsize) {
    generation.fetch_add(1, Ordering::Relaxed);
}

/// Commits a captured `sequence` for `action`: writes the (possibly
/// minimality-collapsed) override, saves, and bumps `generation` so
/// `HummApp` rebuilds the resolved map next frame (module doc).
pub fn commit_capture(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    generation: &AtomicUsize,
    action: ShortcutAction,
    sequence: &str,
) {
    persist_shortcuts(settings, settings_path, |overrides| {
        apply_override(overrides, action, sequence);
    });
    bump_generation(generation);
}

/// Commits a per-row Clear: removes `action`'s override entirely (restoring
/// its built-in default — design spec §8.3's per-row clear button), saves,
/// and bumps `generation`.
pub fn commit_clear(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    generation: &AtomicUsize,
    action: ShortcutAction,
) {
    persist_shortcuts(settings, settings_path, |overrides| {
        overrides.remove(action.settings_key());
    });
    bump_generation(generation);
}

/// Commits Reset to Defaults: clears the *entire* overrides map (Buzz
/// `Shortcuts.clear`, `shortcuts.py:20-21`), saves, and bumps `generation`.
pub fn commit_reset_all(
    settings: &Mutex<Settings>,
    settings_path: &Path,
    generation: &AtomicUsize,
) {
    persist_shortcuts(settings, settings_path, HashMap::clear);
    bump_generation(generation);
}

// -- Capture-input resolution -------------------------------------------------

/// What a keypress resolves to while a row is capturing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum CaptureOutcome {
    /// Esc — cancel capture without binding anything (Buzz's documented
    /// "can't bind Esc" limitation, module doc).
    Cancel,
    /// A [`shortcuts::key_token`]-recognized key, with whatever
    /// Ctrl(command)/Shift/Alt modifiers were held — bind this sequence.
    Bound(KeyboardShortcut),
}

/// Pure core of the capture widget's key handling: resolves one raw
/// `(key, modifiers)` pair — as delivered by an egui `Event::Key { pressed:
/// true, .. }` — to a [`CaptureOutcome`], or `None` if `key` is neither Esc
/// nor in [`shortcuts::key_token`]'s bindable vocabulary. That vocabulary
/// exclusion is also what implements Buzz's "ignore a bare modifier press"
/// guard (module doc): a physical `Key::ControlLeft`/`ShiftRight`/`AltLeft`/
/// `SuperLeft`-etc. press (egui does emit these) resolves to `None` here
/// exactly like an unrelated non-vocabulary key (Tab, a function key) would
/// — left alone entirely so it doesn't disrupt focus navigation.
/// `modifiers.ctrl`/`.mac_cmd` are deliberately dropped in favor
/// of `.command` alone when building the bound shortcut, normalizing to
/// exactly the representation [`shortcuts::parse_sequence`] itself produces
/// for a `"Ctrl+…"` token (cross-platform: `.command` mirrors the physical
/// Ctrl key on Windows/Linux and the physical Cmd key on Mac, per
/// [`egui::Modifiers`]'s own doc) — this is what makes the round trip
/// (capture → [`shortcuts::serialize_shortcut`] → [`shortcuts::
/// parse_sequence`]) land on the same [`KeyboardShortcut`], not a
/// platform-specific look-alike.
fn resolve_capture_event(key: Key, modifiers: Modifiers) -> Option<CaptureOutcome> {
    if key == Key::Escape {
        return Some(CaptureOutcome::Cancel);
    }
    shortcuts::key_token(key)?;
    let bound_modifiers = Modifiers {
        alt: modifiers.alt,
        ctrl: false,
        shift: modifiers.shift,
        mac_cmd: false,
        command: modifiers.command,
    };
    Some(CaptureOutcome::Bound(KeyboardShortcut::new(
        bound_modifiers,
        key,
    )))
}

/// Scans this frame's raw key-press events for the first one
/// [`resolve_capture_event`] resolves, consuming exactly that event (so it
/// doesn't also, say, activate the focused button via egui's own
/// Enter/Space-activates-buttons default) and leaving every other event
/// untouched.
fn consume_capture_input(ui: &egui::Ui) -> Option<CaptureOutcome> {
    ui.input_mut(|input| {
        let mut outcome = None;
        input.events.retain(|event| {
            if outcome.is_some() {
                return true;
            }
            let egui::Event::Key {
                key,
                pressed: true,
                modifiers,
                ..
            } = event
            else {
                return true;
            };
            match resolve_capture_event(*key, *modifiers) {
                Some(result) => {
                    outcome = Some(result);
                    false
                }
                None => true,
            }
        });
        outcome
    })
}

// -- Render -------------------------------------------------------------------

/// Renders the Shortcuts tab: one row per [`ShortcutAction::ALL`]
/// (description label + capture field + per-row Clear + conflict caption),
/// then a Reset to Defaults button.
///
/// `current_map` is the app's already-resolved [`ShortcutMap`] (`HummApp`'s
/// own `self.shortcuts`, rebuilt from `settings.shortcuts` whenever
/// `shortcuts_generation` moves — module doc); it drives every row's
/// *displayed* sequence and the conflict list computed once up front. The
/// one exception: the row currently capturing, once it resolves a binding
/// this very frame, renders that binding's freshly-formatted text directly
/// (not looked up in `current_map`, which cannot reflect a write this same
/// call is about to make) — so the edited row never itself shows a stale
/// value, even though *other* rows' conflict captions may lag by the usual
/// one frame.
///
/// Row-click handling is two-phase to make switching capture between rows
/// correct regardless of egui's own focus-transition timing: which row (if
/// any) is capturing *at the start* of this call is fixed in
/// `currently_capturing` and used for every row's capturing-branch logic;
/// each row's click is only *collected* into `clicked_action` during the
/// loop, and applied to `state.capturing` once, after every row has
/// rendered. A click on a row other than the one currently capturing always
/// wins, deterministically switching capture to it.
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut ShortcutsTabState,
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
    shortcuts_generation: &Arc<AtomicUsize>,
    current_map: &ShortcutMap,
) {
    let currently_capturing = state.capturing;
    let conflicts = shortcuts::conflicts(current_map);
    let mut clicked_action: Option<ShortcutAction> = None;

    for action in ShortcutAction::ALL {
        ui.horizontal(|ui| {
            ui.label(tr(action.description_key()));

            let is_capturing = currently_capturing == Some(action);
            let mut field_text = if is_capturing {
                tr("prefs-shortcuts-press")
            } else {
                ui.ctx().format_shortcut(&current_map.get(action))
            };

            if is_capturing {
                match consume_capture_input(ui) {
                    Some(CaptureOutcome::Cancel) => {
                        state.capturing = None;
                    }
                    Some(CaptureOutcome::Bound(shortcut)) => {
                        let sequence = shortcuts::serialize_shortcut(&shortcut);
                        commit_capture(
                            settings,
                            settings_path,
                            shortcuts_generation,
                            action,
                            &sequence,
                        );
                        // Same-frame feedback for the row just edited (doc
                        // comment above) -- current_map can't reflect this
                        // write until next frame's rebuild.
                        field_text = ui.ctx().format_shortcut(&shortcut);
                        state.capturing = None;
                    }
                    None => {}
                }
            }

            let response =
                ui.add_sized(egui::vec2(FIELD_WIDTH, 0.0), egui::Button::new(field_text));
            if is_capturing {
                response.request_focus();
                if response.lost_focus() {
                    state.capturing = None;
                }
            }
            if response.clicked() && currently_capturing != Some(action) {
                clicked_action = Some(action);
            }

            if ui
                .small_button("\u{21ba}") // ↺
                .on_hover_text(tr("prefs-shortcuts-clear-tooltip"))
                .clicked()
            {
                commit_clear(settings, settings_path, shortcuts_generation, action);
                // Clearing the row that's mid-capture also ends the capture
                // (Reset to Defaults' own unconditional reset, scoped here to
                // just this row -- clearing a *different* action's override
                // must not cancel whatever row is currently capturing).
                if currently_capturing == Some(action) {
                    state.capturing = None;
                }
            }

            if let Some(other) = conflicting_action(&conflicts, action) {
                ui.colored_label(CONFLICT_COLOR, "\u{26A0}"); // ⚠
                ui.colored_label(
                    CONFLICT_COLOR,
                    tr_args(
                        "prefs-shortcuts-conflict",
                        &[("other", &tr(other.description_key()))],
                    ),
                );
            }
        });
    }

    if let Some(action) = clicked_action {
        state.capturing = Some(action);
    }

    ui.separator();
    if ui.button(tr("prefs-shortcuts-reset")).clicked() {
        commit_reset_all(settings, settings_path, shortcuts_generation);
        state.capturing = None;
    }
}

/// The other action `action` conflicts with, if any (first match in
/// `conflicts`' own order — Task 14 renders at most one caption per row even
/// if an action somehow collided with two others at once, which cannot
/// currently happen since a shortcut is a single value, but this keeps the
/// row's layout to one line regardless).
fn conflicting_action(
    conflicts: &[(ShortcutAction, ShortcutAction)],
    action: ShortcutAction,
) -> Option<ShortcutAction> {
    conflicts.iter().find_map(|&(a, b)| {
        if a == action {
            Some(b)
        } else if b == action {
            Some(a)
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- apply_override (map minimality) -------------------------------------

    #[test]
    fn apply_override_stores_a_non_default_sequence() {
        let mut overrides = HashMap::new();
        apply_override(
            &mut overrides,
            ShortcutAction::OpenImportWindow,
            "Ctrl+Shift+I",
        );
        assert_eq!(
            overrides.get(ShortcutAction::OpenImportWindow.settings_key()),
            Some(&"Ctrl+Shift+I".to_string())
        );
    }

    #[test]
    fn apply_override_setting_back_to_default_removes_the_entry() {
        let mut overrides = HashMap::new();
        apply_override(
            &mut overrides,
            ShortcutAction::OpenImportWindow,
            "Ctrl+Shift+I",
        );
        assert!(!overrides.is_empty());

        apply_override(
            &mut overrides,
            ShortcutAction::OpenImportWindow,
            ShortcutAction::OpenImportWindow.default_sequence(),
        );

        assert!(
            overrides.is_empty(),
            "setting an action back to its own default must remove the override, \
             not store a redundant explicit entry"
        );
    }

    #[test]
    fn apply_override_never_touches_other_actions_entries() {
        let mut overrides = HashMap::new();
        overrides.insert(
            ShortcutAction::ClearHistory.settings_key().to_string(),
            "Ctrl+Shift+S".to_string(),
        );

        apply_override(
            &mut overrides,
            ShortcutAction::OpenImportWindow,
            ShortcutAction::OpenImportWindow.default_sequence(),
        );

        assert_eq!(
            overrides.get(ShortcutAction::ClearHistory.settings_key()),
            Some(&"Ctrl+Shift+S".to_string()),
            "collapsing one action's override to its default must not disturb another's"
        );
    }

    // -- commit_capture / commit_clear / commit_reset_all --------------------

    #[test]
    fn commit_capture_persists_and_bumps_generation() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let settings = Mutex::new(Settings::default());
        let generation = AtomicUsize::new(0);

        commit_capture(
            &settings,
            &settings_path,
            &generation,
            ShortcutAction::OpenImportWindow,
            "Ctrl+Shift+I",
        );

        assert_eq!(generation.load(Ordering::Relaxed), 1);
        let reloaded = Settings::load(&settings_path).expect("settings file was written");
        assert_eq!(
            reloaded
                .shortcuts
                .get(ShortcutAction::OpenImportWindow.settings_key()),
            Some(&"Ctrl+Shift+I".to_string())
        );
    }

    #[test]
    fn commit_capture_to_the_default_sequence_removes_any_override() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let mut initial = Settings::default();
        initial.shortcuts.insert(
            ShortcutAction::OpenImportWindow.settings_key().to_string(),
            "Ctrl+Shift+I".to_string(),
        );
        let settings = Mutex::new(initial);
        let generation = AtomicUsize::new(0);

        commit_capture(
            &settings,
            &settings_path,
            &generation,
            ShortcutAction::OpenImportWindow,
            ShortcutAction::OpenImportWindow.default_sequence(),
        );

        let reloaded = Settings::load(&settings_path).expect("settings file was written");
        assert!(
            !reloaded
                .shortcuts
                .contains_key(ShortcutAction::OpenImportWindow.settings_key()),
            "capturing the default sequence must remove the override, not store it"
        );
    }

    #[test]
    fn commit_clear_removes_only_that_actions_override() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let mut initial = Settings::default();
        initial.shortcuts.insert(
            ShortcutAction::OpenImportWindow.settings_key().to_string(),
            "Ctrl+Shift+I".to_string(),
        );
        initial.shortcuts.insert(
            ShortcutAction::ClearHistory.settings_key().to_string(),
            "Ctrl+Shift+S".to_string(),
        );
        let settings = Mutex::new(initial);
        let generation = AtomicUsize::new(0);

        commit_clear(
            &settings,
            &settings_path,
            &generation,
            ShortcutAction::OpenImportWindow,
        );

        assert_eq!(generation.load(Ordering::Relaxed), 1);
        let reloaded = Settings::load(&settings_path).expect("settings file was written");
        assert!(
            !reloaded
                .shortcuts
                .contains_key(ShortcutAction::OpenImportWindow.settings_key())
        );
        assert_eq!(
            reloaded
                .shortcuts
                .get(ShortcutAction::ClearHistory.settings_key()),
            Some(&"Ctrl+Shift+S".to_string()),
            "clearing one action must not disturb another's override"
        );
    }

    #[test]
    fn commit_reset_all_clears_every_override() {
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.toml");
        let mut initial = Settings::default();
        initial.shortcuts.insert(
            ShortcutAction::OpenImportWindow.settings_key().to_string(),
            "Ctrl+Shift+I".to_string(),
        );
        initial.shortcuts.insert(
            ShortcutAction::ClearHistory.settings_key().to_string(),
            "Ctrl+Shift+S".to_string(),
        );
        let settings = Mutex::new(initial);
        let generation = AtomicUsize::new(0);

        commit_reset_all(&settings, &settings_path, &generation);

        assert_eq!(generation.load(Ordering::Relaxed), 1);
        let reloaded = Settings::load(&settings_path).expect("settings file was written");
        assert!(
            reloaded.shortcuts.is_empty(),
            "Reset to Defaults must clear every override, Buzz Shortcuts.clear() parity"
        );
    }

    // -- resolve_capture_event ------------------------------------------------

    #[test]
    fn escape_cancels() {
        assert_eq!(
            resolve_capture_event(Key::Escape, Modifiers::NONE),
            Some(CaptureOutcome::Cancel)
        );
        // Even with modifiers held -- Buzz's Esc-cancels is unconditional.
        assert_eq!(
            resolve_capture_event(Key::Escape, Modifiers::COMMAND),
            Some(CaptureOutcome::Cancel)
        );
    }

    #[test]
    fn a_vocabulary_key_with_command_binds_normalized_ctrl() {
        let outcome = resolve_capture_event(Key::I, Modifiers::COMMAND);
        assert_eq!(
            outcome,
            Some(CaptureOutcome::Bound(KeyboardShortcut::new(
                Modifiers::COMMAND,
                Key::I
            )))
        );
    }

    #[test]
    fn platform_ctrl_and_mac_cmd_bits_are_normalized_away() {
        // On Windows/Linux, a physical Ctrl press sets both `.ctrl` and
        // `.command`; on Mac, a physical Cmd press sets both `.mac_cmd` and
        // `.command` (egui::Modifiers's own doc). Either raw shape must
        // normalize to the same bound shortcut -- only `.command` survives.
        let windows_ctrl = Modifiers {
            ctrl: true,
            command: true,
            ..Modifiers::NONE
        };
        let mac_cmd = Modifiers {
            mac_cmd: true,
            command: true,
            ..Modifiers::NONE
        };
        let expected = Some(CaptureOutcome::Bound(KeyboardShortcut::new(
            Modifiers::COMMAND,
            Key::S,
        )));
        assert_eq!(resolve_capture_event(Key::S, windows_ctrl), expected);
        assert_eq!(resolve_capture_event(Key::S, mac_cmd), expected);
    }

    #[test]
    fn an_unbindable_key_resolves_to_none() {
        // Tab has no shortcuts.rs token -- must be left alone (not bound,
        // not treated as a cancel).
        assert_eq!(resolve_capture_event(Key::Tab, Modifiers::NONE), None);
    }

    #[test]
    fn a_bare_modifier_key_press_resolves_to_none() {
        // egui 0.35 *does* define physical-position modifier variants
        // (module doc, `key.rs:199-220`) -- a bare press of one of them must
        // be ignored exactly like any other non-vocabulary key (the sibling
        // of `an_unbindable_key_resolves_to_none` above), not bound and not
        // treated as a cancel. This is the vocabulary-exclusion guard that
        // stands in for Buzz's own `sequence_edit.py:26-33` modifier-only
        // check.
        for key in [
            Key::ControlLeft,
            Key::ShiftRight,
            Key::AltLeft,
            Key::SuperLeft,
        ] {
            assert_eq!(
                resolve_capture_event(key, Modifiers::NONE),
                None,
                "{key:?} must not resolve to a binding or a cancel"
            );
        }
    }

    #[test]
    fn a_captured_binding_round_trips_through_serialize_and_parse() {
        let Some(CaptureOutcome::Bound(shortcut)) =
            resolve_capture_event(Key::Num5, Modifiers::COMMAND)
        else {
            panic!("Num5 with Command must resolve to a binding");
        };
        let sequence = shortcuts::serialize_shortcut(&shortcut);
        assert_eq!(sequence, "Ctrl+5");
        assert_eq!(crate::shortcuts::parse_sequence(&sequence), Some(shortcut));
    }

    // -- conflicting_action ----------------------------------------------------

    #[test]
    fn conflicting_action_finds_the_other_side_of_a_pair_either_direction() {
        let pairs = vec![(
            ShortcutAction::OpenImportWindow,
            ShortcutAction::ClearHistory,
        )];
        assert_eq!(
            conflicting_action(&pairs, ShortcutAction::OpenImportWindow),
            Some(ShortcutAction::ClearHistory)
        );
        assert_eq!(
            conflicting_action(&pairs, ShortcutAction::ClearHistory),
            Some(ShortcutAction::OpenImportWindow)
        );
        assert_eq!(
            conflicting_action(&pairs, ShortcutAction::OpenRecordWindow),
            None
        );
    }
}
