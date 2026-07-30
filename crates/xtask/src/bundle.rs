//! Assembles `target/bundle/humm.app` (Task 10 brief, spec §1.3, assembly
//! half only). **Scope note:** this module places binaries, resources, and
//! `Info.plist` into the right layout. Dylib closure walk/rewrite, code
//! signing, and DMG creation are Task 11 — an assembled-by-this-module app
//! is deliberately unsigned and has its Homebrew dylib references
//! unresolved (`otool -L` would still show absolute `/opt/homebrew/...`
//! paths); it runs the CLI binary fine (no OpenBLAS link) but the GUI
//! binary is not launchable yet.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::plist;

/// The repo root, computed from `xtask`'s own manifest directory
/// (`crates/xtask` is always exactly two levels below the workspace root),
/// so resource files (`NOTICE`, `LICENSE`, the committed icon) resolve by a
/// fixed relative path regardless of the process's current directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xtask is nested two directories below the repo root")
        .to_path_buf()
}

/// Assembles `<staging>/humm.app` from the release binaries in `bin_dir`
/// (in real usage, `target/release/`; a dummy pair of files in tests).
/// Layout (task brief, verbatim):
///
/// ```text
/// humm.app/Contents/MacOS/humm-gui
/// humm.app/Contents/MacOS/humm
/// humm.app/Contents/Resources/humm.icns
/// humm.app/Contents/Resources/NOTICE
/// humm.app/Contents/Resources/LICENSE
/// humm.app/Contents/Info.plist
/// ```
///
/// Returns the path to the assembled `humm.app` directory. `NOTICE`,
/// `LICENSE`, and `assets/icon/humm.icns` are read from the repo root (not
/// from `bin_dir`) — they're source-controlled inputs, not build outputs.
pub fn assemble(bin_dir: &Path, staging: &Path) -> io::Result<PathBuf> {
    let app = staging.join("humm.app");
    let contents = app.join("Contents");
    let macos = contents.join("MacOS");
    let resources = contents.join("Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;

    fs::copy(bin_dir.join("humm-gui"), macos.join("humm-gui"))?;
    fs::copy(bin_dir.join("humm"), macos.join("humm"))?;

    let root = repo_root();
    fs::copy(
        root.join("assets/icon/humm.icns"),
        resources.join("humm.icns"),
    )?;
    fs::copy(root.join("NOTICE"), resources.join("NOTICE"))?;
    fs::copy(root.join("LICENSE"), resources.join("LICENSE"))?;

    fs::write(
        contents.join("Info.plist"),
        plist::render(env!("CARGO_PKG_VERSION")),
    )?;

    Ok(app)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn bundle_layout_places_binaries_and_resources() {
        let bin_dir = tempfile::tempdir().expect("tempdir");
        fs::write(bin_dir.path().join("humm-gui"), b"fake-gui-binary").expect("write fake gui");
        fs::write(bin_dir.path().join("humm"), b"fake-cli-binary").expect("write fake cli");
        let staging = tempfile::tempdir().expect("tempdir");

        let app = assemble(bin_dir.path(), staging.path()).expect("assemble succeeds");

        assert_eq!(app, staging.path().join("humm.app"));
        assert!(app.join("Contents/MacOS/humm-gui").is_file());
        assert!(app.join("Contents/MacOS/humm").is_file());
        assert!(app.join("Contents/Resources/humm.icns").is_file());
        assert!(app.join("Contents/Resources/NOTICE").is_file());
        assert!(app.join("Contents/Resources/LICENSE").is_file());
        assert!(app.join("Contents/Info.plist").is_file());

        let plist_xml = fs::read_to_string(app.join("Contents/Info.plist")).unwrap();
        assert!(plist_xml.contains("com.digikal.humm"));

        let gui_bytes = fs::read(app.join("Contents/MacOS/humm-gui")).unwrap();
        assert_eq!(gui_bytes, b"fake-gui-binary");
    }
}
