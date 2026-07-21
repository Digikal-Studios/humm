//! The whisper language table.
//!
//! Ported verbatim (code + English name) from Buzz's `LANGUAGES` dict
//! (`buzz/transcriber/transcriber.py:37-138`). The Buzz source table has
//! **100** entries, not the 99 the task brief's line-range citation implied;
//! this file follows the source (Global Constraint 3) and the discrepancy is
//! reported alongside this task's other findings.

use serde::{Deserialize, Serialize};

/// A whisper-supported language: an ISO 639-1 (mostly) code plus its English
/// display name, taken from Buzz's language table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Language(usize);

/// `(code, english_name)` pairs, in Buzz's source order.
const LANGUAGES: &[(&str, &str)] = &[
    ("en", "English"),
    ("zh", "Chinese"),
    ("de", "German"),
    ("es", "Spanish"),
    ("ru", "Russian"),
    ("ko", "Korean"),
    ("fr", "French"),
    ("ja", "Japanese"),
    ("pt", "Portuguese"),
    ("tr", "Turkish"),
    ("pl", "Polish"),
    ("ca", "Catalan"),
    ("nl", "Dutch"),
    ("ar", "Arabic"),
    ("sv", "Swedish"),
    ("it", "Italian"),
    ("id", "Indonesian"),
    ("hi", "Hindi"),
    ("fi", "Finnish"),
    ("vi", "Vietnamese"),
    ("he", "Hebrew"),
    ("uk", "Ukrainian"),
    ("el", "Greek"),
    ("ms", "Malay"),
    ("cs", "Czech"),
    ("ro", "Romanian"),
    ("da", "Danish"),
    ("hu", "Hungarian"),
    ("ta", "Tamil"),
    ("no", "Norwegian"),
    ("th", "Thai"),
    ("ur", "Urdu"),
    ("hr", "Croatian"),
    ("bg", "Bulgarian"),
    ("lt", "Lithuanian"),
    ("la", "Latin"),
    ("mi", "Maori"),
    ("ml", "Malayalam"),
    ("cy", "Welsh"),
    ("sk", "Slovak"),
    ("te", "Telugu"),
    ("fa", "Persian"),
    ("lv", "Latvian"),
    ("bn", "Bengali"),
    ("sr", "Serbian"),
    ("az", "Azerbaijani"),
    ("sl", "Slovenian"),
    ("kn", "Kannada"),
    ("et", "Estonian"),
    ("mk", "Macedonian"),
    ("br", "Breton"),
    ("eu", "Basque"),
    ("is", "Icelandic"),
    ("hy", "Armenian"),
    ("ne", "Nepali"),
    ("mn", "Mongolian"),
    ("bs", "Bosnian"),
    ("kk", "Kazakh"),
    ("sq", "Albanian"),
    ("sw", "Swahili"),
    ("gl", "Galician"),
    ("mr", "Marathi"),
    ("pa", "Punjabi"),
    ("si", "Sinhala"),
    ("km", "Khmer"),
    ("sn", "Shona"),
    ("yo", "Yoruba"),
    ("so", "Somali"),
    ("af", "Afrikaans"),
    ("oc", "Occitan"),
    ("ka", "Georgian"),
    ("be", "Belarusian"),
    ("tg", "Tajik"),
    ("sd", "Sindhi"),
    ("gu", "Gujarati"),
    ("am", "Amharic"),
    ("yi", "Yiddish"),
    ("lo", "Lao"),
    ("uz", "Uzbek"),
    ("fo", "Faroese"),
    ("ht", "Haitian Creole"),
    ("ps", "Pashto"),
    ("tk", "Turkmen"),
    ("nn", "Nynorsk"),
    ("mt", "Maltese"),
    ("sa", "Sanskrit"),
    ("lb", "Luxembourgish"),
    ("my", "Myanmar"),
    ("bo", "Tibetan"),
    ("tl", "Tagalog"),
    ("mg", "Malagasy"),
    ("as", "Assamese"),
    ("tt", "Tatar"),
    ("haw", "Hawaiian"),
    ("ln", "Lingala"),
    ("ha", "Hausa"),
    ("ba", "Bashkir"),
    ("jw", "Javanese"),
    ("su", "Sundanese"),
    ("yue", "Cantonese"),
];

const fn build_all() -> [Language; LANGUAGES.len()] {
    let mut all = [Language(0); LANGUAGES.len()];
    let mut i = 0;
    while i < LANGUAGES.len() {
        all[i] = Language(i);
        i += 1;
    }
    all
}

static ALL: [Language; LANGUAGES.len()] = build_all();

impl Language {
    /// The language's code, as used in Buzz's `LANGUAGES` table (mostly
    /// ISO 639-1, with a few ISO 639-2/3 codes for languages lacking a
    /// 639-1 entry, e.g. `"haw"` Hawaiian, `"yue"` Cantonese).
    pub fn code(self) -> &'static str {
        LANGUAGES[self.0].0
    }

    /// The language's English display name.
    pub fn english_name(self) -> &'static str {
        LANGUAGES[self.0].1
    }

    /// Looks up a language by its code (case-sensitive, exact match).
    pub fn parse(code: &str) -> Option<Language> {
        LANGUAGES
            .iter()
            .position(|(candidate, _)| *candidate == code)
            .map(Language)
    }

    /// All 100 supported languages, in Buzz's source order.
    pub fn all() -> &'static [Language] {
        &ALL
    }
}

impl Serialize for Language {
    /// Serializes as the bare code string, e.g. `"fr"`.
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.code())
    }
}

impl<'de> Deserialize<'de> for Language {
    /// Deserializes from the bare code string; an unrecognized code fails
    /// with a message naming it.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let code = String::deserialize(deserializer)?;
        Language::parse(&code)
            .ok_or_else(|| serde::de::Error::custom(format!("unknown language code: {code:?}")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn all_has_every_buzz_language() {
        assert_eq!(Language::all().len(), 100);
    }

    #[test]
    fn parse_english_by_code() {
        assert_eq!(Language::parse("en").unwrap().english_name(), "English");
    }

    #[test]
    fn parse_hawaiian_by_code() {
        let hawaiian = Language::parse("haw").expect("haw should parse");
        assert_eq!(hawaiian.code(), "haw");
        assert_eq!(hawaiian.english_name(), "Hawaiian");
    }

    #[test]
    fn parse_unknown_code_is_none() {
        assert!(Language::parse("xx").is_none());
    }

    #[test]
    fn every_language_code_round_trips_through_parse() {
        for language in Language::all() {
            let code = language.code();
            let reparsed = Language::parse(code).unwrap_or_else(|| panic!("{code} should parse"));
            assert_eq!(reparsed.code(), code);
        }
    }

    #[test]
    fn cantonese_is_last_entry_ported_verbatim() {
        let cantonese = Language::parse("yue").expect("yue should parse");
        assert_eq!(cantonese.english_name(), "Cantonese");
    }

    #[test]
    fn language_serializes_as_bare_code_string() {
        let french = Language::parse("fr").unwrap();
        assert_eq!(serde_json::to_string(&french).unwrap(), "\"fr\"");
    }

    #[test]
    fn language_round_trips_through_json() {
        let french = Language::parse("fr").unwrap();
        let json = serde_json::to_string(&french).unwrap();
        let round_tripped: Language = serde_json::from_str(&json).unwrap();
        assert_eq!(round_tripped.code(), french.code());
    }

    #[test]
    fn language_deserialize_unknown_code_names_it_in_error() {
        let err = serde_json::from_str::<Language>("\"xx\"").unwrap_err();
        assert!(
            err.to_string().contains("xx"),
            "error should name the unknown code: {err}"
        );
    }
}
