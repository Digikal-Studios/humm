//! `cargo xtask`: dev-only release tooling (Task 10/11 briefs; ADR-025).
//! Never shipped — this binary is excluded from the app bundle it builds
//! and is not published (`publish = false`).
//!
//! Subcommands (aliased via the repo-root `.cargo/config.toml`, so `cargo
//! xtask <subcommand>` works from anywhere in the workspace):
//!
//! - `bundle [--identity "Developer ID Application: …"]` — builds release
//!   `humm-gui`/`humm`, assembles `target/bundle/humm.app`
//!   ([`bundle::assemble`]), fixes up its dylib closure
//!   ([`dylib::bundle_closure`]), signs it inside-out
//!   ([`dylib::sign_bundle`] — ad-hoc if `--identity` is absent, a real
//!   Developer ID with the hardened runtime if present), and packages
//!   `target/bundle/humm-<version>-macos-arm64.dmg` ([`dmg::create`]).
//! - `notarize --profile <keychain-profile> [--dmg <path>]` — wraps `xcrun
//!   notarytool submit --wait` + `xcrun stapler staple` on the release DMG.
//!   STOP-gate: the keychain profile is created by Jawad
//!   (`xcrun notarytool store-credentials`) — this performs a real
//!   submission to Apple's servers, only ever run at his explicit go.
//! - `gen-icon` — renders the committed `assets/icon/humm.icns` from
//!   scratch via `iconutil` (see [`icon`]). Run manually when the mark
//!   needs to change; `bundle` never regenerates it.
//! - `version-info --notes <file> [--out <path>]` — renders
//!   `version_info.json` (see [`version_info::render`]) to `--out`, or to
//!   stdout if `--out` is omitted.
//!
//! No subcommand here invokes an Apple CLI (`otool`, `iconutil`,
//! `install_name_tool`, `codesign`, `hdiutil`, …) from within a
//! `#[cfg(test)]` — those are exercised only by the real, manually-run
//! `bundle`/`gen-icon`/`notarize` commands, never by `cargo test`.

mod bundle;
mod dmg;
mod dylib;
mod icon;
mod plist;
mod version_info;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use anyhow::{Context, Result, anyhow, bail};

/// Absolute path, not a bare name — see [`dylib`]'s module doc for why:
/// this machine's `PATH` shadows some Apple CLIs (`otool`,
/// `install_name_tool`) with an older Anaconda toolchain. `xcrun` isn't one
/// of the shadowed ones, but hardcoding it too keeps every subprocess this
/// binary launches independent of `PATH` at invocation time.
const XCRUN: &str = "/usr/bin/xcrun";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("bundle") => cmd_bundle(&args[1..]),
        Some("notarize") => cmd_notarize(&args[1..]),
        Some("gen-icon") => cmd_gen_icon(),
        Some("version-info") => cmd_version_info(&args[1..]),
        Some(other) => Err(anyhow!("unknown subcommand `{other}`\n\n{USAGE}")),
        None => Err(anyhow!("{USAGE}")),
    };

    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("xtask: {err:#}");
            ExitCode::FAILURE
        }
    }
}

const USAGE: &str = "usage: cargo xtask <bundle [--identity <id>]|notarize --profile <profile>|gen-icon|version-info --notes <file> [--out <path>]>";

/// The repo root — `crates/xtask` is always exactly two directories below
/// it, so this resolves regardless of the process's current directory.
fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("crates/xtask is nested two directories below the repo root")
        .to_path_buf()
}

/// The release DMG's path for the workspace's current version — shared by
/// `bundle` (which creates it) and `notarize`'s default (which submits it).
fn default_dmg_path(staging: &Path) -> PathBuf {
    staging.join(format!(
        "humm-{}-macos-arm64.dmg",
        env!("CARGO_PKG_VERSION")
    ))
}

/// `cargo xtask bundle [--identity <id>]`: release-builds `humm-gui` +
/// `humm`, assembles `target/bundle/humm.app` ([`bundle::assemble`]),
/// fixes up its dylib closure ([`dylib::bundle_closure`] — copies every
/// non-system dylib into `Contents/Frameworks`, rewrites every reference,
/// asserts no external reference remains), signs it inside-out
/// ([`dylib::sign_bundle`]), and packages the release DMG ([`dmg::create`]).
fn cmd_bundle(args: &[String]) -> Result<()> {
    let identity = parse_identity_flag(args)?;
    let root = repo_root();

    let status = Command::new("cargo")
        .args(["build", "--release", "-p", "humm-gui", "-p", "humm-cli"])
        .current_dir(&root)
        .status()
        .context("failed to launch `cargo build --release`")?;
    if !status.success() {
        bail!("release build failed: {status}");
    }

    let bin_dir = root.join("target/release");
    let staging = root.join("target/bundle");

    // Assembling from scratch each time avoids stale files left over from a
    // previous layout (e.g. a resource that a later task renames) or a
    // previous closure/signing pass over now-stale binaries.
    let app = staging.join("humm.app");
    if app.exists() {
        fs_remove_dir_all(&app)?;
    }

    let app = bundle::assemble(&bin_dir, &staging).context("assemble failed")?;
    println!("assembled {}", app.display());

    let report = dylib::bundle_closure(&app).context("dylib closure failed")?;
    println!(
        "dylib closure: {} dylib(s) copied into Contents/Frameworks",
        report.copied.len()
    );
    for path in &report.copied {
        println!("  {}", path.display());
    }

    dylib::sign_bundle(&app, identity.as_deref()).context("signing failed")?;
    println!(
        "signed bundle ({})",
        identity.as_deref().unwrap_or("ad-hoc")
    );

    let dmg_path = default_dmg_path(&staging);
    dmg::create(&app, &dmg_path, "humm").context("dmg creation failed")?;
    println!("created {}", dmg_path.display());

    Ok(())
}

fn fs_remove_dir_all(path: &Path) -> Result<()> {
    std::fs::remove_dir_all(path)
        .with_context(|| format!("failed to clear stale {}", path.display()))
}

fn parse_identity_flag(args: &[String]) -> Result<Option<String>> {
    let mut identity = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--identity" => {
                identity = Some(
                    args.get(i + 1)
                        .cloned()
                        .ok_or_else(|| anyhow!("--identity requires a value"))?,
                );
                i += 2;
            }
            other => bail!("unrecognized argument `{other}` for bundle\n\n{USAGE}"),
        }
    }
    Ok(identity)
}

const NOTARIZE_HELP: &str = "\
usage: cargo xtask notarize --profile <keychain-profile> [--dmg <path>]

Submits target/bundle/humm-<version>-macos-arm64.dmg (or --dmg <path>) to
Apple's notary service via `xcrun notarytool submit --keychain-profile
<profile> --wait`, then staples the resulting ticket via `xcrun stapler
staple`.

STOP-gate: <keychain-profile> must already exist in this Mac's keychain,
created once via `xcrun notarytool store-credentials <profile>` — that is
Jawad's own step, not this tool's, and requires an Apple Developer ID plus
an app-specific password or API key. This subcommand performs a real
submission to Apple's servers the moment it runs (`--wait` blocks for the
outcome) — only ever run it at Jawad's explicit go.";

/// `cargo xtask notarize --profile <keychain-profile> [--dmg <path>]`:
/// wraps `xcrun notarytool submit --wait` + `xcrun stapler staple` on the
/// release DMG. See [`NOTARIZE_HELP`] for the STOP-gate this subcommand
/// exists behind.
fn cmd_notarize(args: &[String]) -> Result<()> {
    let mut profile: Option<String> = None;
    let mut dmg_arg: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                println!("{NOTARIZE_HELP}");
                return Ok(());
            }
            "--profile" => {
                profile = Some(
                    args.get(i + 1)
                        .cloned()
                        .ok_or_else(|| anyhow!("--profile requires a keychain-profile name"))?,
                );
                i += 2;
            }
            "--dmg" => {
                dmg_arg = Some(
                    args.get(i + 1)
                        .cloned()
                        .ok_or_else(|| anyhow!("--dmg requires a path"))?,
                );
                i += 2;
            }
            other => bail!("unrecognized argument `{other}`\n\n{NOTARIZE_HELP}"),
        }
    }

    let profile = profile
        .ok_or_else(|| anyhow!("--profile <keychain-profile> is required\n\n{NOTARIZE_HELP}"))?;
    let dmg_path = match dmg_arg {
        Some(path) => PathBuf::from(path),
        None => default_dmg_path(&repo_root().join("target/bundle")),
    };
    if !dmg_path.exists() {
        bail!(
            "{} does not exist — run `cargo xtask bundle` first, or pass --dmg <path>",
            dmg_path.display()
        );
    }

    println!(
        "submitting {} for notarization (keychain profile `{profile}`)...",
        dmg_path.display()
    );
    let status = Command::new(XCRUN)
        .args([
            "notarytool",
            "submit",
            "--keychain-profile",
            &profile,
            "--wait",
        ])
        .arg(&dmg_path)
        .status()
        .context("failed to launch `xcrun notarytool`")?;
    if !status.success() {
        bail!("xcrun notarytool submit failed: {status}");
    }

    println!("stapling {}...", dmg_path.display());
    let status = Command::new(XCRUN)
        .args(["stapler", "staple"])
        .arg(&dmg_path)
        .status()
        .context("failed to launch `xcrun stapler`")?;
    if !status.success() {
        bail!("xcrun stapler staple failed: {status}");
    }

    println!("notarized and stapled {}", dmg_path.display());
    Ok(())
}

/// `cargo xtask gen-icon`: renders the iconset PNGs into a scratch
/// directory under `target/`, then shells out to `iconutil -c icns` to
/// assemble `assets/icon/humm.icns` — the one and only place this
/// workspace invokes `iconutil`. Run manually; `bundle` never regenerates
/// this file, and no unit test invokes this function.
fn cmd_gen_icon() -> Result<()> {
    let root = repo_root();
    let iconset_dir = root.join("target/xtask-icon/humm.iconset");
    if iconset_dir.exists() {
        fs_remove_dir_all(&iconset_dir)?;
    }
    icon::generate_iconset(&iconset_dir).context("failed to render iconset")?;

    let out = root.join("assets/icon/humm.icns");
    std::fs::create_dir_all(out.parent().expect("assets/icon has a parent"))
        .context("failed to create assets/icon")?;

    let status = Command::new("iconutil")
        .args([
            "-c",
            "icns",
            iconset_dir.to_str().expect("utf8 path"),
            "-o",
            out.to_str().expect("utf8 path"),
        ])
        .status()
        .context("failed to launch iconutil")?;
    if !status.success() {
        bail!("iconutil failed: {status}");
    }

    println!("wrote {}", out.display());
    Ok(())
}

/// `cargo xtask version-info --notes <file> [--out <path>]`: reads `notes`
/// from `<file>`, renders `version_info.json` via [`version_info::render`]
/// against xtask's own crate version (the workspace version — see that
/// function's doc comment), and writes it to `<path>` if `--out` is given,
/// or prints it to stdout otherwise.
fn cmd_version_info(args: &[String]) -> Result<()> {
    let mut notes_path: Option<&str> = None;
    let mut out_path: Option<&str> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--notes" => {
                notes_path = Some(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow!("--notes requires a file path argument"))?,
                );
                i += 2;
            }
            "--out" => {
                out_path = Some(
                    args.get(i + 1)
                        .ok_or_else(|| anyhow!("--out requires a path argument"))?,
                );
                i += 2;
            }
            other => bail!("unrecognized argument `{other}`\n\n{USAGE}"),
        }
    }

    let notes_path = notes_path.ok_or_else(|| anyhow!("--notes <file> is required\n\n{USAGE}"))?;
    let notes = std::fs::read_to_string(notes_path)
        .with_context(|| format!("failed to read notes file {notes_path}"))?;

    let json = version_info::render(env!("CARGO_PKG_VERSION"), notes.trim_end());

    match out_path {
        Some(path) => {
            std::fs::write(path, &json).with_context(|| format!("failed to write {path}"))?;
            println!("wrote {path}");
        }
        None => println!("{json}"),
    }
    Ok(())
}
