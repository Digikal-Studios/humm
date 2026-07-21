//! Import intake (F-020/F-021/F-022 GUI halves): the supported-extension
//! set, a recursive folder scanner, the `Files`/`Url` request shape the
//! options sheet consumes, and the (rfd-backed) file/folder pickers behind a
//! thin seam so a kittest harness never has to drive a real, blocking native
//! dialog.
//!
//! Ported from Buzz's own extension/dialog constants
//! (`buzz/transcriber/transcription_task_folder_watcher.py:15-18`,
//! `buzz/transcriber/transcriber.py:222-225`,
//! `buzz/widgets/main_window.py:268-304`) — see
//! `buzz-notes-import-progress-models.md` §F-022 for the full citations.

use std::path::{Path, PathBuf};

/// Buzz's folder-watch/import supported extensions
/// (`transcription_task_folder_watcher.py:15-18`), lowercase and without a
/// leading dot: 6 audio + 7 video = 13. Used both by [`scan_folder`] (folder
/// import) and the "Media files" file-dialog filter group.
pub const SUPPORTED_EXTENSIONS: [&str; 13] = [
    "mp3", "wav", "m4a", "ogg", "opus", "flac", "mp4", "webm", "ogm", "mov", "mkv", "avi", "wmv",
];

/// The "Audio files" file-dialog filter group (`transcriber.py:222-225`) —
/// the audio-only subset of [`SUPPORTED_EXTENSIONS`], same order.
const AUDIO_EXTENSIONS: [&str; 6] = ["mp3", "wav", "m4a", "ogg", "opus", "flac"];

/// The "Video files" file-dialog filter group (`transcriber.py:222-225`) —
/// the video-only subset of [`SUPPORTED_EXTENSIONS`], same order.
const VIDEO_EXTENSIONS: [&str; 7] = ["mp4", "webm", "ogm", "mov", "mkv", "avi", "wmv"];

/// What the options sheet was opened for: a batch of local files, or one
/// remote URL. Produced by every import path (file dialog, drag-drop, folder
/// scan, URL dialog, argv) and consumed by
/// `views::options_sheet::build_specs`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportRequest {
    /// One or more local files, picked, dropped, or found by a folder scan.
    Files(Vec<PathBuf>),
    /// A single remote URL, validated by `views::url_dialog::validate_url`.
    Url(url::Url),
}

/// Whether `path`'s extension (case-insensitive) is one of
/// [`SUPPORTED_EXTENSIONS`].
fn has_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        })
}

/// Recursively scans `root` for files with a [`SUPPORTED_EXTENSIONS`]
/// extension, returning them sorted by path for a deterministic result
/// regardless of the OS's own directory-listing order.
///
/// Ported from Buzz's `os.walk`-based folder import
/// (`main_window.py:288-304`): hidden files (dotfiles) are **included** —
/// Buzz's walk applies no dotfile filtering — and a missing/unreadable
/// directory (including `root` itself) silently contributes nothing rather
/// than erroring, matching Buzz's own "none found → silent return" folder-
/// import contract one level up.
///
/// Symlinked directories are **not** followed — Python's `os.walk` defaults
/// to `followlinks=False`, so a symlink pointing at a directory is never
/// descended into (though, like `os.walk`, a symlink pointing at a *file* is
/// still matched against the extension set: only the "walk into a
/// directory" step is skipped for symlinks, not file classification).
pub fn scan_folder(root: &Path) -> Vec<PathBuf> {
    let mut results = Vec::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };

            if file_type.is_symlink() {
                // `DirEntry::file_type` never follows symlinks (it's an
                // `lstat`, not a `stat`), so the symlink's *target* type is
                // resolved separately here — a symlinked directory is
                // skipped (not followed); a symlinked regular file is still
                // matched against the extension set, same as `os.walk`.
                if let Ok(target) = std::fs::metadata(&path)
                    && target.is_file()
                    && has_supported_extension(&path)
                {
                    results.push(path);
                }
            } else if file_type.is_dir() {
                stack.push(path);
            } else if file_type.is_file() && has_supported_extension(&path) {
                results.push(path);
            }
        }
    }

    results.sort();
    results
}

/// Seam over the (blocking, native) file/folder pickers, so a kittest
/// harness never has to drive a real rfd dialog: production code always
/// uses [`RfdFileDialogs`]; a test-only fake can script picks instead.
/// Nothing in this milestone's kittest coverage exercises this trait's real
/// (rfd-backed) implementation — that only happens at manual acceptance —
/// but the seam exists so a future test *can* inject one without touching
/// call sites.
pub trait FileDialogs {
    /// Opens the "Select audio file" dialog with Buzz's four filter groups
    /// (Media/Audio/Video/All), starting in `start_dir` if given. `None` on
    /// cancel.
    fn pick_files(&self, start_dir: Option<&Path>) -> Option<Vec<PathBuf>>;

    /// Opens the "Select folder" dialog, starting in `start_dir` if given.
    /// `None` on cancel.
    fn pick_folder(&self, start_dir: Option<&Path>) -> Option<PathBuf>;
}

/// The real, rfd-backed [`FileDialogs`] — blocking native dialogs, run on the
/// main thread (the whole app is single-threaded egui).
pub struct RfdFileDialogs;

impl FileDialogs for RfdFileDialogs {
    fn pick_files(&self, start_dir: Option<&Path>) -> Option<Vec<PathBuf>> {
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Media files", &SUPPORTED_EXTENSIONS)
            .add_filter("Audio files", &AUDIO_EXTENSIONS)
            .add_filter("Video files", &VIDEO_EXTENSIONS)
            .add_filter("All files", &["*"]);
        if let Some(dir) = start_dir {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_files()
    }

    fn pick_folder(&self, start_dir: Option<&Path>) -> Option<PathBuf> {
        let mut dialog = rfd::FileDialog::new();
        if let Some(dir) = start_dir {
            dialog = dialog.set_directory(dir);
        }
        dialog.pick_folder()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn touch(path: &Path) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(path, b"fake media").expect("write fixture file");
    }

    #[test]
    fn supported_extensions_is_buzz_s_thirteen_lowercase_no_dot() {
        assert_eq!(
            SUPPORTED_EXTENSIONS,
            [
                "mp3", "wav", "m4a", "ogg", "opus", "flac", "mp4", "webm", "ogm", "mov", "mkv",
                "avi", "wmv",
            ]
        );
    }

    #[test]
    fn scan_folder_on_missing_root_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        let missing = dir.path().join("does-not-exist");
        assert_eq!(scan_folder(&missing), Vec::<PathBuf>::new());
    }

    #[test]
    fn scan_folder_on_empty_folder_is_empty() {
        let dir = tempfile::tempdir().expect("tempdir");
        assert_eq!(scan_folder(dir.path()), Vec::<PathBuf>::new());
    }

    #[test]
    fn scan_folder_matches_nested_files_and_excludes_unsupported() {
        let dir = tempfile::tempdir().expect("tempdir");
        let a = dir.path().join("a.mp3");
        let b = dir.path().join("sub").join("b.wav");
        let readme = dir.path().join("readme.txt");
        touch(&a);
        touch(&b);
        touch(&readme);

        let found = scan_folder(dir.path());
        assert_eq!(found, vec![a, b]);
    }

    #[test]
    fn scan_folder_includes_hidden_files() {
        let dir = tempfile::tempdir().expect("tempdir");
        let hidden = dir.path().join(".hidden.mp3");
        touch(&hidden);

        assert_eq!(scan_folder(dir.path()), vec![hidden]);
    }

    #[test]
    fn scan_folder_matches_extension_case_insensitively() {
        let dir = tempfile::tempdir().expect("tempdir");
        let upper = dir.path().join("file.MP3");
        touch(&upper);

        assert_eq!(scan_folder(dir.path()), vec![upper]);
    }

    #[test]
    fn scan_folder_result_is_sorted_deterministically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let z = dir.path().join("z.wav");
        let a = dir.path().join("a.wav");
        let m = dir.path().join("m.wav");
        touch(&z);
        touch(&a);
        touch(&m);

        assert_eq!(scan_folder(dir.path()), vec![a, m, z]);
    }

    #[cfg(unix)]
    #[test]
    fn scan_folder_does_not_follow_symlinked_directories() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real_sub = dir.path().join("real");
        std::fs::create_dir(&real_sub).expect("create real subdir");
        let inside = real_sub.join("inside.wav");
        touch(&inside);

        let link = dir.path().join("linked");
        std::os::unix::fs::symlink(&real_sub, &link).expect("create symlink");

        // The real subdirectory's file is found via the real path; the
        // symlinked directory is not descended into a second time, so
        // `inside.wav` appears exactly once (via `real/`, not `linked/`).
        assert_eq!(scan_folder(dir.path()), vec![inside]);
    }

    #[cfg(unix)]
    #[test]
    fn scan_folder_still_matches_a_symlinked_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target.wav");
        touch(&target);
        let link = dir.path().join("link.wav");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let found = scan_folder(dir.path());
        assert_eq!(found, vec![link, target]);
    }
}
