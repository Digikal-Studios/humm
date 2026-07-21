//! Buzz-default keyboard shortcuts (F-005 defaults).
//!
//! Ported from `buzz/settings/shortcut.py`: 20 actions, each with a default
//! key sequence (source count re-verified 2026-07-21 — earlier analysis docs
//! said 21; `buzz-notes-options-shortcuts-telemetry.md` §F-005 has the
//! corrected table). Buzz stores only *overrides* in a flat `shortcuts`
//! settings map keyed by the action's `SCREAMING_SNAKE_CASE` enum name
//! (`get()` = override else default); [`ShortcutMap::from_settings`] mirrors
//! that lookup exactly, so `[shortcuts]` entries in `settings.toml` use the
//! same key spelling Buzz's `QSettings` did.
//!
//! Qt maps `Ctrl` to `⌘` on macOS automatically; [`parse_sequence`] gets the
//! same behavior for free by mapping the `Ctrl` token to
//! [`egui::Modifiers::COMMAND`] (cmd-on-mac, ctrl-elsewhere) rather than the
//! platform-literal [`egui::Modifiers::CTRL`].

use std::collections::HashMap;

use egui::{Key, KeyboardShortcut, Modifiers};

/// One of Buzz's 20 shortcut-bindable actions (`shortcut.py`'s
/// `ShortcutSettingKey` enum, ported verbatim — names, order, and default
/// sequences all re-checked against the source).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    /// `OPEN_RECORD_WINDOW` — "Open Record Window".
    OpenRecordWindow,
    /// `OPEN_IMPORT_WINDOW` — "Import File".
    OpenImportWindow,
    /// `OPEN_IMPORT_URL_WINDOW` — "Import URL".
    OpenImportUrlWindow,
    /// `OPEN_PREFERENCES_WINDOW` — "Open Preferences Window".
    OpenPreferencesWindow,
    /// `VIEW_TRANSCRIPT_TEXT` — "View Transcript Text".
    ViewTranscriptText,
    /// `VIEW_TRANSCRIPT_TRANSLATION` — "View Transcript Translation".
    ViewTranscriptTranslation,
    /// `VIEW_TRANSCRIPT_TIMESTAMPS` — "View Transcript Timestamps".
    ViewTranscriptTimestamps,
    /// `SEARCH_TRANSCRIPT` — "Search Transcript".
    SearchTranscript,
    /// `SEARCH_NEXT`.
    SearchNext,
    /// `SEARCH_PREVIOUS`.
    SearchPrevious,
    /// `SCROLL_TO_CURRENT_TEXT`.
    ScrollToCurrentText,
    /// `PLAY_PAUSE_AUDIO`.
    PlayPauseAudio,
    /// `REPLAY_CURRENT_SEGMENT`.
    ReplayCurrentSegment,
    /// `TOGGLE_PLAYBACK_CONTROLS`.
    TogglePlaybackControls,
    /// `DECREASE_SEGMENT_START`.
    DecreaseSegmentStart,
    /// `INCREASE_SEGMENT_START`.
    IncreaseSegmentStart,
    /// `DECREASE_SEGMENT_END`.
    DecreaseSegmentEnd,
    /// `INCREASE_SEGMENT_END`.
    IncreaseSegmentEnd,
    /// `CLEAR_HISTORY` — "Clear History".
    ClearHistory,
    /// `STOP_TRANSCRIPTION` — "Cancel Transcription".
    StopTranscription,
}

impl ShortcutAction {
    /// Every action, in `shortcut.py`'s declared order — the 20-action set
    /// [`ShortcutMap::from_settings`] builds bindings for.
    pub const ALL: [ShortcutAction; 20] = [
        ShortcutAction::OpenRecordWindow,
        ShortcutAction::OpenImportWindow,
        ShortcutAction::OpenImportUrlWindow,
        ShortcutAction::OpenPreferencesWindow,
        ShortcutAction::ViewTranscriptText,
        ShortcutAction::ViewTranscriptTranslation,
        ShortcutAction::ViewTranscriptTimestamps,
        ShortcutAction::SearchTranscript,
        ShortcutAction::SearchNext,
        ShortcutAction::SearchPrevious,
        ShortcutAction::ScrollToCurrentText,
        ShortcutAction::PlayPauseAudio,
        ShortcutAction::ReplayCurrentSegment,
        ShortcutAction::TogglePlaybackControls,
        ShortcutAction::DecreaseSegmentStart,
        ShortcutAction::IncreaseSegmentStart,
        ShortcutAction::DecreaseSegmentEnd,
        ShortcutAction::IncreaseSegmentEnd,
        ShortcutAction::ClearHistory,
        ShortcutAction::StopTranscription,
    ];

    /// The `SCREAMING_SNAKE_CASE` name Buzz's `QSettings` stores overrides
    /// under (and this action's variant name, ported verbatim) — the key
    /// [`ShortcutMap::from_settings`] looks up in the settings overrides map.
    pub fn settings_key(self) -> &'static str {
        match self {
            ShortcutAction::OpenRecordWindow => "OPEN_RECORD_WINDOW",
            ShortcutAction::OpenImportWindow => "OPEN_IMPORT_WINDOW",
            ShortcutAction::OpenImportUrlWindow => "OPEN_IMPORT_URL_WINDOW",
            ShortcutAction::OpenPreferencesWindow => "OPEN_PREFERENCES_WINDOW",
            ShortcutAction::ViewTranscriptText => "VIEW_TRANSCRIPT_TEXT",
            ShortcutAction::ViewTranscriptTranslation => "VIEW_TRANSCRIPT_TRANSLATION",
            ShortcutAction::ViewTranscriptTimestamps => "VIEW_TRANSCRIPT_TIMESTAMPS",
            ShortcutAction::SearchTranscript => "SEARCH_TRANSCRIPT",
            ShortcutAction::SearchNext => "SEARCH_NEXT",
            ShortcutAction::SearchPrevious => "SEARCH_PREVIOUS",
            ShortcutAction::ScrollToCurrentText => "SCROLL_TO_CURRENT_TEXT",
            ShortcutAction::PlayPauseAudio => "PLAY_PAUSE_AUDIO",
            ShortcutAction::ReplayCurrentSegment => "REPLAY_CURRENT_SEGMENT",
            ShortcutAction::TogglePlaybackControls => "TOGGLE_PLAYBACK_CONTROLS",
            ShortcutAction::DecreaseSegmentStart => "DECREASE_SEGMENT_START",
            ShortcutAction::IncreaseSegmentStart => "INCREASE_SEGMENT_START",
            ShortcutAction::DecreaseSegmentEnd => "DECREASE_SEGMENT_END",
            ShortcutAction::IncreaseSegmentEnd => "INCREASE_SEGMENT_END",
            ShortcutAction::ClearHistory => "CLEAR_HISTORY",
            ShortcutAction::StopTranscription => "STOP_TRANSCRIPTION",
        }
    }

    /// Buzz's default key sequence for this action (`shortcut.py`), in the
    /// `Ctrl+Shift+P`-style notation [`parse_sequence`] accepts.
    pub fn default_sequence(self) -> &'static str {
        match self {
            ShortcutAction::OpenRecordWindow => "Ctrl+R",
            ShortcutAction::OpenImportWindow => "Ctrl+O",
            ShortcutAction::OpenImportUrlWindow => "Ctrl+U",
            ShortcutAction::OpenPreferencesWindow => "Ctrl+,",
            ShortcutAction::ViewTranscriptText => "Ctrl+E",
            ShortcutAction::ViewTranscriptTranslation => "Ctrl+L",
            ShortcutAction::ViewTranscriptTimestamps => "Ctrl+T",
            ShortcutAction::SearchTranscript => "Ctrl+F",
            ShortcutAction::SearchNext => "Ctrl+Return",
            ShortcutAction::SearchPrevious => "Shift+Return",
            ShortcutAction::ScrollToCurrentText => "Ctrl+G",
            ShortcutAction::PlayPauseAudio => "Ctrl+P",
            ShortcutAction::ReplayCurrentSegment => "Ctrl+Shift+P",
            ShortcutAction::TogglePlaybackControls => "Ctrl+Alt+P",
            ShortcutAction::DecreaseSegmentStart => "Ctrl+Left",
            ShortcutAction::IncreaseSegmentStart => "Ctrl+Right",
            ShortcutAction::DecreaseSegmentEnd => "Ctrl+Shift+Left",
            ShortcutAction::IncreaseSegmentEnd => "Ctrl+Shift+Right",
            ShortcutAction::ClearHistory => "Ctrl+S",
            ShortcutAction::StopTranscription => "Ctrl+X",
        }
    }
}

/// Parses a Buzz-style key sequence (`"Ctrl+Shift+P"`) into an
/// [`egui::KeyboardShortcut`].
///
/// Modifier tokens: `Ctrl` (maps to [`Modifiers::COMMAND`] — cmd-on-mac,
/// ctrl-elsewhere, matching Qt's automatic `Ctrl`→`⌘` remap on macOS),
/// `Shift`, `Alt`. The final token is the key: a single letter/digit, `,`
/// (comma), `Return` (Enter), or `Left`/`Right` (arrow keys) — the full set
/// [`ShortcutAction::default_sequence`] uses. Anything else, including an
/// empty string or an unrecognized token, returns `None`.
pub fn parse_sequence(sequence: &str) -> Option<KeyboardShortcut> {
    let tokens: Vec<&str> = sequence.split('+').collect();
    let (key_token, modifier_tokens) = tokens.split_last()?;

    let mut modifiers = Modifiers::NONE;
    for token in modifier_tokens {
        match *token {
            "Ctrl" => modifiers.command = true,
            "Shift" => modifiers.shift = true,
            "Alt" => modifiers.alt = true,
            _ => return None,
        }
    }

    let key = parse_key(key_token)?;
    Some(KeyboardShortcut::new(modifiers, key))
}

/// Parses the final (key) token of a sequence into an [`egui::Key`].
///
/// Covers exactly the tokens Buzz's 20 default sequences use: `,`, `Return`,
/// `Left`/`Right`, and the ten letters `E F G L O P R S T U X` — spelled out
/// explicitly (rather than derived from a single-char-to-`Key` helper) so
/// this stays correct without depending on an `egui::Key` API surface beyond
/// its named constants.
fn parse_key(token: &str) -> Option<Key> {
    match token {
        "," => Some(Key::Comma),
        "Return" => Some(Key::Enter),
        "Left" => Some(Key::ArrowLeft),
        "Right" => Some(Key::ArrowRight),
        "A" => Some(Key::A),
        "B" => Some(Key::B),
        "C" => Some(Key::C),
        "D" => Some(Key::D),
        "E" => Some(Key::E),
        "F" => Some(Key::F),
        "G" => Some(Key::G),
        "H" => Some(Key::H),
        "I" => Some(Key::I),
        "J" => Some(Key::J),
        "K" => Some(Key::K),
        "L" => Some(Key::L),
        "M" => Some(Key::M),
        "N" => Some(Key::N),
        "O" => Some(Key::O),
        "P" => Some(Key::P),
        "Q" => Some(Key::Q),
        "R" => Some(Key::R),
        "S" => Some(Key::S),
        "T" => Some(Key::T),
        "U" => Some(Key::U),
        "V" => Some(Key::V),
        "W" => Some(Key::W),
        "X" => Some(Key::X),
        "Y" => Some(Key::Y),
        "Z" => Some(Key::Z),
        _ => None,
    }
}

/// The resolved shortcut for every [`ShortcutAction`]: Buzz defaults, with
/// `[shortcuts]` settings overrides applied (Buzz storage parity — flat map,
/// [`ShortcutAction::settings_key`] as the key, only overrides stored).
pub struct ShortcutMap {
    bindings: HashMap<ShortcutAction, KeyboardShortcut>,
}

impl ShortcutMap {
    /// Builds the resolved map: `overrides.get(action.settings_key())` if
    /// present and parseable, else `action.default_sequence()` (which always
    /// parses — covered by this module's test matrix). An override that
    /// fails to parse (a hand-edited settings file with a typo) falls back
    /// to the default rather than leaving the action unbound.
    pub fn from_settings(overrides: &HashMap<String, String>) -> Self {
        let bindings = ShortcutAction::ALL
            .into_iter()
            .map(|action| {
                let sequence = overrides
                    .get(action.settings_key())
                    .map(String::as_str)
                    .unwrap_or_else(|| action.default_sequence());
                let shortcut = parse_sequence(sequence)
                    .or_else(|| parse_sequence(action.default_sequence()))
                    .unwrap_or_else(|| {
                        unreachable!(
                            "default sequence for {:?} must parse (covered by unit tests)",
                            action
                        )
                    });
                (action, shortcut)
            })
            .collect();
        ShortcutMap { bindings }
    }

    /// The resolved shortcut for `action`.
    pub fn get(&self, action: ShortcutAction) -> KeyboardShortcut {
        self.bindings[&action]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_default_sequence_parses() {
        for action in ShortcutAction::ALL {
            let sequence = action.default_sequence();
            assert!(
                parse_sequence(sequence).is_some(),
                "default sequence {sequence:?} for {action:?} failed to parse"
            );
        }
    }

    #[test]
    fn garbage_input_is_none() {
        for garbage in ["", "Ctrl+", "+", "Ctrl+Shift+", "Ctrl+Nope", "Banana"] {
            assert!(
                parse_sequence(garbage).is_none(),
                "expected {garbage:?} to fail to parse"
            );
        }
    }

    #[test]
    fn ctrl_maps_to_command_modifier() {
        let shortcut = parse_sequence("Ctrl+R").expect("parses");
        assert_eq!(shortcut.modifiers, Modifiers::COMMAND);
        assert_eq!(shortcut.logical_key, Key::R);
    }

    #[test]
    fn ctrl_shift_left_is_command_shift_arrow_left() {
        let shortcut = parse_sequence("Ctrl+Shift+Left").expect("parses");
        assert_eq!(shortcut.modifiers, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(shortcut.logical_key, Key::ArrowLeft);
    }

    #[test]
    fn ctrl_comma_is_comma_key() {
        let shortcut = parse_sequence("Ctrl+,").expect("parses");
        assert_eq!(shortcut.logical_key, Key::Comma);
        assert_eq!(shortcut.modifiers, Modifiers::COMMAND);
    }

    #[test]
    fn ctrl_return_is_enter_key() {
        let shortcut = parse_sequence("Ctrl+Return").expect("parses");
        assert_eq!(shortcut.logical_key, Key::Enter);
    }

    #[test]
    fn shift_return_has_no_command_modifier() {
        let shortcut = parse_sequence("Shift+Return").expect("parses");
        assert_eq!(shortcut.modifiers, Modifiers::SHIFT);
        assert_eq!(shortcut.logical_key, Key::Enter);
    }

    #[test]
    fn arrow_keys_parse_both_directions() {
        assert_eq!(
            parse_sequence("Ctrl+Left").expect("parses").logical_key,
            Key::ArrowLeft
        );
        assert_eq!(
            parse_sequence("Ctrl+Right").expect("parses").logical_key,
            Key::ArrowRight
        );
    }

    #[test]
    fn ctrl_alt_p_combines_both_modifiers() {
        let shortcut = parse_sequence("Ctrl+Alt+P").expect("parses");
        assert_eq!(shortcut.modifiers, Modifiers::COMMAND | Modifiers::ALT);
        assert_eq!(shortcut.logical_key, Key::P);
    }

    #[test]
    fn settings_key_round_trips_through_from_settings_override() {
        let mut overrides = HashMap::new();
        overrides.insert(
            ShortcutAction::OpenImportWindow.settings_key().to_string(),
            "Ctrl+Shift+I".to_string(),
        );
        let map = ShortcutMap::from_settings(&overrides);
        let shortcut = map.get(ShortcutAction::OpenImportWindow);
        assert_eq!(shortcut.modifiers, Modifiers::COMMAND | Modifiers::SHIFT);
        assert_eq!(shortcut.logical_key, Key::I);
    }

    #[test]
    fn from_settings_without_overrides_uses_defaults_for_every_action() {
        let map = ShortcutMap::from_settings(&HashMap::new());
        for action in ShortcutAction::ALL {
            let expected = parse_sequence(action.default_sequence()).expect("default parses");
            assert_eq!(map.get(action), expected);
        }
    }

    #[test]
    fn unparseable_override_falls_back_to_default() {
        let mut overrides = HashMap::new();
        overrides.insert(
            ShortcutAction::ClearHistory.settings_key().to_string(),
            "not a sequence".to_string(),
        );
        let map = ShortcutMap::from_settings(&overrides);
        let expected = parse_sequence(ShortcutAction::ClearHistory.default_sequence()).unwrap();
        assert_eq!(map.get(ShortcutAction::ClearHistory), expected);
    }

    #[test]
    fn all_twenty_actions_have_distinct_settings_keys() {
        let keys: std::collections::HashSet<_> = ShortcutAction::ALL
            .iter()
            .map(|a| a.settings_key())
            .collect();
        assert_eq!(keys.len(), 20);
    }
}
