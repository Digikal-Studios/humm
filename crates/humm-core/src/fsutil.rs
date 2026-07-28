//! Filesystem helpers shared by `humm-core` and its adapter crates.
//!
//! Currently a single primitive: [`copy_then_rename`], an atomic
//! same-filesystem file-placement helper. It backs both the folder-watch
//! move path ([`crate::pipeline`]'s `move_file`) and `humm-media`'s
//! `DemucsIsolator` output placement — promoted here (from a private
//! `humm-media::demucs` copy) so the two call sites share one
//! implementation and one set of tests instead of drifting independently
//! (M9, D5b-1/D5b-13).

use std::path::Path;

/// Cross-device-safe file placement: copies `from` to a sibling temp file in
/// **`to`'s own directory** (same filesystem as the eventual destination),
/// then `rename`s that temp file into place as the one atomic last step — a
/// same-filesystem rename can't itself fail with a cross-device error, and
/// rename is atomic, so `to` either doesn't exist yet or is fully, correctly
/// present; never partial. Callers reach for this as the fallback when a
/// plain `fs::rename(from, to)` fails (typically `EXDEV`, a cross-device
/// rename) — `from` and `to` are not required to share a filesystem, only
/// `to` and this function's own temp file do.
///
/// The temp file is named `<to's file name>.tmp-<pid>` in `to`'s directory
/// (falling back to `.` / `"output"` if `to` has no parent/file name
/// respectively) — mirrors `Settings::save`'s own `<file_name>.tmp-<pid>`
/// atomic-write convention (`settings.rs`): same-directory temp name,
/// rename-into-place last.
///
/// On success, `from` is removed (best-effort: a failure removing it is not
/// treated as this function's own failure, since `to` is already
/// completely, correctly placed by that point). This fixes a bug class the
/// original direct-`fs::copy(from, to)` version had: a copy failing partway
/// (disk full, I/O error) could leave a partial file *at* `to`; worse, if
/// the copy fully succeeded but the following `remove_file(from)` failed,
/// the function still returned `Err` even though `to` now held a complete
/// file — a caller's `?` would propagate that error and never learn `to`'s
/// path, permanently orphaning it. This version never writes to `to`
/// directly: on any failure (the copy itself, or the rename-into-place),
/// the temp file is removed and `to` is never touched at all.
pub fn copy_then_rename(from: &Path, to: &Path) -> std::io::Result<()> {
    let parent = to.parent().unwrap_or_else(|| Path::new("."));
    let file_name = to
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".to_string());
    let temp = parent.join(format!("{file_name}.tmp-{}", std::process::id()));

    match std::fs::copy(from, &temp).and_then(|_| std::fs::rename(&temp, to)) {
        Ok(_) => {
            // See this function's doc: a failure removing `from` here is
            // not this function's own failure, since `to` is already
            // complete and correctly placed.
            let _ = std::fs::remove_file(from);
            Ok(())
        }
        Err(err) => {
            // Never leave a partial/orphaned file behind: on *any* failure
            // (the copy itself, or the rename-into-place), the temp file is
            // removed and `to` is never touched at all.
            let _ = std::fs::remove_file(&temp);
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn copy_then_rename_places_file_atomically() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("source.mp3");
        let to = dir.path().join("dest.mp3");
        std::fs::write(&from, b"payload").expect("write source");

        copy_then_rename(&from, &to).expect("copy_then_rename should succeed");

        assert_eq!(std::fs::read_to_string(&to).unwrap(), "payload");
        assert!(
            !from.exists(),
            "source should be removed after a successful move"
        );
        // No leftover temp file beside the destination.
        let siblings: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(
            siblings.len(),
            1,
            "expected only the destination file, no leftover temp: {siblings:?}"
        );
    }

    #[test]
    fn copy_then_rename_cleans_up_temp_on_copy_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        // Source doesn't exist: the initial `fs::copy` fails immediately,
        // before any temp file is even created.
        let from = dir.path().join("does-not-exist.mp3");
        let to = dir.path().join("dest.mp3");

        let err = copy_then_rename(&from, &to).expect_err("missing source should fail the copy");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

        let siblings: Vec<_> = std::fs::read_dir(dir.path()).expect("read_dir").collect();
        assert!(
            siblings.is_empty(),
            "expected no leftover files after a failed copy: {siblings:?}"
        );
    }

    #[test]
    fn copy_then_rename_cleans_up_temp_on_rename_failure() {
        let dir = tempfile::tempdir().expect("tempdir");
        let from = dir.path().join("source.mp3");
        std::fs::write(&from, b"payload").expect("write source");
        // `to` is an existing directory: a file can't be renamed onto a
        // directory, giving a copy-succeeds/rename-fails case.
        let to = dir.path().join("dest-is-a-dir");
        std::fs::create_dir(&to).expect("create directory at dest path");

        let result = copy_then_rename(&from, &to);
        assert!(
            result.is_err(),
            "renaming a file onto an existing directory should fail"
        );

        // Regardless of the exact (platform-dependent) error kind: no
        // leftover temp file, and `to` is still the empty directory it was
        // (never touched).
        let siblings: Vec<_> = std::fs::read_dir(dir.path())
            .expect("read_dir")
            .map(|e| e.expect("entry").file_name())
            .collect();
        assert_eq!(
            siblings.len(),
            2,
            "expected only source + the dest dir, no leftover temp: {siblings:?}"
        );
        assert!(
            to.is_dir(),
            "dest path should still be the untouched directory"
        );
    }
}
