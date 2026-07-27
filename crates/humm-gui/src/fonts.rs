//! CJK-capable fallback font (F-008 gate, design spec §7.1): egui's built-in
//! default fonts carry no Han/Kana glyphs, so any locale that needs them —
//! `zh_CN`, `zh_TW`, `ja_JP` — renders as tofu (`□`). This also affects
//! Buzz's own default sentence-terminator punctuation (`。？！`), which is
//! baked into segment-splitting logic and therefore shows up under *any*
//! locale, not just a CJK one.
//!
//! Bundled font: **Noto Sans SC** (Simplified-Chinese cut), static Regular
//! weight, OFL-1.1 — `assets/fonts/NotoSansSC-Regular.otf`. The plan named
//! this file `NotoSansSC-Regular.ttf`; the artifact actually bundled is the
//! notofonts release's static **OTF** subset cut instead (source option 1
//! of the two the task brief allows) — the only single-file, single-weight
//! *static* artifact (design spec §7.1: "static Regular") the upstream
//! release publishes standalone, without unpacking a multi-hundred-MB
//! all-languages zip. License and glyph coverage are identical either way;
//! the family key stays the plan's `"noto-sc"`.
//!
//! Source: <https://raw.githubusercontent.com/notofonts/noto-cjk/Sans2.004/Sans/SubsetOTF/SC/NotoSansSC-Regular.otf>
//! (`notofonts/noto-cjk`, release tag `Sans2.004`, path
//! `Sans/SubsetOTF/SC/NotoSansSC-Regular.otf`; fetched 2026-07-26) — the URL
//! pins the tag, not `main`, so this provenance record for a vendored binary
//! stays accurate even after upstream's `main` moves on.
//! SHA-256: `faa6c9df652116dde789d351359f3d7e5d2285a2b2a1f04a2d7244df706d5ea9`
//! (8,331,336 bytes). License: SIL Open Font License 1.1, committed
//! alongside as `assets/fonts/OFL.txt` (that same release's `Sans/LICENSE`).
//!
//! Coverage: CJK Unified Ideographs (Han), Hiragana, Katakana, and CJK
//! punctuation (`。？！`). Documented divergence (ledger #8): Noto's
//! per-region cuts (JP/KR/SC/TC/HK) share one glyph repertoire and differ
//! only in which *default* glyph shape a shared codepoint resolves to —
//! bundling only the SC cut means Traditional-Chinese and Japanese text
//! renders with Simplified-preferred Han shapes (Buzz-on-Qt instead asked
//! the OS for a region-correct system font per locale). Every codepoint
//! this module tests still resolves to a real glyph, just not always the
//! regionally "native" shape.
//!
//! [`install`] appends the font as the **last** fallback (never first —
//! Latin glyphs must keep resolving to egui's own compact default fonts
//! first) on both the `Proportional` and `Monospace` families, then calls
//! `ctx.set_fonts` once. Called from `HummApp::new`, before the first frame
//! paints — the same "resolved once, before first paint" contract that
//! function's locale negotiation and F-100 font-size application already
//! follow immediately below this call site.

use std::sync::Arc;

use egui::{FontData, FontDefinitions, FontFamily};

/// The family key the plan specifies for the bundled fallback font.
const NOTO_SC_KEY: &str = "noto-sc";

/// Bundled Noto Sans SC bytes. `include_bytes!` rather than a runtime file
/// read: the font never changes after a build, so it belongs in the binary
/// the same way the compiled-in `.ftl` catalogs (`i18n` module) do — no
/// runtime file dependency, no missing-file failure mode to handle.
const NOTO_SC_BYTES: &[u8] = include_bytes!("../assets/fonts/NotoSansSC-Regular.otf");

/// Builds the [`FontDefinitions`] [`install`] applies: egui's own defaults
/// (`FontDefinitions::default()`), plus Noto Sans SC inserted under the
/// `"noto-sc"` key and appended — never prepended — to both
/// `Proportional`'s and `Monospace`'s fallback chains, so a Latin glyph
/// always resolves through egui's own fonts first and CJK glyphs fall
/// through to this one only when nothing earlier in the chain has them.
pub fn definitions() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();
    fonts.font_data.insert(
        NOTO_SC_KEY.to_owned(),
        Arc::new(FontData::from_static(NOTO_SC_BYTES)),
    );

    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        fonts
            .families
            .entry(family)
            .or_default()
            .push(NOTO_SC_KEY.to_owned());
    }

    fonts
}

/// Installs [`definitions`] on `ctx`. Called exactly once, from
/// `HummApp::new`.
pub fn install(ctx: &egui::Context) {
    ctx.set_fonts(definitions());
}

#[cfg(test)]
mod tests {
    use ab_glyph::{Font, FontRef};

    use super::*;

    /// The RED-first test (design spec §7.1 verification method): parses the
    /// bundled bytes independent of egui's own font stack and checks a
    /// resolvable glyph (id != 0, i.e. not `.notdef`) exists for one sample
    /// character from each CJK catalog need the spec names: zh_CN, zh_TW,
    /// ja_JP (kanji + hiragana + katakana), and Buzz's default punctuation.
    #[test]
    fn cjk_glyphs_resolve() {
        let font = FontRef::try_from_slice(NOTO_SC_BYTES).expect("bundled font must parse");
        for c in ['转', '轉', '語', 'ひ', 'カ', '。', '？', '！'] {
            assert_ne!(
                font.glyph_id(c).0,
                0,
                "expected a resolvable glyph for {c:?}, got .notdef (glyph id 0)"
            );
        }
    }

    /// `definitions()` places `"noto-sc"` last (never first) in both
    /// families' fallback chains.
    #[test]
    fn font_definitions_contain_noto_fallback() {
        let fonts = definitions();

        assert!(
            fonts.font_data.contains_key(NOTO_SC_KEY),
            "font_data must carry the noto-sc entry"
        );

        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let chain = fonts
                .families
                .get(&family)
                .unwrap_or_else(|| panic!("{family:?} must have a fallback chain"));
            assert_eq!(
                chain.last().map(String::as_str),
                Some(NOTO_SC_KEY),
                "{family:?} must end with the noto-sc fallback, got {chain:?}"
            );
        }
    }
}
