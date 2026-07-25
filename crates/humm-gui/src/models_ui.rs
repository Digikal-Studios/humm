//! Preferences → Models tab (F-044 basic): a flat list of every ggml size
//! Buzz's own group (`docs/design/ARCHITECTURE.md`'s single whisper.cpp
//! backend, ADR-008 — no group combo) can hold, split into "Downloaded" /
//! "Available for Download" sections, with Download/Delete/Show-file-
//! location actions — ported from `models_preferences_widget.py`'s
//! `QTreeWidget` (buzz-notes-import-progress-models.md §F-044).
//!
//! State/actions are plain (`egui`-light) data so [`ModelsTabState`]/
//! [`refresh`]/[`poll_downloads`]/[`delete`] are unit-testable against a
//! temp directory with no UI context at all, and [`ui`] itself is drivable
//! inside an ordinary `egui::CentralPanel` in a kittest harness (it needs no
//! OS viewport — only `app.rs`'s wrapping call does).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use humm_core::cancel::CancelToken;
use humm_core::domain::ModelSize;
use humm_core::error::ModelError;
use humm_core::models::download::{DownloadProgress, ModelDownloader};
use humm_core::models::registry::{
    SENTINEL_SUFFIX, coreml_encoder_dir, coreml_no_encoder_marker, ggml_file_name, local_model_path,
};
use humm_core::settings::Settings;

use crate::i18n::{tr, tr_args};
use crate::views::options_sheet::{selectable_model_sizes, title_case};

/// Live progress/outcome of one in-flight download, shared between the
/// background thread ([`start_download`]) and the tab's per-frame poll
/// ([`poll_downloads`]).
#[derive(Debug, Clone, Default)]
pub struct DownloadStatus {
    /// Bytes downloaded so far.
    pub downloaded: u64,
    /// Total artifact size, once known from the response headers.
    pub total: Option<u64>,
    /// `None` while still running; `Some` once the thread has finished,
    /// carrying either success or the download's error text
    /// ([`ModelError::to_string`] — [`poll_downloads`] tells a genuine
    /// failure apart from a user cancel via the sibling [`CancelToken`], not
    /// by inspecting this string).
    pub finished: Option<Result<(), String>>,
}

impl DownloadStatus {
    /// The `[0.0, 1.0]` fraction for a determinate progress bar, or `None`
    /// while the total size isn't known yet (an indeterminate spinner is
    /// the caller's job in that case).
    pub fn fraction(&self) -> Option<f32> {
        let total = self.total?;
        if total == 0 {
            return None;
        }
        Some(self.downloaded as f32 / total as f32)
    }
}

/// One row's on-disk/in-flight state.
#[derive(Debug, Clone)]
pub enum RowState {
    /// Fully downloaded (ggml + its `.humm_complete` sentinel both present)
    /// — the `u64` is the ggml file's size in bytes
    /// (`std::fs::metadata::len`).
    Downloaded(u64),
    /// Not downloaded (or a previous attempt never completed).
    Available,
    /// A download is in flight.
    Downloading {
        /// Shared with the background thread; polled each frame.
        status: Arc<Mutex<DownloadStatus>>,
        /// Cancels the download; also lets [`poll_downloads`] tell a
        /// user-initiated cancel apart from a genuine failure.
        cancel: CancelToken,
    },
}

/// One row of the Models tab.
#[derive(Debug, Clone)]
pub struct ModelRow {
    /// The size this row represents (`ModelSize::Custom` for the
    /// custom-URL row).
    pub size: ModelSize,
    /// Its current on-disk/in-flight state.
    pub state: RowState,
}

/// Scans `root` and builds one [`ModelRow`] per size
/// [`selectable_model_sizes`] lists (the 12 ggml sizes), plus a trailing
/// `Custom` row iff `custom_url` is `Some` (same gate the options sheet's
/// model-size combo uses — `settings.models.custom_model_url.is_some()`),
/// plus a trailing `Lumii` row iff `include_lumii` (F-044/J-9, design spec
/// §8.2 — same locale gate as every other model-size combo,
/// [`crate::i18n::lumii_locale_requested`]). A listed `Lumii` row behaves
/// exactly like any other size row: download/delete/reveal all work, since
/// [`ModelDownloader::resolve_model`] already has a `Lumii` arm (F-042, the
/// CLI's `-s lumii` already exercises it).
///
/// A row already mid-download is *not* something this function can see or
/// preserve — it always returns `Downloaded`/`Available` rows, computed
/// fresh from disk. A caller that has live `Downloading` rows to keep
/// splices this function's result in only for the rows it isn't already
/// tracking in flight (see [`ModelsTabState::refresh_from_disk`]).
pub fn refresh(root: &Path, custom_url: Option<&str>, include_lumii: bool) -> Vec<ModelRow> {
    selectable_model_sizes(custom_url.is_some(), include_lumii)
        .into_iter()
        .map(|size| ModelRow {
            size,
            state: row_state_for(root, size),
        })
        .collect()
}

/// The disk-only state (`Downloaded`/`Available`) for one size — the shared
/// core of [`refresh`] and [`poll_downloads`]'s finalize step.
fn row_state_for(root: &Path, size: ModelSize) -> RowState {
    match local_model_path(root, size) {
        Some(path) => {
            let bytes = std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
            RowState::Downloaded(bytes)
        }
        None => RowState::Available,
    }
}

/// The `<file>.humm_complete` sentinel path for a ggml artifact. Duplicates
/// (rather than calls) `humm_core::models::registry`'s own
/// `sentinel_path_for` — that helper is `pub(crate)` to `humm-core`, not
/// visible here — but is otherwise byte-identical to it, built from the
/// same public [`SENTINEL_SUFFIX`] constant so the two can never drift on
/// the suffix itself.
fn sentinel_path(ggml_path: &Path) -> PathBuf {
    let mut name = ggml_path
        .file_name()
        .map(std::ffi::OsStr::to_os_string)
        .unwrap_or_default();
    name.push(SENTINEL_SUFFIX);
    ggml_path.with_file_name(name)
}

/// Deletes `size`'s cached model entirely: the ggml file, its sentinel, the
/// CoreML encoder directory (recursively, if present), and the CoreML
/// negative-cache marker (if present) — Buzz's `models_preferences_widget.py`
/// delete flow, minus its HF-hub-cache-layout arm (humm's cache is always
/// flat, `docs/design/ARCHITECTURE.md` §5). Every removal tolerates the
/// target already being absent (a partial prior delete, or a row that was
/// never fully downloaded) rather than erroring on it — only a genuine I/O
/// failure (permissions, disk in a bad state) propagates.
pub fn delete(root: &Path, size: ModelSize) -> std::io::Result<()> {
    let ggml_path = root.join(ggml_file_name(size));
    remove_file_if_exists(&ggml_path)?;
    remove_file_if_exists(&sentinel_path(&ggml_path))?;

    let encoder_dir = coreml_encoder_dir(root, size);
    if encoder_dir.is_dir() {
        std::fs::remove_dir_all(&encoder_dir)?;
    }
    remove_file_if_exists(&coreml_no_encoder_marker(root, size))?;
    Ok(())
}

fn remove_file_if_exists(path: &Path) -> std::io::Result<()> {
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(err) => Err(err),
    }
}

/// Spawns `size`'s download on a background thread via
/// [`ModelDownloader::resolve_model`] (or, for `ModelSize::Custom`,
/// [`ModelDownloader::ensure_custom_model`] against `custom_url`), returning
/// the `Downloading` [`RowState`] to install immediately (before the thread
/// has produced any progress at all) so the row flips to its in-flight
/// look on the very next frame.
///
/// `ctx` is cloned into the thread so its progress callback (and its
/// completion) can call [`egui::Context::request_repaint`] — otherwise a
/// finished/updated download would sit invisible until some unrelated input
/// happened to trigger a repaint. CoreML is requested exactly when the real
/// task-queue downloader would request it
/// (`cfg!(all(target_os = "macos", target_arch = "aarch64"))` — mirrors
/// `services::init`'s `DownloaderEnsurer::coreml`), so a Preferences-tab
/// download and an in-task download of the same size leave the cache in the
/// identical state.
pub fn start_download(
    root: PathBuf,
    size: ModelSize,
    custom_url: Option<String>,
    ctx: egui::Context,
) -> RowState {
    let coreml = cfg!(all(target_os = "macos", target_arch = "aarch64"));
    let status = Arc::new(Mutex::new(DownloadStatus::default()));
    let cancel = CancelToken::new();

    let status_for_thread = Arc::clone(&status);
    let cancel_for_thread = cancel.clone();
    std::thread::spawn(move || {
        let downloader = ModelDownloader::new(root);

        let status_for_progress = Arc::clone(&status_for_thread);
        let ctx_for_progress = ctx.clone();
        let mut on_progress = move |update: DownloadProgress| {
            if let Ok(mut guard) = status_for_progress.lock() {
                guard.downloaded = update.downloaded;
                guard.total = update.total;
            }
            ctx_for_progress.request_repaint();
        };

        let result: Result<(), ModelError> = if size == ModelSize::Custom {
            match custom_url.as_deref() {
                Some(url) => downloader
                    .ensure_custom_model(url, &mut on_progress, &cancel_for_thread)
                    .map(|_path| ()),
                None => Err(ModelError::Unsupported {
                    detail: "no custom model URL configured — set it in the Models tab".to_string(),
                }),
            }
        } else {
            downloader
                .resolve_model(size, coreml, &mut on_progress, &cancel_for_thread)
                .map(|_resolved| ())
        };

        if let Ok(mut guard) = status_for_thread.lock() {
            guard.finished = Some(result.map_err(|err| err.to_string()));
        }
        ctx.request_repaint();
    });

    RowState::Downloading { status, cancel }
}

/// Polls every `Downloading` row for completion and finalizes it in place:
/// success re-stats the file into `Downloaded`; a genuine failure reports
/// `"{models-download-failed}: {detail}"` via `on_error` (the tab's "Error"
/// modal — Buzz: title "Error", body "Download failed: {err}") and reverts
/// to `Available`; a user-initiated cancel (`cancel.is_canceled()`) also
/// reverts to `Available`, silently — Buzz's own `cancel()` has no error
/// dialog either. Call once per frame, before rendering rows.
pub fn poll_downloads(root: &Path, rows: &mut [ModelRow], on_error: &mut dyn FnMut(String)) {
    for row in rows.iter_mut() {
        let (status, cancel) = match &row.state {
            RowState::Downloading { status, cancel } => (Arc::clone(status), cancel.clone()),
            RowState::Downloaded(_) | RowState::Available => continue,
        };
        let finished = {
            let guard = status
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.finished.clone()
        };
        let Some(outcome) = finished else {
            continue;
        };
        row.state = match outcome {
            Ok(()) => row_state_for(root, row.size),
            Err(_) if cancel.is_canceled() => RowState::Available,
            Err(detail) => {
                on_error(format!("{}: {detail}", tr("models-download-failed")));
                RowState::Available
            }
        };
    }
}

/// The Models tab's full live state, owned by [`crate::views::preferences::PreferencesState`].
#[derive(Debug)]
pub struct ModelsTabState {
    /// Every row, `Downloaded`/`Available` sizes plus `Custom` when
    /// configured — refreshed from disk on open and after every
    /// download/delete.
    pub rows: Vec<ModelRow>,
    /// The custom-ggml-URL text field's live buffer (mirrors
    /// `settings.models.custom_model_url`, empty when unset).
    pub custom_url: String,
    /// A pending "Download failed: …" message, shown as an "Error" modal
    /// (Buzz parity) until dismissed.
    pub error: Option<String>,
    /// The size a Delete click wants to confirm, if the confirm modal is
    /// open.
    pub delete_confirm: Option<ModelSize>,
    /// Whether the `Lumii` row is included (F-044/J-9, design spec §8.2) —
    /// snapshotted once at [`ModelsTabState::new`] time (the tab has no
    /// locale picker of its own to react to; `PreferencesState::new`'s own
    /// contract already rebuilds this whole tab fresh on every Preferences
    /// window (re)open, which is when a locale change made elsewhere would
    /// take effect here too). Carried across [`ModelsTabState::refresh_from_disk`]
    /// calls so a custom-URL edit or a delete doesn't silently drop the row.
    pub include_lumii: bool,
}

impl ModelsTabState {
    /// Builds the tab's initial state: `rows` from [`refresh`], `custom_url`
    /// from the current setting, `include_lumii` from the caller's
    /// locale-derived gate (see [`crate::i18n::lumii_locale_requested`]).
    pub fn new(root: &Path, custom_url: Option<&str>, include_lumii: bool) -> Self {
        ModelsTabState {
            rows: refresh(root, custom_url, include_lumii),
            custom_url: custom_url.unwrap_or_default().to_string(),
            error: None,
            delete_confirm: None,
            include_lumii,
        }
    }

    /// Re-scans disk state, preserving any row currently `Downloading` (a
    /// plain [`refresh`] would otherwise report it as `Available` — it has
    /// no on-disk artifact yet). Called after the custom-URL field changes
    /// (a fresh `Custom` row may need to appear/disappear) and is safe to
    /// call any time otherwise.
    pub fn refresh_from_disk(&mut self, root: &Path) {
        let custom_url = non_empty(&self.custom_url);
        let mut fresh = refresh(root, custom_url.as_deref(), self.include_lumii);
        for row in &mut fresh {
            if let Some(existing) = self
                .rows
                .iter()
                .find(|r| r.size == row.size && matches!(r.state, RowState::Downloading { .. }))
            {
                row.state = existing.state.clone();
            }
        }
        self.rows = fresh;
    }
}

fn non_empty(s: &str) -> Option<String> {
    let trimmed = s.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

/// One row's clicked action, collected during [`ui`]'s render pass and
/// applied afterward (same accumulate-then-apply shape as
/// `main_window.rs`'s `TableAction`s) — a click handler mid-render can't
/// itself spawn a thread or mutate `state.rows` while `state.rows` is still
/// being iterated for rendering.
enum RowAction {
    Download(ModelSize),
    Cancel(CancelToken),
    Delete(ModelSize),
    Reveal,
}

/// The measured-speed ratio for `size`'s row, if `measured_speed` (a
/// snapshot of `settings.recording.measured_speed`, M5 design spec §2.8) has
/// an entry keyed by its exact wire string — `None` for a fresh install (the
/// map is empty) and, just as importantly, for any *other* key the map might
/// carry: a future backend-qualified entry (e.g. an `openai_api`-prefixed
/// key, Task 18) simply never equals any [`ModelSize::as_str`] value, so it
/// is skipped by construction rather than needing an explicit exclusion —
/// see `non_size_keys_ignored` below.
fn measured_ratio_for(measured_speed: &BTreeMap<String, f32>, size: ModelSize) -> Option<f32> {
    measured_speed.get(size.as_str()).copied()
}

/// The measured-speed badge's own display text (J-9 ✚, design spec §8.2):
/// `"{ratio:.1}× realtime"`, localized via the `models-speed-badge` Fluent
/// key. The `.1`-precision formatting happens here, in Rust, before the
/// value ever reaches Fluent — passed through as a preformatted string arg
/// rather than a numeric placeable, so this function (not each locale's
/// number-formatting rules) controls the precision.
fn speed_badge_text(ratio: f32) -> String {
    let ratio_text = format!("{ratio:.1}");
    tr_args("models-speed-badge", &[("ratio", ratio_text.as_str())])
}

/// Renders one row: its title-cased name, the measured-speed badge (J-9 ✚,
/// design spec §8.2 — only when `measured_speed` has an entry for this row's
/// size; never rendered otherwise), a size/progress readout, and its
/// Buzz-parity action buttons — `[Download]` while `Available`, a progress
/// bar + `[Cancel]` while `Downloading`, `[Delete]` + `[Show file location]`
/// while `Downloaded` (`models_preferences_widget.py`'s visibility rules).
fn row_ui(
    ui: &mut egui::Ui,
    row: &ModelRow,
    measured_speed: &BTreeMap<String, f32>,
    actions: &mut Vec<RowAction>,
) {
    ui.horizontal(|ui| {
        ui.label(title_case(row.size.as_str()));
        if let Some(ratio) = measured_ratio_for(measured_speed, row.size) {
            ui.label(speed_badge_text(ratio))
                .on_hover_text(tr("models-speed-badge-tip"));
        }
        match &row.state {
            RowState::Downloaded(bytes) => {
                ui.label(format_bytes(*bytes));
                if ui.button(tr("models-delete-button")).clicked() {
                    actions.push(RowAction::Delete(row.size));
                }
                if ui.button(tr("models-show-location-button")).clicked() {
                    actions.push(RowAction::Reveal);
                }
            }
            RowState::Available => {
                if ui.button(tr("models-download-button")).clicked() {
                    actions.push(RowAction::Download(row.size));
                }
            }
            RowState::Downloading { status, cancel } => {
                let snapshot = status.lock().map(|guard| guard.clone()).unwrap_or_default();
                match snapshot.fraction() {
                    Some(fraction) => {
                        ui.add(egui::ProgressBar::new(fraction).show_percentage());
                    }
                    None => {
                        ui.add(egui::ProgressBar::new(0.0).animate(true));
                    }
                }
                if ui.button(tr("models-cancel-button")).clicked() {
                    actions.push(RowAction::Cancel(cancel.clone()));
                }
            }
        }
    });
}

/// Bytes formatted as `"N.N MB"`/`"N.N GB"` (whisper ggml files are always
/// comfortably in that range — no need for a general byte-unit ladder).
fn format_bytes(bytes: u64) -> String {
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = MB * 1024.0;
    let bytes = bytes as f64;
    if bytes >= GB {
        format!("{:.1} GB", bytes / GB)
    } else {
        format!("{:.1} MB", bytes / MB)
    }
}

/// Renders the full Models tab (sections, rows, custom-URL field, and the
/// delete-confirm/error modals) and applies whatever the frame's clicks
/// produced: a Download spawns [`start_download`]; a Cancel calls
/// `CancelToken::cancel`; a confirmed Delete calls [`delete`] and refreshes;
/// Reveal calls `open::that(root)` (the flat cache root — every model's
/// "parent" either way, `docs/design/ARCHITECTURE.md` §5). The custom-URL
/// field persists to `settings.models.custom_model_url` on focus-loss only
/// (carried M3 ticket, spec §8.3 — not per keystroke, which would otherwise
/// hit disk on every character typed); egui's single-line `TextEdit`
/// surrenders focus on Enter, so pressing Enter also commits, blank meaning
/// `None` (binding decision). Also reads `settings.recording.measured_speed`
/// fresh every frame (J-9 ✚, design spec §8.2) for the per-row speed badge —
/// a cheap, read-only snapshot clone, same "read live settings straight from
/// the already-threaded `Arc<Mutex<Settings>>`" convention every other
/// per-frame read in this function already uses.
pub fn ui(
    ui: &mut egui::Ui,
    state: &mut ModelsTabState,
    root: &Path,
    settings: &Arc<Mutex<Settings>>,
    settings_path: &Path,
) {
    {
        let rows = &mut state.rows;
        let error = &mut state.error;
        poll_downloads(root, rows, &mut |message| *error = Some(message));
    }

    let measured_speed = {
        let guard = settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.recording.measured_speed.clone()
    };

    let mut actions: Vec<RowAction> = Vec::new();

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(tr("models-section-downloaded"));
        for row in state
            .rows
            .iter()
            .filter(|row| matches!(row.state, RowState::Downloaded(_)))
        {
            row_ui(ui, row, &measured_speed, &mut actions);
        }

        ui.separator();
        ui.label(tr("models-section-available"));
        for row in state
            .rows
            .iter()
            .filter(|row| !matches!(row.state, RowState::Downloaded(_)))
        {
            row_ui(ui, row, &measured_speed, &mut actions);
        }
    });

    ui.separator();
    let mut custom_url_committed = false;
    ui.horizontal(|ui| {
        ui.label(tr("models-custom-url-label"));
        let response = ui.text_edit_singleline(&mut state.custom_url);
        // `lost_focus()` alone (not `changed()`): persist once the field is
        // actually committed, not on every keystroke. Covers Enter too —
        // egui's single-line `TextEdit` surrenders focus (so `lost_focus()`
        // fires) when Enter is pressed, rather than inserting a newline.
        if response.lost_focus() {
            custom_url_committed = true;
        }
    });
    if custom_url_committed {
        let value = non_empty(&state.custom_url);
        let mut guard = settings
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        guard.models.custom_model_url = value;
        if let Err(err) = guard.save(settings_path) {
            log::warn!("could not persist the custom model URL: {err}");
        }
        drop(guard);
        state.refresh_from_disk(root);
    }

    let mut needs_refresh = false;
    for action in actions {
        match action {
            RowAction::Download(size) => {
                let custom_url = non_empty(&state.custom_url);
                if let Some(row) = state.rows.iter_mut().find(|row| row.size == size) {
                    row.state =
                        start_download(root.to_path_buf(), size, custom_url, ui.ctx().clone());
                }
            }
            RowAction::Cancel(cancel) => cancel.cancel(),
            RowAction::Delete(size) => state.delete_confirm = Some(size),
            RowAction::Reveal => {
                if let Err(err) = open::that(root) {
                    log::warn!("could not open the model cache folder: {err}");
                }
            }
        }
    }

    if let Some(size) = state.delete_confirm {
        let mut close = false;
        let modal =
            egui::Modal::new(egui::Id::new("humm-models-delete-confirm")).show(ui.ctx(), |ui| {
                ui.heading(tr("models-delete-confirm-title"));
                ui.label(tr("models-delete-confirm-body"));
                ui.horizontal(|ui| {
                    if ui.button(tr("dialog-ok")).clicked() {
                        if let Err(err) = delete(root, size) {
                            log::warn!("could not delete the model for {size:?}: {err}");
                        }
                        needs_refresh = true;
                        close = true;
                    }
                    if ui.button(tr("dialog-cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if close || modal.should_close() {
            state.delete_confirm = None;
        }
    }

    if needs_refresh {
        state.refresh_from_disk(root);
    }

    if state.error.is_some() {
        let mut close = false;
        let modal =
            egui::Modal::new(egui::Id::new("humm-models-download-error")).show(ui.ctx(), |ui| {
                ui.heading(tr("models-download-error-title"));
                ui.label(state.error.as_deref().unwrap_or_default());
                if ui.button(tr("dialog-ok")).clicked() {
                    close = true;
                }
            });
        if close || modal.should_close() {
            state.error = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    // -- refresh ----------------------------------------------------------

    #[test]
    fn refresh_on_empty_root_lists_all_twelve_ggml_sizes_as_available() {
        let dir = tempfile::tempdir().expect("tempdir");
        let rows = refresh(dir.path(), None, false);
        assert_eq!(rows.len(), 12);
        assert!(
            rows.iter()
                .all(|row| matches!(row.state, RowState::Available))
        );
        assert!(!rows.iter().any(|row| row.size == ModelSize::Custom));
        assert!(!rows.iter().any(|row| row.size == ModelSize::Lumii));
    }

    #[test]
    fn refresh_reports_downloaded_with_size_when_ggml_and_sentinel_both_present() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ggml_path = dir.path().join(ggml_file_name(ModelSize::Small));
        std::fs::write(&ggml_path, b"01234567890123456789").expect("write ggml");
        std::fs::write(sentinel_path(&ggml_path), b"").expect("write sentinel");

        let rows = refresh(dir.path(), None, false);
        let small = rows
            .iter()
            .find(|row| row.size == ModelSize::Small)
            .expect("small row present");
        match small.state {
            RowState::Downloaded(bytes) => assert_eq!(bytes, 20),
            ref other => panic!("expected Downloaded, got {other:?}"),
        }
    }

    #[test]
    fn refresh_reports_available_when_ggml_present_without_sentinel() {
        // Matches the downloader's own semantics (registry::local_model_path):
        // a bare/partial file with no completeness sentinel is not "done".
        let dir = tempfile::tempdir().expect("tempdir");
        let ggml_path = dir.path().join(ggml_file_name(ModelSize::Small));
        std::fs::write(&ggml_path, b"partial").expect("write ggml");

        let rows = refresh(dir.path(), None, false);
        let small = rows
            .iter()
            .find(|row| row.size == ModelSize::Small)
            .expect("small row present");
        assert!(matches!(small.state, RowState::Available));
    }

    #[test]
    fn refresh_appends_custom_row_only_when_url_configured() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            !refresh(dir.path(), None, false)
                .iter()
                .any(|row| row.size == ModelSize::Custom)
        );
        let rows = refresh(dir.path(), Some("https://example.com/model.bin"), false);
        assert_eq!(rows.len(), 13);
        assert!(rows.iter().any(|row| row.size == ModelSize::Custom));
    }

    #[test]
    fn refresh_lists_lumii_row_when_included() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert!(
            !refresh(dir.path(), None, false)
                .iter()
                .any(|row| row.size == ModelSize::Lumii),
            "include_lumii: false must not list the Lumii row"
        );
        let rows = refresh(dir.path(), None, true);
        assert_eq!(rows.len(), 13);
        let lumii = rows
            .iter()
            .find(|row| row.size == ModelSize::Lumii)
            .expect("lumii row present when included");
        assert!(
            matches!(lumii.state, RowState::Available),
            "a never-downloaded Lumii row is Available like any other size"
        );
    }

    #[test]
    fn refresh_custom_row_reports_downloaded_from_the_custom_slot() {
        let dir = tempfile::tempdir().expect("tempdir");
        let ggml_path = dir.path().join(ggml_file_name(ModelSize::Custom));
        std::fs::write(&ggml_path, b"0123456789").expect("write ggml");
        std::fs::write(sentinel_path(&ggml_path), b"").expect("write sentinel");

        let rows = refresh(dir.path(), Some("https://example.com/model.bin"), false);
        let custom = rows
            .iter()
            .find(|row| row.size == ModelSize::Custom)
            .expect("custom row present");
        assert!(matches!(custom.state, RowState::Downloaded(10)));
    }

    // -- delete -------------------------------------------------------------

    #[test]
    fn delete_removes_ggml_sentinel_coreml_dir_and_no_coreml_marker() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;
        let ggml_path = dir.path().join(ggml_file_name(size));
        std::fs::write(&ggml_path, b"data").expect("write ggml");
        std::fs::write(sentinel_path(&ggml_path), b"").expect("write sentinel");
        let encoder_dir = coreml_encoder_dir(dir.path(), size);
        std::fs::create_dir_all(&encoder_dir).expect("mkdir encoder dir");
        std::fs::write(encoder_dir.join(SENTINEL_SUFFIX), b"").expect("write in-dir sentinel");
        std::fs::write(coreml_no_encoder_marker(dir.path(), size), b"")
            .expect("write no-coreml marker");

        delete(dir.path(), size).expect("delete succeeds");

        assert!(!ggml_path.exists());
        assert!(!sentinel_path(&ggml_path).exists());
        assert!(!encoder_dir.exists());
        assert!(!coreml_no_encoder_marker(dir.path(), size).exists());
    }

    #[test]
    fn delete_on_an_already_missing_model_is_not_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        delete(dir.path(), ModelSize::Small).expect("deleting a never-downloaded size is a no-op");
    }

    // -- DownloadStatus::fraction ---------------------------------------------

    #[test]
    fn fraction_none_without_a_known_total() {
        let status = DownloadStatus {
            downloaded: 10,
            total: None,
            finished: None,
        };
        assert_eq!(status.fraction(), None);
    }

    #[test]
    fn fraction_is_downloaded_over_total() {
        let status = DownloadStatus {
            downloaded: 25,
            total: Some(100),
            finished: None,
        };
        assert_eq!(status.fraction(), Some(0.25));
    }

    // -- poll_downloads -------------------------------------------------------

    #[test]
    fn poll_downloads_success_finalizes_to_downloaded_with_size() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;
        let ggml_path = dir.path().join(ggml_file_name(size));
        std::fs::write(&ggml_path, b"0123456789").expect("write ggml");
        std::fs::write(sentinel_path(&ggml_path), b"").expect("write sentinel");

        let status = Arc::new(Mutex::new(DownloadStatus {
            downloaded: 10,
            total: Some(10),
            finished: Some(Ok(())),
        }));
        let mut rows = vec![ModelRow {
            size,
            state: RowState::Downloading {
                status,
                cancel: CancelToken::new(),
            },
        }];

        let mut errors = Vec::new();
        poll_downloads(dir.path(), &mut rows, &mut |msg| errors.push(msg));

        assert!(errors.is_empty());
        assert!(matches!(rows[0].state, RowState::Downloaded(10)));
    }

    #[test]
    fn poll_downloads_failure_reports_error_and_reverts_to_available() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;
        let status = Arc::new(Mutex::new(DownloadStatus {
            downloaded: 0,
            total: None,
            finished: Some(Err("connection refused".to_string())),
        }));
        let mut rows = vec![ModelRow {
            size,
            state: RowState::Downloading {
                status,
                cancel: CancelToken::new(),
            },
        }];

        let mut errors = Vec::new();
        poll_downloads(dir.path(), &mut rows, &mut |msg| errors.push(msg));

        assert_eq!(errors.len(), 1);
        assert!(errors[0].contains("connection refused"));
        assert!(matches!(rows[0].state, RowState::Available));
    }

    #[test]
    fn poll_downloads_cancel_reverts_to_available_without_reporting_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let size = ModelSize::Small;
        let cancel = CancelToken::new();
        cancel.cancel();
        let status = Arc::new(Mutex::new(DownloadStatus {
            downloaded: 0,
            total: None,
            finished: Some(Err("canceled".to_string())),
        }));
        let mut rows = vec![ModelRow {
            size,
            state: RowState::Downloading { status, cancel },
        }];

        let mut errors = Vec::new();
        poll_downloads(dir.path(), &mut rows, &mut |msg| errors.push(msg));

        assert!(errors.is_empty());
        assert!(matches!(rows[0].state, RowState::Available));
    }

    #[test]
    fn poll_downloads_still_in_flight_row_is_left_untouched() {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = Arc::new(Mutex::new(DownloadStatus {
            downloaded: 5,
            total: Some(10),
            finished: None,
        }));
        let mut rows = vec![ModelRow {
            size: ModelSize::Small,
            state: RowState::Downloading {
                status,
                cancel: CancelToken::new(),
            },
        }];

        let mut errors = Vec::new();
        poll_downloads(dir.path(), &mut rows, &mut |msg| errors.push(msg));

        assert!(errors.is_empty());
        assert!(matches!(rows[0].state, RowState::Downloading { .. }));
    }

    // -- format_bytes -----------------------------------------------------

    #[test]
    fn format_bytes_under_a_gb_is_megabytes() {
        assert_eq!(format_bytes(75 * 1024 * 1024), "75.0 MB");
    }

    #[test]
    fn format_bytes_at_or_above_a_gb_is_gigabytes() {
        assert_eq!(format_bytes(3 * 1024 * 1024 * 1024), "3.0 GB");
    }

    // -- speed badge (J-9 ✚, design spec §8.2) ---------------------------------

    #[test]
    fn speed_badge_text_formats_one_decimal() {
        // `.contains` rather than `assert_eq!`: Fluent's default
        // `use_isolating` wraps each interpolated placeable value in
        // invisible bidi-isolation marks (U+2068/U+2069), so the rendered
        // string is not byte-identical to the plain concatenation — the
        // same reason every other `tr_args` assertion in this crate
        // (`main_window.rs`'s folder-watch pill test, the placeable-survives
        // suite in `i18n.rs`) checks substrings, not exact equality.
        let one_decimal = speed_badge_text(2.0);
        assert!(one_decimal.contains("2.0"), "got {one_decimal:?}");
        assert!(one_decimal.contains("realtime"), "got {one_decimal:?}");

        let rounded = speed_badge_text(3.1789);
        assert!(rounded.contains("3.2"), "got {rounded:?}");
    }

    #[test]
    fn measured_ratio_for_matches_the_sizes_own_wire_string() {
        let mut measured_speed = BTreeMap::new();
        measured_speed.insert("small".to_string(), 2.5);
        assert_eq!(
            measured_ratio_for(&measured_speed, ModelSize::Small),
            Some(2.5)
        );
        assert_eq!(measured_ratio_for(&measured_speed, ModelSize::Tiny), None);
    }

    #[test]
    fn measured_ratio_for_none_on_an_empty_map() {
        assert_eq!(measured_ratio_for(&BTreeMap::new(), ModelSize::Small), None);
    }

    /// A future backend-qualified entry (e.g. an `openai_api`-prefixed key,
    /// Task 18) must never match a size row, and must never panic — it
    /// simply isn't any [`ModelSize::as_str`] value, so a plain `BTreeMap`
    /// lookup already skips it without any special-case code.
    #[test]
    fn non_size_keys_ignored() {
        let mut measured_speed = BTreeMap::new();
        measured_speed.insert("openai_api".to_string(), 3.0);
        measured_speed.insert("openai_api:gpt-4o-mini-transcribe".to_string(), 1.2);
        for size in [
            ModelSize::Tiny,
            ModelSize::Small,
            ModelSize::Large,
            ModelSize::Custom,
            ModelSize::Lumii,
        ] {
            assert_eq!(
                measured_ratio_for(&measured_speed, size),
                None,
                "non-size key must not match {size:?}"
            );
        }
    }

    // -- ModelsTabState::new / refresh_from_disk -------------------------------

    #[test]
    fn new_seeds_custom_url_buffer_from_the_setting() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = ModelsTabState::new(dir.path(), Some("https://example.com/m.bin"), false);
        assert_eq!(state.custom_url, "https://example.com/m.bin");
        assert!(state.rows.iter().any(|row| row.size == ModelSize::Custom));
    }

    #[test]
    fn new_seeds_lumii_row_only_when_included() {
        let dir = tempfile::tempdir().expect("tempdir");
        let without = ModelsTabState::new(dir.path(), None, false);
        assert!(!without.include_lumii);
        assert!(!without.rows.iter().any(|row| row.size == ModelSize::Lumii));

        let with = ModelsTabState::new(dir.path(), None, true);
        assert!(with.include_lumii);
        assert!(with.rows.iter().any(|row| row.size == ModelSize::Lumii));
    }

    #[test]
    fn refresh_from_disk_carries_include_lumii_across_calls() {
        // `include_lumii` is snapshotted once at `new` time (the tab has no
        // locale picker of its own) — a later `refresh_from_disk` (e.g. after
        // a custom-URL edit) must not silently drop the Lumii row.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = ModelsTabState::new(dir.path(), None, true);
        state.refresh_from_disk(dir.path());
        assert!(state.rows.iter().any(|row| row.size == ModelSize::Lumii));
    }

    #[test]
    fn refresh_from_disk_preserves_an_in_flight_downloading_row() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut state = ModelsTabState::new(dir.path(), None, false);
        let status = Arc::new(Mutex::new(DownloadStatus::default()));
        let cancel = CancelToken::new();
        if let Some(row) = state
            .rows
            .iter_mut()
            .find(|row| row.size == ModelSize::Small)
        {
            row.state = RowState::Downloading { status, cancel };
        }

        state.refresh_from_disk(dir.path());

        let small = state
            .rows
            .iter()
            .find(|row| row.size == ModelSize::Small)
            .expect("small row still present");
        assert!(matches!(small.state, RowState::Downloading { .. }));
    }
}
