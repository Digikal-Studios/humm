//! Folder-watch scan/skip core (F-030), ported from Buzz's
//! `buzz/widgets/transcription_task_folder_watcher.py` (whole file read for
//! this port). This module is the **pure** half only — the walk, the six
//! per-file skip rules (`:70-81`), hidden-directory pruning (`:44-46`,
//! `:125-126`), the seen-set semantics (`:120-123`), and output-directory
//! mirroring (`:97-107`). There is no watcher/`notify` integration here —
//! that's the GUI-side `humm-gui::folder_watch` service, a separate concern
//! (design spec §3.2), built on top of [`scan`].

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Buzz's folder-watch/import supported extensions, lowercase and without a
/// leading dot. Audio then video, in the order of Buzz `main`
/// `buzz/transcriber/transcriber.py` `SUPPORTED_AUDIO_EXTENSIONS` /
/// `SUPPORTED_VIDEO_EXTENSIONS` (8 + 8). Shared by [`scan`]'s extension rule
/// and `humm_gui::import`'s folder-import scan / file-dialog filters, which
/// re-export this rather than duplicating it. The viewer playback classifier
/// is a separate list and already includes `m4v`.
pub const SUPPORTED_EXTENSIONS: [&str; 16] = [
    "mp3", "wav", "m4a", "m4b", "aac", "ogg", "opus", "flac", "mp4", "m4v", "webm", "ogm", "mov",
    "mkv", "avi", "wmv",
];

/// The "Audio files" file-dialog filter group — the audio-only subset of
/// [`SUPPORTED_EXTENSIONS`], same order. Lives here (rather than only in
/// `humm-gui`) so `humm_gui::import`'s re-export has a single source of truth
/// for every extension grouping.
pub const AUDIO_EXTENSIONS: [&str; 8] = ["mp3", "wav", "m4a", "m4b", "aac", "ogg", "opus", "flac"];

/// The "Video files" file-dialog filter group — the video-only subset of
/// [`SUPPORTED_EXTENSIONS`], same order.
pub const VIDEO_EXTENSIONS: [&str; 8] = ["mp4", "m4v", "webm", "ogm", "mov", "mkv", "avi", "wmv"];

/// Whether `path`'s extension (case-insensitive) is one of
/// [`SUPPORTED_EXTENSIONS`] — Buzz's
/// `os.path.splitext(filename)[1].lower() in SUPPORTED_EXTENSIONS`
/// (`:63`, `:72`).
pub fn has_supported_extension(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        })
}

/// Session-scoped memory of files [`scan`] has already yielded, mirroring
/// Buzz's `self.paths_emitted: Dict[str, Tuple[float, float]]`
/// (`transcription_task_folder_watcher.py:34`): a file only re-emits once
/// its size or mtime changes since it was last [`record`](SeenSet::record)ed.
///
/// This is deliberately *not* updated by [`scan`] itself — see that
/// function's docs.
#[derive(Debug, Default, Clone)]
pub struct SeenSet {
    emitted: HashMap<PathBuf, (u64, SystemTime)>,
}

impl SeenSet {
    /// A fresh, empty seen-set — nothing has been emitted yet this session.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Records that `path` was just emitted at this `size`/`mtime`, so a
    /// later [`SeenSet::matches`] call with the identical pair suppresses
    /// it. Buzz updates `paths_emitted` only *after* successfully emitting a
    /// task (`:120-123`) — callers of [`scan`] must call this themselves,
    /// once a [`Candidate`] has actually been enqueued, not during the scan.
    pub fn record(&mut self, path: PathBuf, size: u64, mtime: SystemTime) {
        self.emitted.insert(path, (size, mtime));
    }

    /// Whether `path` was already emitted at exactly this `size`/`mtime` —
    /// Buzz's `self.paths_emitted.get(file_path) == (size, mtime)`
    /// (`:76-79`). A change to *either* value makes this `false`, so a
    /// growing, shrinking, or freshly-touched file is treated as new again.
    #[must_use]
    pub fn matches(&self, path: &Path, size: u64, mtime: SystemTime) -> bool {
        self.emitted.get(path) == Some(&(size, mtime))
    }
}

/// One file [`scan`] decided is a genuine new folder-watch task.
///
/// `relative_dir` is `path`'s parent directory relative to the scan's
/// `input_dir` — empty for a root-level file. Feed it to
/// [`mirrored_output_dir`] to get the file's output directory (Buzz's
/// `os.path.relpath(dirpath, input_directory)`, `:97-107`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// Full path to the candidate media file.
    pub path: PathBuf,
    /// `path`'s parent directory, relative to the scanned input root; empty
    /// for a root-level file.
    pub relative_dir: PathBuf,
}

/// Recursively scans `input_dir` for new folder-watch candidates, applying
/// Buzz's six per-file skip rules in its exact order
/// (`transcription_task_folder_watcher.py:70-81`):
///
/// 1. hidden files (name starts with `.`);
/// 2. extension not in [`SUPPORTED_EXTENSIONS`] (case-insensitive);
/// 3. temp double-extension conversion files, e.g. `file.ogg.wav` (the
///    *secondary* extension — re-splitting the stem, `:66-68` — is itself
///    supported);
/// 4. `"_speech.mp3"` or `"_DeepFilterNet3.wav"` as a substring of the file
///    name (not just the extension) — Buzz's extracted-speech output files,
///    plus humm's own DeepFilterNet3 denoised-output files (deviations
///    ledger entry 15: Buzz's `deep_filter_net` plugin writes its output
///    beside the input the same way and has the same latent re-enqueue bug;
///    this is the same rule, applied to humm's second generated-artifact
///    class, not a new rule);
/// 5. `known(path)` is `true` — already a known task (the store, in
///    practice, via `has_task_for_file`);
/// 6. [`SeenSet::matches`] is `true` — already emitted at this exact
///    `(size, mtime)`.
///
/// Hidden directories (name starts with `.`) are pruned from the walk
/// entirely, never descended into — Buzz's `dirnames[:] = [d for d in
/// dirnames if not d.startswith(".")]` (`:44-46`, `:125-126`). Directory
/// entries are sorted before being walked, so the result is deterministic
/// regardless of the OS's own directory-listing order.
///
/// Symlinked directories are **not** followed — Python's `os.walk` defaults
/// to `followlinks=False`, so a symlink pointing at a directory is never
/// descended into (though, like `os.walk`, a symlink pointing at a *file* is
/// still resolved and run through the six skip rules above like any other
/// file: only the "walk into a directory" step is skipped for symlinks, not
/// file classification). This mirrors the sibling
/// `humm_gui::import::scan_folder`'s established precedent exactly (the
/// folder-import scanner, same `os.walk` `followlinks=False` default).
///
/// A `stat` failure (or a platform that can't report an mtime) on a
/// candidate file is treated as "skip this file" and logged at `debug` —
/// Buzz instead calls `os.path.getsize`/`os.path.getmtime` unguarded
/// (`:76-79`), so a stat race (e.g. the file vanishes mid-walk) raises out
/// of the Qt slot and is silently swallowed by Qt's exception handling;
/// core has no such framework to catch it for us, so this is a deliberate,
/// more-defensive deviation rather than a port of a crash.
///
/// **This function does not update `seen`.** Buzz only records into
/// `paths_emitted` after a task is actually emitted (`:120-123`); the
/// equivalent step here is the caller recording via [`SeenSet::record`]
/// once a [`Candidate`] has been successfully enqueued — scanning alone
/// must stay side-effect-free and repeatable.
#[must_use]
pub fn scan(input_dir: &Path, seen: &SeenSet, known: &dyn Fn(&Path) -> bool) -> Vec<Candidate> {
    let mut candidates = Vec::new();
    let mut stack = vec![input_dir.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        let mut files = Vec::new();
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if file_type.is_symlink() {
                // `DirEntry::file_type` never follows symlinks (it's an
                // `lstat`, not a `stat`), so the symlink's *target* type is
                // resolved separately here — a symlinked directory is
                // skipped (not traversed); a symlinked regular file is
                // still classified and run through the six skip rules,
                // same as `scan_folder`'s established precedent
                // (`humm_gui::import`). A dangling symlink (target
                // unresolvable) is silently dropped, matching that same
                // precedent.
                if std::fs::metadata(&path).is_ok_and(|target| target.is_file()) {
                    files.push(path);
                }
            } else if file_type.is_dir() {
                let is_hidden = path
                    .file_name()
                    .and_then(|name| name.to_str())
                    .is_some_and(|name| name.starts_with('.'));
                if !is_hidden {
                    subdirs.push(path);
                }
            } else if file_type.is_file() {
                files.push(path);
            }
        }

        // Sorted per directory for deterministic output; subdirs are pushed
        // in reverse so popping the (LIFO) stack visits them in sorted
        // order too, making the whole walk deterministic.
        files.sort();
        subdirs.sort();

        for path in files {
            if is_folder_watch_candidate(&path, seen, known) {
                let relative_dir = path
                    .parent()
                    .and_then(|parent| parent.strip_prefix(input_dir).ok())
                    .map_or_else(PathBuf::new, PathBuf::from);
                candidates.push(Candidate { path, relative_dir });
            }
        }

        stack.extend(subdirs.into_iter().rev());
    }

    candidates
}

/// Applies the six skip rules to a single file, in Buzz's order. Returns
/// `true` if `path` should be surfaced as a [`Candidate`].
fn is_folder_watch_candidate(path: &Path, seen: &SeenSet, known: &dyn Fn(&Path) -> bool) -> bool {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return false;
    };

    // Rule 1: hidden files.
    if file_name.starts_with('.') {
        return false;
    }

    // Rule 2: extension not in the supported set.
    if !has_supported_extension(path) {
        return false;
    }

    // Rule 3: temp double-extension conversion files (e.g. `file.ogg.wav`)
    // — Buzz re-splits the stem for a secondary extension
    // (`os.path.splitext(name_without_ext)[1]`, `:66-68`).
    let secondary_is_supported = Path::new(file_name)
        .file_stem()
        .and_then(|stem| stem.to_str())
        .and_then(|stem| Path::new(stem).extension())
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            SUPPORTED_EXTENSIONS
                .iter()
                .any(|supported| ext.eq_ignore_ascii_case(supported))
        });
    if secondary_is_supported {
        return false;
    }

    // Rule 4: extracted-speech and DeepFilterNet3 denoised-output files —
    // both are generated artifacts humm (like Buzz) writes beside the input
    // file, in the very folder being watched; without this, folder watch
    // re-enqueues its own output (deviations ledger entry 15).
    if file_name.contains("_speech.mp3") || file_name.contains("_DeepFilterNet3.wav") {
        return false;
    }

    // Rule 5: already a known task.
    if known(path) {
        return false;
    }

    // Rule 6: seen with an unchanged (size, mtime).
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(error) => {
            log::debug!(
                "folderwatch: skipping {}, stat failed: {error}",
                path.display()
            );
            return false;
        }
    };
    let Ok(mtime) = metadata.modified() else {
        log::debug!(
            "folderwatch: skipping {}, mtime unavailable on this platform",
            path.display()
        );
        return false;
    };
    if seen.matches(path, metadata.len(), mtime) {
        return false;
    }

    true
}

/// How long a folder-watch file's size and mtime must stay unchanged before
/// it is transcribed. Buzz `40f6a78` (`settle_seconds = 30`).
pub const FOLDER_WATCH_SETTLE: Duration = Duration::from_secs(30);

/// How often a pending file is re-stat'd. Buzz `settle_poll_interval_ms = 5000`.
pub const FOLDER_WATCH_SETTLE_POLL: Duration = Duration::from_secs(5);

/// Size and modification time of one watched file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStamp {
    /// Length in bytes.
    pub size: u64,
    /// Last modification time.
    pub mtime: SystemTime,
}

/// Files seen by folder watch that are not transcribed until their stamp has
/// been stable for [`FOLDER_WATCH_SETTLE`].
///
/// `now` is a duration from an arbitrary origin the caller owns, so tests can
/// move the clock without sleeping. Buzz stores `time.monotonic()`.
#[derive(Debug)]
pub struct SettleSet<T> {
    pending: HashMap<PathBuf, Pending<T>>,
}

#[derive(Debug)]
struct Pending<T> {
    stamp: FileStamp,
    since: Duration,
    value: T,
}

/// What [`SettleSet::poll`] decided for the files it was asked about.
#[derive(Debug)]
pub struct SettlePoll<T> {
    /// Stamp unchanged for the settle interval. Removed from the set.
    pub settled: Vec<(PathBuf, FileStamp, T)>,
    /// `stat` returned `None` (removed, or unreadable). Removed from the set.
    pub discarded: Vec<(PathBuf, T)>,
}

impl<T> Default for SettleSet<T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<T> SettleSet<T> {
    /// An empty set.
    #[must_use]
    pub fn new() -> Self {
        Self {
            pending: HashMap::new(),
        }
    }

    /// Whether any file is waiting.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Remembers `path` if it is not already pending. Returns `true` when
    /// this is the first sighting.
    pub fn insert(&mut self, path: PathBuf, stamp: FileStamp, now: Duration, value: T) -> bool {
        if self.pending.contains_key(&path) {
            return false;
        }
        self.pending.insert(
            path,
            Pending {
                stamp,
                since: now,
                value,
            },
        );
        true
    }

    /// Re-stats every pending file. A changed stamp resets the clock. A
    /// missing file is discarded. An unchanged stamp older than
    /// `settle_after` is settled.
    pub fn poll(
        &mut self,
        now: Duration,
        settle_after: Duration,
        mut stat: impl FnMut(&Path) -> Option<FileStamp>,
    ) -> SettlePoll<T> {
        let mut settled = Vec::new();
        let mut discarded = Vec::new();
        let paths: Vec<PathBuf> = self.pending.keys().cloned().collect();
        for path in paths {
            let Some(pending) = self.pending.get_mut(&path) else {
                continue;
            };
            let Some(stamp) = stat(&path) else {
                if let Some(pending) = self.pending.remove(&path) {
                    discarded.push((path, pending.value));
                }
                continue;
            };
            if stamp != pending.stamp {
                pending.stamp = stamp;
                pending.since = now;
                continue;
            }
            if now.saturating_sub(pending.since) < settle_after {
                continue;
            }
            if let Some(pending) = self.pending.remove(&path) {
                settled.push((path, stamp, pending.value));
            }
        }
        SettlePoll { settled, discarded }
    }

    /// Forgets `path` and returns its value, if it was pending.
    pub fn remove(&mut self, path: &Path) -> Option<T> {
        self.pending.remove(path).map(|pending| pending.value)
    }

    /// Drops every pending file and returns the values, for a watch that is
    /// stopping.
    pub fn drain(&mut self) -> Vec<(PathBuf, T)> {
        self.pending
            .drain()
            .map(|(path, pending)| (path, pending.value))
            .collect()
    }
}

/// Where `relative_dir` (a [`Candidate::relative_dir`]) mirrors to under
/// `output_root` — Buzz's subdirectory-preserving output layout
/// (`transcription_task_folder_watcher.py:97-107`): an empty `relative_dir`
/// (a root-level file) mirrors to `output_root` itself, Buzz's
/// `relpath == "."` arm; otherwise it's `output_root` joined with
/// `relative_dir`. Creating the resulting directory (Buzz's
/// `os.makedirs(..., exist_ok=True)`) is the *caller's* job at emit time,
/// not this function's — it only computes the path.
#[must_use]
pub fn mirrored_output_dir(output_root: &Path, relative_dir: &Path) -> PathBuf {
    if relative_dir.as_os_str().is_empty() {
        output_root.to_path_buf()
    } else {
        output_root.join(relative_dir)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use std::time::Duration;

    fn touch(path: &Path, contents: &[u8]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent dirs");
        }
        std::fs::write(path, contents).expect("write fixture file");
    }

    fn never_known(_: &Path) -> bool {
        false
    }

    fn metadata_of(path: &Path) -> (u64, SystemTime) {
        let metadata = std::fs::metadata(path).expect("stat fixture file");
        (metadata.len(), metadata.modified().expect("mtime"))
    }

    #[test]
    fn plain_wav_included() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file.wav");
        touch(&file, b"audio");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(
            candidates,
            vec![Candidate {
                path: file,
                relative_dir: PathBuf::new(),
            }]
        );
    }

    #[test]
    fn double_extension_ogg_wav_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join("file.ogg.wav"), b"temp conversion");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn speech_mp3_substring_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join("talk_speech.mp3"), b"speech output");
        touch(&dir.path().join("_speech.mp3"), b"speech output");
        // "episode_speech.mp3_final.wav" traced against every other rule to
        // confirm this case is isolated to rule 4 alone:
        //   - rule 1 (hidden): doesn't start with `.` — passes.
        //   - rule 2 (extension): `Path::extension` of the full name is
        //     `wav`, supported — passes.
        //   - rule 3 (double extension): `Path::file_stem` of the full name
        //     is `episode_speech.mp3_final`; *its* extension is
        //     `mp3_final`, which is not in `SUPPORTED_EXTENSIONS` (it's not
        //     `mp3`) — passes.
        //   - rule 4 (`_speech.mp3` substring): the full name contains
        //     `_speech.mp3` literally (`episode` + `_speech.mp3` +
        //     `_final.wav`) — fails, skipped here and only here.
        touch(
            &dir.path().join("episode_speech.mp3_final.wav"),
            b"speech output",
        );

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn deep_filter_net_output_skipped() {
        // F-030 x F-112: DeepFilterNet3 writes `<stem>_DeepFilterNet3.wav`
        // beside the input, inside the watched folder — without rule 4
        // covering this suffix too, folder watch would re-enqueue its own
        // denoised output (see deviations ledger entry 15).
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join("talk_DeepFilterNet3.wav"), b"denoised");
        touch(&dir.path().join("_DeepFilterNet3.wav"), b"denoised");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn doubled_deep_filter_net_suffix_skipped() {
        // If "Keep denoised file" is on and a kept output somehow gets
        // re-run through the reducer, Buzz's naming produces a doubled
        // suffix (`..._DeepFilterNet3_DeepFilterNet3.wav`); confirm it's
        // still caught by the substring check regardless of repetition.
        let dir = tempfile::tempdir().expect("tempdir");
        touch(
            &dir.path().join("talk_DeepFilterNet3_DeepFilterNet3.wav"),
            b"denoised twice",
        );

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn hidden_directory_subtree_pruned() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join(".hidden").join("inside.mp3"), b"audio");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn hidden_file_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join(".hidden.mp3"), b"audio");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn m4b_aac_and_m4v_are_supported() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join("book.m4b"), b"audio");
        touch(&dir.path().join("clip.aac"), b"audio");
        touch(&dir.path().join("film.m4v"), b"video");

        let mut names: Vec<_> = scan(dir.path(), &SeenSet::new(), &never_known)
            .into_iter()
            .map(|candidate| {
                candidate
                    .path
                    .file_name()
                    .expect("fixture file name")
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        names.sort();
        assert_eq!(names, ["book.m4b", "clip.aac", "film.m4v"]);
    }

    #[test]
    fn unsupported_extension_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        touch(&dir.path().join("notes.txt"), b"not media");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn known_task_path_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file.mp3");
        touch(&file, b"audio");

        let candidates = scan(dir.path(), &SeenSet::new(), &|path| path == file);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn seen_same_size_mtime_not_reemitted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file.mp3");
        touch(&file, b"audio");
        let (size, mtime) = metadata_of(&file);

        let mut seen = SeenSet::new();
        seen.record(file.clone(), size, mtime);

        let candidates = scan(dir.path(), &seen, &never_known);

        assert_eq!(candidates, Vec::new());
    }

    #[test]
    fn seen_size_change_reemits() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file.mp3");
        touch(&file, b"audio");
        let (size, mtime) = metadata_of(&file);

        let mut seen = SeenSet::new();
        seen.record(file.clone(), size, mtime);

        // Grow the file: size changes, so it must re-emit regardless of
        // whether the filesystem's mtime clock ticked in between.
        touch(&file, b"a much longer audio payload than before");

        let candidates = scan(dir.path(), &seen, &never_known);

        assert_eq!(
            candidates,
            vec![Candidate {
                path: file,
                relative_dir: PathBuf::new(),
            }]
        );
    }

    #[test]
    fn seen_set_matches_requires_both_size_and_mtime() {
        let path = PathBuf::from("/does/not/matter.mp3");
        let mtime = SystemTime::now();
        let mut seen = SeenSet::new();
        seen.record(path.clone(), 100, mtime);

        assert!(seen.matches(&path, 100, mtime));
        assert!(!seen.matches(&path, 101, mtime));
        assert!(!seen.matches(&path, 100, mtime + Duration::from_secs(1)));
        assert!(!seen.matches(Path::new("/other.mp3"), 100, mtime));
    }

    #[test]
    fn uppercase_extension_accepted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let file = dir.path().join("file.MP3");
        touch(&file, b"audio");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(
            candidates,
            vec![Candidate {
                path: file,
                relative_dir: PathBuf::new(),
            }]
        );
    }

    #[test]
    fn mirroring_relative_math() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("a").join("b").join("f.mp3");
        let root_file = dir.path().join("f.mp3");
        touch(&nested, b"audio");
        touch(&root_file, b"audio");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);
        let nested_candidate = candidates
            .iter()
            .find(|candidate| candidate.path == nested)
            .expect("nested candidate present");
        let root_candidate = candidates
            .iter()
            .find(|candidate| candidate.path == root_file)
            .expect("root candidate present");

        assert_eq!(nested_candidate.relative_dir, Path::new("a").join("b"));
        assert_eq!(root_candidate.relative_dir, PathBuf::new());

        let output_root = Path::new("/out");
        assert_eq!(
            mirrored_output_dir(output_root, &nested_candidate.relative_dir),
            Path::new("/out/a/b")
        );
        assert_eq!(
            mirrored_output_dir(output_root, &root_candidate.relative_dir),
            Path::new("/out")
        );
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_media_file_is_scanned() {
        let dir = tempfile::tempdir().expect("tempdir");
        let target = dir.path().join("target.wav");
        touch(&target, b"audio");
        let link = dir.path().join("link.wav");
        std::os::unix::fs::symlink(&target, &link).expect("create symlink");

        let mut candidates = scan(dir.path(), &SeenSet::new(), &never_known);
        candidates.sort_by(|a, b| a.path.cmp(&b.path));

        let mut expected = vec![
            Candidate {
                path: link,
                relative_dir: PathBuf::new(),
            },
            Candidate {
                path: target,
                relative_dir: PathBuf::new(),
            },
        ];
        expected.sort_by(|a, b| a.path.cmp(&b.path));

        assert_eq!(candidates, expected);
    }

    #[cfg(unix)]
    #[test]
    fn symlinked_directory_not_traversed() {
        let dir = tempfile::tempdir().expect("tempdir");
        let real_sub = dir.path().join("real");
        std::fs::create_dir(&real_sub).expect("create real subdir");
        let inside = real_sub.join("inside.wav");
        touch(&inside, b"audio");

        let link = dir.path().join("linked");
        std::os::unix::fs::symlink(&real_sub, &link).expect("create symlink");

        // The real subdirectory's file is found via the real path; the
        // symlinked directory is not descended into a second time, so
        // `inside.wav` appears exactly once (via `real/`, not `linked/`) —
        // matching `scan_folder`'s own precedent for this exact scenario.
        assert_eq!(
            scan(dir.path(), &SeenSet::new(), &never_known),
            vec![Candidate {
                path: inside,
                relative_dir: PathBuf::from("real"),
            }]
        );
    }

    #[test]
    fn scan_result_is_sorted_deterministically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let z = dir.path().join("z.wav");
        let a = dir.path().join("a.wav");
        touch(&z, b"audio");
        touch(&a, b"audio");

        let candidates = scan(dir.path(), &SeenSet::new(), &never_known);

        assert_eq!(
            candidates,
            vec![
                Candidate {
                    path: a,
                    relative_dir: PathBuf::new(),
                },
                Candidate {
                    path: z,
                    relative_dir: PathBuf::new(),
                },
            ]
        );
    }

    fn stamp(size: u64, secs: u64) -> FileStamp {
        FileStamp {
            size,
            mtime: SystemTime::UNIX_EPOCH + Duration::from_secs(secs),
        }
    }

    #[test]
    fn settle_waits_thirty_seconds_of_unchanged_stamp() {
        assert_eq!(FOLDER_WATCH_SETTLE, Duration::from_secs(30));
        let path = PathBuf::from("/watch/clip.wav");
        let mut set = SettleSet::new();
        assert!(set.insert(path.clone(), stamp(10, 1), Duration::ZERO, "task"));
        assert!(!set.insert(path.clone(), stamp(10, 1), Duration::from_secs(1), "other"));

        let early = set.poll(Duration::from_secs(29), FOLDER_WATCH_SETTLE, |_| {
            Some(stamp(10, 1))
        });
        assert!(early.settled.is_empty());
        assert!(early.discarded.is_empty());

        let ready = set.poll(Duration::from_secs(30), FOLDER_WATCH_SETTLE, |_| {
            Some(stamp(10, 1))
        });
        assert_eq!(ready.settled.len(), 1);
        assert_eq!(ready.settled[0].0, path);
        assert_eq!(ready.settled[0].2, "task");
        assert!(set.is_empty());
    }

    #[test]
    fn settle_resets_when_the_stamp_changes() {
        let path = PathBuf::from("/watch/clip.wav");
        let mut set = SettleSet::new();
        set.insert(path.clone(), stamp(10, 1), Duration::ZERO, ());
        let changed = set.poll(Duration::from_secs(20), FOLDER_WATCH_SETTLE, |_| {
            Some(stamp(40, 2))
        });
        assert!(changed.settled.is_empty());
        let too_soon = set.poll(Duration::from_secs(49), FOLDER_WATCH_SETTLE, |_| {
            Some(stamp(40, 2))
        });
        assert!(too_soon.settled.is_empty());
        let ready = set.poll(Duration::from_secs(50), FOLDER_WATCH_SETTLE, |_| {
            Some(stamp(40, 2))
        });
        assert_eq!(ready.settled.len(), 1);
        assert_eq!(ready.settled[0].1, stamp(40, 2));
    }

    #[test]
    fn settle_discards_a_file_that_disappears() {
        let path = PathBuf::from("/watch/clip.wav");
        let mut set = SettleSet::new();
        set.insert(path.clone(), stamp(10, 1), Duration::ZERO, 7);
        let polled = set.poll(Duration::from_secs(5), FOLDER_WATCH_SETTLE, |_| None);
        assert!(polled.settled.is_empty());
        assert_eq!(polled.discarded, vec![(path, 7)]);
    }
}
