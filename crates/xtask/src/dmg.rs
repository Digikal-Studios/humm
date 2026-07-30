//! Builds the release DMG (Task 11 brief, spec §1.3 item 5): a staging
//! directory containing a copy of the signed `.app` plus an `/Applications`
//! symlink, compressed via `hdiutil create -format UDZO` into
//! `target/bundle/humm-<version>-macos-arm64.dmg`.
//!
//! `hdiutil` is the only non-Rust boundary here (ADR-025's "Apple CLIs as
//! the packaging-time non-Rust boundary" list); the staging-directory
//! assembly itself — recursive copy + one symlink — is plain `std::fs`,
//! no shelling out to `cp`/`ditto`.

use std::fs;
use std::path::Path;
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Absolute path, not a bare name — see [`crate::dylib`]'s module doc for
/// why every Apple CLI this workspace shells out to is invoked this way
/// (this machine's `PATH` shadows some of them with an older Anaconda
/// toolchain; `hdiutil` isn't one of the shadowed ones, but hardcoding it
/// too keeps that guarantee independent of `PATH` at invocation time).
const HDIUTIL: &str = "/usr/bin/hdiutil";

/// Creates `out` (a `.dmg`) containing `app` plus an `/Applications`
/// symlink, volume-named `volname`. Builds a throwaway staging directory
/// next to `out` (removed on success), so the DMG's root contains exactly
/// those two entries — nothing else that happens to live alongside `app`
/// on disk.
pub fn create(app: &Path, out: &Path, volname: &str) -> Result<()> {
    let staging = out
        .parent()
        .with_context(|| format!("{} has no parent directory", out.display()))?
        .join("dmg-staging");
    if staging.exists() {
        fs::remove_dir_all(&staging)
            .with_context(|| format!("failed to clear stale {}", staging.display()))?;
    }
    fs::create_dir_all(&staging)
        .with_context(|| format!("failed to create {}", staging.display()))?;

    let app_name = app
        .file_name()
        .with_context(|| format!("{} has no file name", app.display()))?;
    copy_dir_recursive(app, &staging.join(app_name))
        .with_context(|| format!("failed to stage a copy of {}", app.display()))?;
    std::os::unix::fs::symlink("/Applications", staging.join("Applications"))
        .context("failed to create the /Applications symlink in the DMG staging directory")?;

    if out.exists() {
        fs::remove_file(out)
            .with_context(|| format!("failed to remove stale {}", out.display()))?;
    }

    let status = Command::new(HDIUTIL)
        .arg("create")
        .args(["-volname", volname])
        .arg("-srcfolder")
        .arg(&staging)
        .args(["-format", "UDZO"])
        .arg(out)
        .status()
        .context("failed to launch hdiutil create")?;

    // Clean up the staging copy regardless of hdiutil's outcome — it can be
    // large (the whole signed app) and is never itself a build artifact.
    let cleanup = fs::remove_dir_all(&staging);

    if !status.success() {
        bail!("hdiutil create failed: {status}");
    }
    cleanup.with_context(|| format!("failed to remove staging dir {}", staging.display()))?;

    Ok(())
}

/// Recursively copies `src` into `dst` (created if needed), preserving
/// symlinks as symlinks (Homebrew dylibs are frequently version symlinks;
/// [`crate::dylib::bundle_closure`] itself never leaves one behind in
/// `Contents/Frameworks`, but this stays correct either way) and regular
/// files with `fs::copy`'s own permission-preserving behavior.
fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    fs::create_dir_all(dst).with_context(|| format!("failed to create {}", dst.display()))?;
    for entry in fs::read_dir(src).with_context(|| format!("failed to read {}", src.display()))? {
        let entry =
            entry.with_context(|| format!("failed to read an entry of {}", src.display()))?;
        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat {}", entry.path().display()))?;
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            copy_dir_recursive(&entry.path(), &dst_path)?;
        } else if file_type.is_symlink() {
            let target = fs::read_link(entry.path())
                .with_context(|| format!("failed to read symlink {}", entry.path().display()))?;
            std::os::unix::fs::symlink(&target, &dst_path).with_context(|| {
                format!(
                    "failed to recreate symlink {} -> {}",
                    dst_path.display(),
                    target.display()
                )
            })?;
        } else {
            fs::copy(entry.path(), &dst_path).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    entry.path().display(),
                    dst_path.display()
                )
            })?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Pure-filesystem check, no `hdiutil` involved: `copy_dir_recursive`
    /// reproduces a nested directory (regular files + a symlink) exactly.
    #[test]
    fn copy_dir_recursive_preserves_nested_files_and_symlinks() {
        let src = tempfile::tempdir().expect("tempdir");
        fs::create_dir_all(src.path().join("Contents/MacOS")).expect("mkdir");
        fs::write(src.path().join("Contents/MacOS/humm-gui"), b"binary").expect("write");
        std::os::unix::fs::symlink("humm-gui", src.path().join("Contents/MacOS/humm-link"))
            .expect("symlink");

        let dst = tempfile::tempdir().expect("tempdir");
        let dst_app = dst.path().join("humm.app");
        copy_dir_recursive(src.path(), &dst_app).expect("copy succeeds");

        assert_eq!(
            fs::read(dst_app.join("Contents/MacOS/humm-gui")).expect("read copy"),
            b"binary"
        );
        assert_eq!(
            fs::read_link(dst_app.join("Contents/MacOS/humm-link")).expect("read symlink"),
            Path::new("humm-gui")
        );
    }
}
