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

    /// The Fluent key for this action's description label (Task 14, design
    /// spec §8.3): the shortcuts-editor tab's row label, resolved via
    /// [`crate::i18n::tr`]. English text is Buzz's own `shortcut.py`
    /// description, byte-for-byte (e.g. `OPEN_RECORD_WINDOW`'s `_("Open
    /// Record Window")`) — the `de`/`es`/`zh_CN` catalogs carry Buzz's own
    /// `.po` translations for the same 20 strings.
    pub fn description_key(self) -> &'static str {
        match self {
            ShortcutAction::OpenRecordWindow => "shortcut-desc-open-record-window",
            ShortcutAction::OpenImportWindow => "shortcut-desc-import-file",
            ShortcutAction::OpenImportUrlWindow => "shortcut-desc-import-url",
            ShortcutAction::OpenPreferencesWindow => "shortcut-desc-open-preferences-window",
            ShortcutAction::ViewTranscriptText => "shortcut-desc-view-transcript-text",
            ShortcutAction::ViewTranscriptTranslation => {
                "shortcut-desc-view-transcript-translation"
            }
            ShortcutAction::ViewTranscriptTimestamps => "shortcut-desc-view-transcript-timestamps",
            ShortcutAction::SearchTranscript => "shortcut-desc-search-transcript",
            ShortcutAction::SearchNext => "shortcut-desc-search-next",
            ShortcutAction::SearchPrevious => "shortcut-desc-search-previous",
            ShortcutAction::ScrollToCurrentText => "shortcut-desc-scroll-to-current-text",
            ShortcutAction::PlayPauseAudio => "shortcut-desc-play-pause-audio",
            ShortcutAction::ReplayCurrentSegment => "shortcut-desc-replay-current-segment",
            ShortcutAction::TogglePlaybackControls => "shortcut-desc-toggle-playback-controls",
            ShortcutAction::DecreaseSegmentStart => "shortcut-desc-decrease-segment-start",
            ShortcutAction::IncreaseSegmentStart => "shortcut-desc-increase-segment-start",
            ShortcutAction::DecreaseSegmentEnd => "shortcut-desc-decrease-segment-end",
            ShortcutAction::IncreaseSegmentEnd => "shortcut-desc-increase-segment-end",
            ShortcutAction::ClearHistory => "shortcut-desc-clear-history",
            ShortcutAction::StopTranscription => "shortcut-desc-stop-transcription",
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
/// Covers the tokens Buzz's 20 default sequences use — `,`, `Return`,
/// `Left`/`Right`, and the ten letters `E F G L O P R S T U X` — plus the
/// digits `0`-`9` (Task 14: the shortcuts-editor capture widget can bind a
/// digit key, so the parser must round-trip whatever it emits even though no
/// *default* sequence uses one). Spelled out explicitly (rather than derived
/// from a single-char-to-`Key` helper) so this stays correct without
/// depending on an `egui::Key` API surface beyond its named constants.
fn parse_key(token: &str) -> Option<Key> {
    match token {
        "," => Some(Key::Comma),
        "Return" => Some(Key::Enter),
        "Left" => Some(Key::ArrowLeft),
        "Right" => Some(Key::ArrowRight),
        "0" => Some(Key::Num0),
        "1" => Some(Key::Num1),
        "2" => Some(Key::Num2),
        "3" => Some(Key::Num3),
        "4" => Some(Key::Num4),
        "5" => Some(Key::Num5),
        "6" => Some(Key::Num6),
        "7" => Some(Key::Num7),
        "8" => Some(Key::Num8),
        "9" => Some(Key::Num9),
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

/// The reverse of [`parse_key`]: the Buzz-notation token for `key`, or
/// `None` if `key` falls outside the vocabulary this module's sequences ever
/// use. `pub(crate)` — the shortcuts-editor capture widget
/// (`views/prefs_shortcuts.rs`, Task 14) reuses it to decide whether a
/// captured keypress is bindable at all (a key with no token here is simply
/// not a candidate for a new binding), and [`serialize_shortcut`] uses it to
/// spell the final (key) token of the string it builds.
pub(crate) fn key_token(key: Key) -> Option<&'static str> {
    Some(match key {
        Key::Comma => ",",
        Key::Enter => "Return",
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        Key::Num0 => "0",
        Key::Num1 => "1",
        Key::Num2 => "2",
        Key::Num3 => "3",
        Key::Num4 => "4",
        Key::Num5 => "5",
        Key::Num6 => "6",
        Key::Num7 => "7",
        Key::Num8 => "8",
        Key::Num9 => "9",
        Key::A => "A",
        Key::B => "B",
        Key::C => "C",
        Key::D => "D",
        Key::E => "E",
        Key::F => "F",
        Key::G => "G",
        Key::H => "H",
        Key::I => "I",
        Key::J => "J",
        Key::K => "K",
        Key::L => "L",
        Key::M => "M",
        Key::N => "N",
        Key::O => "O",
        Key::P => "P",
        Key::Q => "Q",
        Key::R => "R",
        Key::S => "S",
        Key::T => "T",
        Key::U => "U",
        Key::V => "V",
        Key::W => "W",
        Key::X => "X",
        Key::Y => "Y",
        Key::Z => "Z",
        _ => return None,
    })
}

/// Serializes a resolved [`KeyboardShortcut`] back into Buzz's own
/// `"Ctrl+Shift+P"`-style notation, modifiers in Buzz's own declared order
/// (Ctrl, Shift, Alt — checked against `shortcut.py`'s own sequences, e.g.
/// `REPLAY_CURRENT_SEGMENT = "Ctrl+Shift+P"`, `TOGGLE_PLAYBACK_CONTROLS =
/// "Ctrl+Alt+P"`) — the exact inverse of [`parse_sequence`]:
/// `parse_sequence(&serialize_shortcut(s)) == Some(s)` for every `s` built
/// from a [`key_token`]-recognized key (this module's tests cover all 20
/// Buzz defaults plus the digit tokens the shortcuts-editor capture widget
/// also accepts, Task 14).
///
/// A `logical_key` outside [`key_token`]'s vocabulary serializes its final
/// token as a literal `"?"` rather than panicking — never produced by
/// [`ShortcutAction::default_sequence`] or the capture widget (which only
/// ever proposes a [`key_token`]-recognized key), so this is an
/// unreachable-in-practice fallback, not a silent-corruption risk.
pub fn serialize_shortcut(shortcut: &KeyboardShortcut) -> String {
    let mut parts = Vec::new();
    if shortcut.modifiers.command {
        parts.push("Ctrl");
    }
    if shortcut.modifiers.shift {
        parts.push("Shift");
    }
    if shortcut.modifiers.alt {
        parts.push("Alt");
    }
    parts.push(key_token(shortcut.logical_key).unwrap_or("?"));
    parts.join("+")
}

/// Every pair of actions whose resolved [`KeyboardShortcut`] is identical
/// (F-005 conflict detection ✚, design spec §8.3, Task 14) — Buzz's own
/// `shortcuts.py` has no such check (it silently allows two actions to share
/// a sequence); humm surfaces the collision instead of forbidding it (the
/// design spec's own "least surprising superset" — the assignment still
/// stands). Each unordered pair appears exactly once, in
/// [`ShortcutAction::ALL`]'s declared order, and no action is ever paired
/// with itself. `O(n²)` over 20 actions (190 comparisons) — cheap enough to
/// recompute every frame the shortcuts-editor tab is open; no caching
/// needed.
pub fn conflicts(map: &ShortcutMap) -> Vec<(ShortcutAction, ShortcutAction)> {
    let mut found = Vec::new();
    for (i, &a) in ShortcutAction::ALL.iter().enumerate() {
        for &b in &ShortcutAction::ALL[i + 1..] {
            if map.get(a) == map.get(b) {
                found.push((a, b));
            }
        }
    }
    found
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

    // -- description_key (Task 14) -----------------------------------------

    #[test]
    fn all_twenty_actions_have_distinct_description_keys() {
        let keys: std::collections::HashSet<_> = ShortcutAction::ALL
            .iter()
            .map(|a| a.description_key())
            .collect();
        assert_eq!(keys.len(), 20);
    }

    // -- serialize_shortcut (Task 14) ---------------------------------------

    #[test]
    fn every_default_sequence_round_trips_through_serialize() {
        for action in ShortcutAction::ALL {
            let sequence = action.default_sequence();
            let shortcut = parse_sequence(sequence)
                .unwrap_or_else(|| panic!("{action:?}'s default sequence {sequence:?} must parse"));
            assert_eq!(
                serialize_shortcut(&shortcut),
                sequence,
                "serialize_shortcut must reproduce {action:?}'s own default sequence text exactly"
            );
            // The full parse -> serialize -> parse loop must land on the same
            // shortcut, not just the same text.
            let reparsed = parse_sequence(&serialize_shortcut(&shortcut))
                .expect("a just-serialized default sequence must re-parse");
            assert_eq!(reparsed, shortcut);
        }
    }

    #[test]
    fn digit_tokens_parse_and_round_trip() {
        let digits = [
            ("0", Key::Num0),
            ("1", Key::Num1),
            ("2", Key::Num2),
            ("3", Key::Num3),
            ("4", Key::Num4),
            ("5", Key::Num5),
            ("6", Key::Num6),
            ("7", Key::Num7),
            ("8", Key::Num8),
            ("9", Key::Num9),
        ];
        for (token, key) in digits {
            let sequence = format!("Ctrl+{token}");
            let shortcut = parse_sequence(&sequence)
                .unwrap_or_else(|| panic!("{sequence:?} must parse now that digits are supported"));
            assert_eq!(shortcut.logical_key, key);
            assert_eq!(
                serialize_shortcut(&shortcut),
                sequence,
                "a digit shortcut must serialize back to the same text it was parsed from"
            );
        }
    }

    // -- conflicts (Task 14) -------------------------------------------------

    #[test]
    fn defaults_are_conflict_free() {
        let map = ShortcutMap::from_settings(&HashMap::new());
        assert!(
            conflicts(&map).is_empty(),
            "Buzz's 20 default sequences must never collide with each other"
        );
    }

    #[test]
    fn conflicts_detects_an_injected_pair_exactly_once_and_never_self_pairs() {
        let mut overrides = HashMap::new();
        // Force OpenImportWindow onto ClearHistory's default sequence.
        overrides.insert(
            ShortcutAction::OpenImportWindow.settings_key().to_string(),
            ShortcutAction::ClearHistory.default_sequence().to_string(),
        );
        let map = ShortcutMap::from_settings(&overrides);

        let found = conflicts(&map);

        assert_eq!(
            found,
            vec![(
                ShortcutAction::OpenImportWindow,
                ShortcutAction::ClearHistory
            )],
            "exactly one pair, in ShortcutAction::ALL order, no duplicates"
        );
        assert!(
            found.iter().all(|(a, b)| a != b),
            "no action may ever be paired with itself"
        );
    }
}
