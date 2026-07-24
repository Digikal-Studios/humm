//! kittest coverage for the presentation window's own content
//! (`views::presentation::window_ui`, F-065, Task 12) — the second half of
//! `views/presentation.rs` (the embedded recording-window bar,
//! `views::presentation::bar_ui`, is covered by `kittest_recording.rs`
//! instead, since it renders inside `recording_ui`).
//!
//! `window_ui` is a real deferred OS viewport in `app.rs`
//! (`show_presentation_viewport`), unreachable from kittest directly (same
//! constraint `kittest_viewer.rs`/`kittest_recording.rs`'s own module docs
//! explain) — this file drives it directly via `Harness::new_ui_state`,
//! mirroring `app.rs`'s own call shape: re-read the "panel text" (here, a
//! plain fixture `String` standing in for `RecordingVm::panel_text`, the VM
//! seam the real app reads fresh every frame) and re-render wholesale.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use humm_gui::views::presentation::{EffectiveStyle, window_ui};

struct Fixture {
    panel_text: String,
    style: EffectiveStyle,
}

fn fixture(panel_text: &str) -> Fixture {
    Fixture {
        panel_text: panel_text.to_string(),
        style: EffectiveStyle {
            text_color: "#000000".to_string(),
            background_color: "#ffffff".to_string(),
            font_size_pt: 24,
        },
    }
}

fn build_harness(fixture: Fixture) -> Harness<'static, Fixture> {
    Harness::new_ui_state(
        |ui, fixture: &mut Fixture| {
            window_ui(ui, &fixture.panel_text, &fixture.style);
        },
        fixture,
    )
}

#[test]
fn renders_the_given_panel_text() {
    let mut harness = build_harness(fixture("hello from the recording window"));
    harness.step();

    harness.get_by_label("hello from the recording window");
}

#[test]
fn mirrors_the_panel_text_on_every_update() {
    // The VM seam: `app.rs`'s real closure re-reads `RecordingVm::panel_text`
    // fresh every frame and hands it to `window_ui` wholesale (module doc) --
    // this fixture's `panel_text` field stands in for that read.
    let mut harness = build_harness(fixture("first utterance"));
    harness.step();
    harness.get_by_label("first utterance");

    harness.state_mut().panel_text = "first utterance second utterance".to_string();
    harness.step();

    harness.get_by_label("first utterance second utterance");
    assert!(
        harness.query_by_label("first utterance").is_none(),
        "the old text is replaced wholesale, not appended alongside"
    );
}

#[test]
fn renders_with_a_custom_theme_and_large_font_without_panicking() {
    let mut fixture = fixture("styled text");
    fixture.style = EffectiveStyle {
        text_color: "#ff0000".to_string(),
        background_color: "#00ff00".to_string(),
        font_size_pt: 100,
    };
    let mut harness = build_harness(fixture);
    harness.step();

    harness.get_by_label("styled text");
}

#[test]
fn renders_empty_panel_text_without_panicking() {
    let mut harness = build_harness(fixture(""));
    harness.step();
    // No crash, no stale content -- nothing more to assert for an empty panel.
}
