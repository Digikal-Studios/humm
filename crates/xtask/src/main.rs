//! `cargo xtask`: dev-only release tooling (Task 10 brief; ADR-025). Never
//! shipped — this binary is excluded from the app bundle it builds and is
//! not published (`publish = false`).
//!
//! Subcommands (aliased via the repo-root `.cargo/config.toml`, so `cargo
//! xtask <subcommand>` works from anywhere in the workspace):
//!
//! - `bundle` — builds release `humm-gui`/`humm` and assembles
//!   `target/bundle/humm.app` (assembly only; dylib closure/signing/DMG are
//!   Task 11 — see [`bundle::assemble`]'s doc comment for exactly what this
//!   produces and what it deliberately does not).
//! - `gen-icon` — renders the committed `assets/icon/humm.icns` from
//!   scratch via `iconutil` (see [`icon`]). Run manually when the mark
//!   needs to change; `bundle` never regenerates it.
//! - `version-info --notes <file> [--out <path>]` — renders
//!   `version_info.json` (see [`version_info::render`]) to `--out`, or to
//!   stdout if `--out` is omitted.
//!
//! No subcommand here invokes an Apple CLI (`otool`, `iconutil`, …) from
//! within a `#[cfg(test)]` — those are exercised only by the real,
//! manually-run `bundle`/`gen-icon` commands, never by `cargo test`.

mod bundle;
mod icon;
mod plist;
mod version_info;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("bundle") => cmd_bundle(),
        Some("gen-icon") => cmd_gen_icon(),
        Some("version-info") => cmd_version_info(&args[1..]),
        Some(other) => Err(format!("unknown subcommand `{other}`\n\n{USAGE}")),
        None => Err(USAGE.to_string()),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("xtask: {message}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str =
    "usage: cargo xtask <bundle|gen-icon|version-info --notes <file> [--out <path>]>";

/// The repo root — `crates/xtask` is always exactly two directories below
/// it, so this resolves regardless of the process's current directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xtask is nested two directories below the repo root")
        .to_path_buf()
}

/// `cargo xtask bundle`: release-build `humm-gui` + `humm`, then assemble
/// `target/bundle/humm.app` via [`bundle::assemble`]. Unsigned, no dylib
/// closure — see that function's doc comment for the exact scope boundary.
fn cmd_bundle() -> Result<(), String> {
    let root = repo_root();

    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "humm-gui", "-p", "humm-cli"])
        .current_dir(&root)
        .status()
        .map_err(|e| format!("failed to launch `cargo build --release`: {e}"))?;
    if !status.success() {
        return Err(format!("release build failed: {status}"));
    }

    let bin_dir = root.join("target/release");
    let staging = root.join("target/bundle");

    // Assembling from scratch each time avoids stale files left over from a
    // previous layout (e.g. a resource that a later task renames).
    let app = staging.join("humm.app");
    if app.exists() {
        std::fs::remove_dir_all(&app)
            .map_err(|e| format!("failed to clear stale {}: {e}", app.display()))?;
    }

    let app = bundle::assemble(&bin_dir, &staging).map_err(|e| format!("assemble failed: {e}"))?;
    println!("assembled {}", app.display());
    Ok(())
}

/// `cargo xtask gen-icon`: renders the iconset PNGs into a scratch
/// directory under `target/`, then shells out to `iconutil -c icns` to
/// assemble `assets/icon/humm.icns` — the one and only place this
/// workspace invokes `iconutil`. Run manually; `bundle` never regenerates
/// this file, and no unit test invokes this function.
fn cmd_gen_icon() -> Result<(), String> {
    let root = repo_root();
    let iconset_dir = root.join("target/xtask-icon/humm.iconset");
    if iconset_dir.exists() {
        std::fs::remove_dir_all(&iconset_dir)
            .map_err(|e| format!("failed to clear stale iconset dir: {e}"))?;
    }
    icon::generate_iconset(&iconset_dir).map_err(|e| format!("failed to render iconset: {e}"))?;

    let out = root.join("assets/icon/humm.icns");
    std::fs::create_dir_all(out.parent().expect("assets/icon has a parent"))
        .map_err(|e| format!("failed to create assets/icon: {e}"))?;

    let status = Command::new("iconutil")
        .args([
            "-c",
            "icns",
            iconset_dir.to_str().expect("utf8 path"),
            "-o",
            out.to_str().expect("utf8 path"),
        ])
        .status()
        .map_err(|e| format!("failed to launch iconutil: {e}"))?;
    if !status.success() {
        return Err(format!("iconutil failed: {status}"));
    }

    println!("wrote {}", out.display());
    Ok(())
}

/// `cargo xtask version-info --notes <file> [--out <path>]`: reads `notes`
/// from `<file>`, renders `version_info.json` via [`version_info::render`]
/// against xtask's own crate version (the workspace version — see that
/// function's doc comment), and writes it to `<path>` if `--out` is given,
/// or prints it to stdout otherwise.
fn cmd_version_info(args: &[String]) -> Result<(), String> {
    let mut notes_path: Option<&str> = None;
    let mut out_path: Option<&str> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--notes" => {
                notes_path = Some(
                    args.get(i + 1)
                        .ok_or("--notes requires a file path argument")?,
                );
                i += 2;
            }
            "--out" => {
                out_path = Some(args.get(i + 1).ok_or("--out requires a path argument")?);
                i += 2;
            }
            other => return Err(format!("unrecognized argument `{other}`\n\n{USAGE}")),
        }
    }

    let notes_path = notes_path.ok_or_else(|| format!("--notes <file> is required\n\n{USAGE}"))?;
    let notes = std::fs::read_to_string(notes_path)
        .map_err(|e| format!("failed to read notes file {notes_path}: {e}"))?;

    let json = version_info::render(env!("CARGO_PKG_VERSION"), notes.trim_end());

    match out_path {
        Some(path) => {
            std::fs::write(path, &json).map_err(|e| format!("failed to write {path}: {e}"))?;
            println!("wrote {path}");
        }
        None => println!("{json}"),
    }
    Ok(())
}
