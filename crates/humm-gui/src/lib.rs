//! humm-gui: the eframe-based desktop shell (ADR-006).
//!
//! Structured as a library-plus-binary crate (`src/lib.rs` here, the real
//! entry point in `src/main.rs`) so `tests/kittest_smoke.rs` — an
//! `egui_kittest` harness that must construct a real [`app::HummApp`]
//! in-process — can `use humm_gui::{app::HummApp, services};` the same way
//! `main.rs` does, rather than driving the compiled binary as a subprocess
//! (`humm-cli`'s black-box style, which doesn't fit an in-process GUI
//! harness).

pub mod app;
pub mod engine_factory;
pub mod events;
pub mod format;
pub mod i18n;
pub mod import;
pub mod live_fixture;
pub mod live_session;
pub mod logging;
pub mod models_ui;
pub mod recording;
pub mod services;
pub mod shortcuts;
pub mod viewer;
pub mod viewmodel;
pub mod views;
