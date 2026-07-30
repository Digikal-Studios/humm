//! Dylib closure walk + rewrite + ad-hoc signing (Task 11 brief, spec §1.3
//! item 3; ADR-024's "closed by ADR-025/M9" note; this module is the Task
//! 11 addendum to ADR-025).
//!
//! `humm-gui` links `/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib` by
//! **absolute path** (ADR-024) — no `@rpath` indirection — so a plain
//! `cargo build --release` binary only launches on a machine with
//! Homebrew's OpenBLAS installed at that exact prefix. This module fixes
//! that for the *bundled* app: it walks the transitive `otool -L` closure
//! from both `Contents/MacOS` binaries, copies every non-system dylib it
//! finds into `Contents/Frameworks/` (deduped by filename — two binaries
//! depending on the same dylib get one copy), rewrites every reference to
//! a bundle-relative path, strips any absolute `LC_RPATH` a copied dylib
//! carries, and ad-hoc re-signs every Mach-O it modifies (arm64
//! hard-requires a valid signature after `install_name_tool`, or the
//! binary gets `SIGKILL`ed at exec).
//!
//! **The closure is two-layered, not one** — discovered running this for
//! real (ADR-025's Task 11 addendum has the full writeup). `otool -L`'s
//! absolute Homebrew entries (`libopenblas.0.dylib` → `libgfortran.5.dylib`
//! → …) are the obvious layer and [`parse_otool_libs`] finds them. But
//! Homebrew's own gcc keg links its *sibling* runtime dylibs
//! (`libgfortran.5.dylib` → `libquadmath.0.dylib`/`libgcc_s.1.1.dylib`) to
//! each other via `@rpath` plus a `@loader_path` `LC_RPATH` — already
//! "portable-looking" text that [`parse_otool_libs`] correctly leaves
//! alone, but the referenced sibling still has to physically travel into
//! `Contents/Frameworks/` too, or the reference dangles once the dylib
//! moves. [`parse_relative_dep_names`] finds *those* (as bare filenames —
//! resolving them to a real path requires knowing the referencing file's
//! original directory, which is [`bundle_closure`]'s job, not a pure
//! function's). And `libopenblas.0.dylib` itself was built carrying three
//! more `LC_RPATH` entries that are absolute Homebrew paths (leftover
//! search-path entries, not entries `otool -L` even shows) —
//! [`parse_otool_rpaths`] finds those, and [`bundle_closure`] deletes them,
//! since an unresolved `@rpath/foo.dylib` in *any* loaded image can fall
//! back to *any* image's `LC_RPATH` list, not just the referencing dylib's
//! own — leaving one absolute Homebrew rpath anywhere in the bundle would
//! silently keep the whole closure dependent on Homebrew being installed,
//! defeating the point.
//!
//! Structure, per the brief: **parsing** ([`parse_otool_libs`],
//! [`parse_relative_dep_names`], [`parse_otool_rpaths`],
//! [`parse_relative_ref_targets`]) and **planning** ([`plan_rewrites`],
//! [`claim_destination`]/[`files_have_identical_content`],
//! [`check_relative_refs_resolve`], [`sign_plan`]) are pure functions over
//! strings/paths (or, for the last few, real-but-cheap filesystem reads —
//! no Apple CLI) — all unit-tested without shelling out. [`bundle_closure`],
//! [`verify_no_external_refs`], and [`sign_bundle`]/[`sign_one`] are the
//! thin exec wrappers that actually shell out to
//! `otool`/`install_name_tool`/`codesign`; no `#[cfg(test)]` path ever
//! calls them (Task 10 environment note, still binding in Task 11).

use std::collections::{BTreeMap, VecDeque};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{Context, Result, bail};

/// Apple's own `otool`/`install_name_tool`/`codesign`, invoked by absolute
/// path rather than bare name. Discovered the hard way (ADR-025 Task 11
/// addendum): this machine's `PATH` puts an Anaconda-bundled `cctools`
/// (`otool`/`install_name_tool`, x86_64-only, `cctools-949.0.1`) ahead of
/// `/usr/bin`'s Apple-provided universal `cctools-1040` — a bare
/// `Command::new("otool")` would silently run the wrong, older toolchain
/// against our arm64 binaries. `codesign` isn't shadowed on this machine,
/// but it's hardcoded here too for the same reason: this is packaging
/// tooling, and "which binary actually ran" must never depend on whatever
/// `PATH` happens to be at invocation time.
const OTOOL: &str = "/usr/bin/otool";
const INSTALL_NAME_TOOL: &str = "/usr/bin/install_name_tool";
const CODESIGN: &str = "/usr/bin/codesign";

/// Which rewritten form a dependency reference gets, depending on what kind
/// of Mach-O references it (spec §1.3 item 3): a main binary in
/// `Contents/MacOS/` rewrites to `@executable_path/../Frameworks/<name>`;
/// a dylib in `Contents/Frameworks/` referencing another dylib rewrites to
/// `@loader_path/<name>`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RefKind {
    MainBinary,
    Dylib,
}

/// One `install_name_tool -change <old> <new>` operation still needed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RewriteOp {
    pub old: PathBuf,
    pub new: String,
}

/// What [`bundle_closure`] actually did — recorded for the ADR-025 addendum
/// and the task report, not just for its own sake.
#[derive(Debug, Default, Clone)]
pub struct ClosureReport {
    /// Every dylib copied into `Contents/Frameworks/`, by its original
    /// absolute source path, in BFS discovery order. One entry per unique
    /// filename — a dylib depended on by both main binaries appears once.
    pub copied: Vec<PathBuf>,
    /// `(file, rpath)` pairs where an absolute, non-system `LC_RPATH` was
    /// deleted from an already-copied dylib (the `libopenblas.0.dylib`
    /// finding — see the module doc).
    pub stripped_rpaths: Vec<(PathBuf, String)>,
}

/// The queried file's own basename — read off `otool`'s first output line
/// (the file path passed on the command line, followed by `:`). Used to
/// recognize a dylib's self-referential dependency entry (its own
/// `LC_ID_DYLIB` commonly reappears as the first entry in its own
/// dependency list) **by filename, not by exact path text** — the entry
/// can legitimately be spelled differently from the queried path (an
/// `@rpath/…` install name, or a different symlink alias of the same real
/// file, e.g. Homebrew's `opt/gcc/...` vs `Cellar/gcc/16.1.0/...`), so
/// comparing full path strings would under-recognize self-references.
fn self_basename(output: &str) -> Option<&str> {
    let header = output.lines().next()?.strip_suffix(':')?.trim();
    Path::new(header).file_name()?.to_str()
}

/// The bare filename of a dependency entry, whether it's an absolute path
/// or an `@rpath/`/`@loader_path/`/`@executable_path/`-prefixed one.
fn entry_basename(entry: &str) -> Option<&str> {
    entry.rsplit('/').next()
}

/// Parses `otool -L <file>` output, returning every dependency that still
/// needs fixing up: an absolute path outside `/usr/lib/` and `/System/`
/// (always-present system dylibs, never bundled) and not the file's own
/// self-referential entry (matched by filename — see [`self_basename`]).
///
/// Anything not starting with `/` (`@rpath/…`, `@loader_path/…`,
/// `@executable_path/…`) is left alone here — it's either a framework
/// alias, already fixed by a previous [`bundle_closure`] run, or (the
/// module doc's finding) a same-directory sibling reference that
/// [`parse_relative_dep_names`] handles instead. Leaving it alone is what
/// makes the closure step idempotent: a rewritten reference never comes
/// back out of this function, so [`plan_rewrites`] never re-plans it.
pub fn parse_otool_libs(output: &str) -> Vec<PathBuf> {
    let self_name = self_basename(output);
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let entry = line.trim().split(" (").next()?.trim();
            if entry.is_empty() || !entry.starts_with('/') {
                return None;
            }
            if entry_basename(entry) == self_name {
                return None;
            }
            if entry.starts_with("/usr/lib/") || entry.starts_with("/System/") {
                return None;
            }
            Some(PathBuf::from(entry))
        })
        .collect()
}

/// Bare filenames of dependency entries already in relative
/// (`@rpath/`/`@loader_path/`/`@executable_path/`-prefixed) form, minus the
/// self-referential entry. These never need rewriting, but the dylib they
/// name may not have been pulled into the bundle yet — resolving that
/// (checking a candidate directory for a same-named file) needs the
/// filesystem, so it's [`bundle_closure`]'s job, not this pure function's.
pub fn parse_relative_dep_names(output: &str) -> Vec<String> {
    let self_name = self_basename(output);
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let entry = line.trim().split(" (").next()?.trim();
            if entry.is_empty() || entry.starts_with('/') {
                return None;
            }
            let name = entry_basename(entry)?;
            if Some(name) == self_name {
                return None;
            }
            Some(name.to_string())
        })
        .collect()
}

/// Which relative-reference prefix a dependency entry uses — the two forms
/// [`verify_no_external_refs`]'s post-condition resolves and checks for
/// existence (task-11 review finding: the closure walk copies whatever it
/// finds, but nothing previously confirmed the *reference itself* resolves
/// once the file lands in the bundle). `@rpath/<name>` resolves against
/// `Contents/Frameworks/` — every bundled dylib's own `LC_RPATH` is either
/// deleted (if absolute-external, [`strip_external_rpaths`]) or left as
/// `@loader_path` (module doc), and `@loader_path` itself always means
/// "next to the file that's loading it", which for every bundled dylib is
/// `Contents/Frameworks/` too. `@loader_path/<name>` is checked directly
/// against the referencing file's own directory, without assuming that
/// directory is `Frameworks/` (a main binary in `Contents/MacOS/` could in
/// principle carry one). `@executable_path/…` entries are
/// [`plan_rewrites`]'s own main-binary rewrite target and resolve
/// correctly by construction of the bundle layout, so they're not
/// re-checked here.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RelPrefix {
    Rpath,
    LoaderPath,
}

/// Bare filenames of `@rpath/`/`@loader_path/`-prefixed dependency entries
/// (minus the self-referential entry — see [`self_basename`]), tagged with
/// which prefix each used. Pure text parsing, like [`parse_otool_libs`] and
/// [`parse_relative_dep_names`]; [`check_relative_refs_resolve`] (given the
/// filesystem) decides whether each one actually resolves.
fn parse_relative_ref_targets(output: &str) -> Vec<(RelPrefix, String)> {
    let self_name = self_basename(output);
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let entry = line.trim().split(" (").next()?.trim();
            let prefix = if entry.starts_with("@rpath/") {
                RelPrefix::Rpath
            } else if entry.starts_with("@loader_path/") {
                RelPrefix::LoaderPath
            } else {
                return None;
            };
            let name = entry_basename(entry)?;
            if Some(name) == self_name {
                return None;
            }
            Some((prefix, name.to_string()))
        })
        .collect()
}

/// For each `(prefix, name)` target found in `file`'s own dependency list
/// ([`parse_relative_ref_targets`]), checks whether the file it names
/// actually exists where that prefix says it should — `frameworks` for
/// `@rpath`, next to `file` itself for `@loader_path` — and returns one
/// "offender" string per target that doesn't. Split out as its own
/// filesystem-only (no `otool`/Mach-O) function so this resolution logic is
/// unit-testable against a constructed tempdir, independent of
/// [`verify_no_external_refs`]'s real `otool` invocation.
fn check_relative_refs_resolve(
    file: &Path,
    targets: &[(RelPrefix, String)],
    frameworks: &Path,
) -> Vec<String> {
    targets
        .iter()
        .filter_map(|(prefix, name)| {
            let expected = match prefix {
                RelPrefix::Rpath => frameworks.join(name),
                RelPrefix::LoaderPath => match file.parent() {
                    Some(dir) => dir.join(name),
                    None => PathBuf::from(name),
                },
            };
            if expected.is_file() {
                return None;
            }
            let prefix_str = match prefix {
                RelPrefix::Rpath => "@rpath",
                RelPrefix::LoaderPath => "@loader_path",
            };
            Some(format!(
                "{}: dangling {prefix_str}/{name} reference — expected {}",
                file.display(),
                expected.display()
            ))
        })
        .collect()
}

/// Parses `otool -l <file>` output for every `LC_RPATH` load command's
/// `path` value, verbatim (the exact text `install_name_tool -delete_rpath`
/// expects). Each `LC_RPATH` block looks like:
///
/// ```text
///           cmd LC_RPATH
///       cmdsize 32
///          path @loader_path (offset 12)
/// ```
pub fn parse_otool_rpaths(output: &str) -> Vec<String> {
    let mut rpaths = Vec::new();
    let mut lines = output.lines();
    while let Some(line) = lines.next() {
        if line.trim() != "cmd LC_RPATH" {
            continue;
        }
        // The `path <value> (offset N)` line follows `cmdsize` within the
        // next couple of lines; stop looking once this block is exhausted.
        for candidate in lines.by_ref().take(2) {
            let candidate = candidate.trim();
            if let Some(rest) = candidate.strip_prefix("path ") {
                let value = rest.rsplit_once(" (offset").map_or(rest, |(v, _)| v);
                rpaths.push(value.trim().to_string());
                break;
            }
        }
    }
    rpaths
}

/// Whether an `LC_RPATH` value (or a dependency path) is an absolute,
/// non-system path that must not survive in a shipped bundle — the same
/// "outside `/usr/lib/` and `/System/`" rule [`parse_otool_libs`] applies
/// to dependency entries, applied here to rpath *search paths* too (a
/// directory, not a specific dylib file, but the same rule: system search
/// paths are always present, anything else absolute is a machine-specific
/// leak).
fn is_external_absolute(path: &str) -> bool {
    path.starts_with('/') && !path.starts_with("/usr/lib/") && !path.starts_with("/System/")
}

/// Builds the `install_name_tool -change` plan for `libs` — the output of
/// [`parse_otool_libs`] — targeting the bundle-relative form `kind` calls
/// for. Already-rewritten references never reach here (they never survive
/// [`parse_otool_libs`]'s filtering), so calling this on a fixture whose
/// `otool -L` text is already fully rewritten naturally produces an empty
/// plan — the idempotency the brief asks for
/// (`closure_rewrite_plan_is_idempotent`).
pub fn plan_rewrites(libs: &[PathBuf], kind: RefKind) -> Vec<RewriteOp> {
    libs.iter()
        .filter_map(|old| {
            let name = old.file_name()?.to_str()?;
            let new = match kind {
                RefKind::MainBinary => format!("@executable_path/../Frameworks/{name}"),
                RefKind::Dylib => format!("@loader_path/{name}"),
            };
            Some(RewriteOp {
                old: old.clone(),
                new,
            })
        })
        .collect()
}

/// Shells out to `otool -L <path>`, returning stdout as a string.
fn run_otool(path: &Path) -> Result<String> {
    let output = Command::new(OTOOL)
        .arg("-L")
        .arg(path)
        .output()
        .with_context(|| format!("failed to launch `otool -L {}`", path.display()))?;
    if !output.status.success() {
        bail!("otool -L {} failed: {}", path.display(), output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Shells out to `otool -l <path>` (load commands), returning stdout as a
/// string — used to read `LC_RPATH` entries ([`parse_otool_rpaths`]).
fn run_otool_load_commands(path: &Path) -> Result<String> {
    let output = Command::new(OTOOL)
        .arg("-l")
        .arg(path)
        .output()
        .with_context(|| format!("failed to launch `otool -l {}`", path.display()))?;
    if !output.status.success() {
        bail!("otool -l {} failed: {}", path.display(), output.status);
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// Shells out to `install_name_tool <args...> <path>`. Failure is reported
/// with a clear, non-retrying message: `install_name_tool` grows a Mach-O's
/// load-command table in place, and if a binary's reserved header space
/// runs out it errors — looping/retrying in place can't fix that; a fresh
/// `cargo xtask bundle` (fresh binaries from a fresh `cargo build`) is the
/// only way forward, per the task brief.
fn run_install_name_tool(path: &Path, args: &[&str]) -> Result<()> {
    let status = Command::new(INSTALL_NAME_TOOL)
        .args(args)
        .arg(path)
        .status()
        .with_context(|| format!("failed to launch install_name_tool on {}", path.display()))?;
    if !status.success() {
        bail!(
            "install_name_tool {args:?} {} failed: {status} — if this is a load-command \
             table overflow, re-run `cargo xtask bundle` from scratch (fresh binaries have \
             fresh header room); this tool does not retry in place",
            path.display()
        );
    }
    Ok(())
}

/// Ad-hoc-signs `path` (`codesign --force -s -`) — mandatory on arm64 for
/// any Mach-O `install_name_tool` just modified, or it gets `SIGKILL`ed at
/// exec.
fn codesign_adhoc(path: &Path) -> Result<()> {
    let status = Command::new(CODESIGN)
        .args(["--force", "-s", "-"])
        .arg(path)
        .status()
        .with_context(|| format!("failed to launch codesign on {}", path.display()))?;
    if !status.success() {
        bail!("codesign --force -s - {} failed: {status}", path.display());
    }
    Ok(())
}

/// Copies `src` into `dest` (must not already exist as a destination the
/// caller intends to write), makes it writable (Homebrew's own dylibs are
/// typically read-only), sets its install name to `@rpath/<name>`, and
/// ad-hoc-signs the copy. Shared by both discovery paths in
/// [`bundle_closure`] (an absolute-dependency copy and a same-directory
/// sibling copy) — the src differs, the prep is identical.
fn copy_and_prepare_dylib(src: &Path, dest: &Path) -> Result<()> {
    fs::copy(src, dest)
        .with_context(|| format!("failed to copy {} to {}", src.display(), dest.display()))?;

    let mut perms = fs::metadata(dest)
        .with_context(|| format!("failed to stat {}", dest.display()))?
        .permissions();
    perms.set_mode(perms.mode() | 0o200);
    fs::set_permissions(dest, perms)
        .with_context(|| format!("failed to chmod {}", dest.display()))?;

    let name = dest
        .file_name()
        .and_then(|n| n.to_str())
        .with_context(|| format!("non-utf8 dylib filename: {}", dest.display()))?;
    let id_flag = format!("@rpath/{name}");
    run_install_name_tool(dest, &["-id", &id_flag])?;
    codesign_adhoc(dest)?;
    Ok(())
}

/// Byte-for-byte comparison of two files — used by [`claim_destination`] to
/// decide whether a `Contents/Frameworks/<name>` filename collision (two
/// different source paths both wanting to land at the same destination) is
/// a harmless re-discovery of the identical dylib (safe to skip) or a real
/// ambiguity (must error). These are dylibs, always well under 100MB, so
/// reading both fully in one shot is cheap — no need for a
/// streaming/chunked comparison or a hash.
fn files_have_identical_content(a: &Path, b: &Path) -> Result<bool> {
    let a_bytes = fs::read(a).with_context(|| format!("failed to read {}", a.display()))?;
    let b_bytes = fs::read(b).with_context(|| format!("failed to read {}", b.display()))?;
    Ok(a_bytes == b_bytes)
}

/// Records that `dest` (a `Contents/Frameworks/<name>` destination path) is
/// claimed by `src`, tracking *which* source path claimed it — not just
/// that the destination name is taken (task-11 review finding: dedupe-by-
/// filename alone silently merges two different files that happen to share
/// a basename). Returns:
///
/// - `Ok(true)` — `dest` was unclaimed; the caller must physically copy
///   `src` in.
/// - `Ok(false)` — `dest` was already claimed by a source that is
///   byte-identical to `src` (the common, legitimate case: two binaries, or
///   two dylibs, depending on the same real file) — safe to skip.
/// - `Err` — `dest` was already claimed by a source with *different*
///   content: an ambiguous closure. A release must not silently pick one
///   file over the other, so this fails the build naming both sources and
///   the colliding basename.
fn claim_destination(
    queued: &mut BTreeMap<PathBuf, PathBuf>,
    dest: &Path,
    src: &Path,
) -> Result<bool> {
    if let Some(existing_src) = queued.get(dest) {
        if existing_src == src {
            return Ok(false);
        }
        if files_have_identical_content(existing_src, src)? {
            return Ok(false);
        }
        bail!(
            "dylib closure collision: {} and {} are different files but both would be copied \
             to {} — a release with an ambiguous closure must not build silently",
            existing_src.display(),
            src.display(),
            dest.display()
        );
    }
    queued.insert(dest.to_path_buf(), src.to_path_buf());
    Ok(true)
}

/// Deletes every absolute, non-system `LC_RPATH` from `path` (the
/// `libopenblas.0.dylib` finding — see the module doc), recording each one
/// into `report`. Returns whether anything was deleted, so the caller
/// knows whether `path` needs re-signing.
fn strip_external_rpaths(path: &Path, report: &mut ClosureReport) -> Result<bool> {
    let load_commands = run_otool_load_commands(path)?;
    let mut stripped = false;
    for rpath in parse_otool_rpaths(&load_commands) {
        if is_external_absolute(&rpath) {
            run_install_name_tool(path, &["-delete_rpath", &rpath])?;
            report.stripped_rpaths.push((path.to_path_buf(), rpath));
            stripped = true;
        }
    }
    Ok(stripped)
}

/// Walks the transitive dylib closure of every binary in
/// `<app>/Contents/MacOS/`, copying each non-system dependency — whether
/// referenced absolutely or as an already-relative same-directory sibling
/// (the module doc's two-layer finding) — into `<app>/Contents/Frameworks/`
/// (deduped by filename), rewriting every absolute reference to the
/// bundle-relative form, stripping any absolute `LC_RPATH` a copied dylib
/// carries, and ad-hoc-signing every modified Mach-O. Asserts its own
/// post-condition before returning ([`verify_no_external_refs`]): if any
/// `/opt/homebrew`/other non-system absolute reference (dependency *or*
/// rpath) remains anywhere in the bundle, this returns `Err` rather than
/// silently shipping a broken app.
pub fn bundle_closure(app: &Path) -> Result<ClosureReport> {
    let macos = app.join("Contents/MacOS");
    let frameworks = app.join("Contents/Frameworks");
    fs::create_dir_all(&frameworks)
        .with_context(|| format!("failed to create {}", frameworks.display()))?;

    let main_binaries: Vec<PathBuf> = fs::read_dir(&macos)
        .with_context(|| format!("failed to read {}", macos.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();

    let mut report = ClosureReport::default();
    // Dedup dylib copies by destination path (i.e. by filename) — two
    // binaries (or two dylibs) depending on the same library must not copy
    // it twice or re-walk its own closure twice. Maps destination path to
    // the *source* path that claimed it, so a same-basename-but-different-
    // source collision can be detected instead of silently merged
    // ([`claim_destination`]).
    let mut queued: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
    // (bundle path to scan, the *original* directory a same-directory
    // sibling reference should resolve against, kind). For a main binary
    // this is its own directory (main binaries have no such siblings in
    // practice, but the value has to be something); for a copied dylib
    // it's the directory its own source came from (Homebrew keeps sibling
    // runtime dylibs together, so a `@rpath` sibling of `X` is found next
    // to wherever `X` itself was copied from — not next to `X`'s new
    // bundle location, which is exactly the chicken-and-egg this avoids).
    let mut frontier: VecDeque<(PathBuf, PathBuf, RefKind)> = main_binaries
        .into_iter()
        .map(|path| {
            let dir = path
                .parent()
                .expect("MacOS entries have a parent")
                .to_path_buf();
            (path, dir, RefKind::MainBinary)
        })
        .collect();

    while let Some((file, original_dir, kind)) = frontier.pop_front() {
        let otool_output = run_otool(&file)?;
        let libs = parse_otool_libs(&otool_output);
        let ops = plan_rewrites(&libs, kind);
        let relative_names = parse_relative_dep_names(&otool_output);

        let mut file_modified = false;

        for op in &ops {
            let name = op
                .old
                .file_name()
                .and_then(|n| n.to_str())
                .with_context(|| format!("non-utf8 dylib filename: {}", op.old.display()))?;
            let dest = frameworks.join(name);
            if claim_destination(&mut queued, &dest, &op.old)? {
                copy_and_prepare_dylib(&op.old, &dest)?;
                report.copied.push(op.old.clone());
                let dest_dir = op
                    .old
                    .parent()
                    .expect("an absolute dylib path has a parent")
                    .to_path_buf();
                frontier.push_back((dest, dest_dir, RefKind::Dylib));
            }
        }

        for name in &relative_names {
            let dest = frameworks.join(name);
            let candidate = original_dir.join(name);
            if !candidate.is_file() {
                continue;
            }
            if claim_destination(&mut queued, &dest, &candidate)? {
                copy_and_prepare_dylib(&candidate, &dest)?;
                report.copied.push(candidate.clone());
                frontier.push_back((dest, original_dir.clone(), RefKind::Dylib));
            }
        }

        if !ops.is_empty() {
            for op in &ops {
                let old_str = op
                    .old
                    .to_str()
                    .with_context(|| format!("non-utf8 path: {}", op.old.display()))?;
                run_install_name_tool(&file, &["-change", old_str, &op.new])?;
            }
            file_modified = true;
        }

        if strip_external_rpaths(&file, &mut report)? {
            file_modified = true;
        }

        if file_modified {
            codesign_adhoc(&file)?;
        }
    }

    verify_no_external_refs(app)?;
    Ok(report)
}

/// Post-condition [`bundle_closure`] asserts on itself before returning:
/// re-walks every Mach-O under `Contents/MacOS/` and `Contents/Frameworks/`
/// and fails, listing every offender, if any still has a non-system
/// absolute dependency ([`parse_otool_libs`]) *or* a non-system absolute
/// `LC_RPATH` ([`parse_otool_rpaths`] + [`is_external_absolute`]) — the
/// module doc's two ways a Homebrew path can hide in an otherwise
/// "portable-looking" bundle — *or* a `@rpath`/`@loader_path` dependency
/// entry whose target doesn't actually exist where that prefix resolves
/// ([`parse_relative_ref_targets`] + [`check_relative_refs_resolve`]): a
/// dangling relative reference that copied cleanly through the closure walk
/// (nothing here was ever an absolute Homebrew path) but would still fail
/// to resolve at `dyld` launch time, so this asserts it as a build-time
/// verify error instead of a launch-time crash.
pub fn verify_no_external_refs(app: &Path) -> Result<()> {
    let mut offenders = Vec::new();
    let frameworks = app.join("Contents/Frameworks");

    for subdir in ["Contents/MacOS", "Contents/Frameworks"] {
        let dir = app.join(subdir);
        if !dir.exists() {
            continue;
        }
        for entry in
            fs::read_dir(&dir).with_context(|| format!("failed to read {}", dir.display()))?
        {
            let file = entry
                .with_context(|| format!("failed to read an entry of {}", dir.display()))?
                .path();
            if !file.is_file() {
                continue;
            }

            let deps = run_otool(&file)?;
            for lib in parse_otool_libs(&deps) {
                offenders.push(format!("{}: dependency {}", file.display(), lib.display()));
            }

            let load_commands = run_otool_load_commands(&file)?;
            for rpath in parse_otool_rpaths(&load_commands) {
                if is_external_absolute(&rpath) {
                    offenders.push(format!("{}: LC_RPATH {rpath}", file.display()));
                }
            }

            let relative_targets = parse_relative_ref_targets(&deps);
            offenders.extend(check_relative_refs_resolve(
                &file,
                &relative_targets,
                &frameworks,
            ));
        }
    }

    if !offenders.is_empty() {
        bail!(
            "dylib closure incomplete — external references remain:\n{}",
            offenders.join("\n")
        );
    }
    Ok(())
}

/// Final "sign inside-out" pass (spec §1.3 item 4): every dylib in
/// `Contents/Frameworks/` first, then every executable in
/// `Contents/MacOS/` individually, then the `.app` bundle as a whole
/// (which re-signs the bundle's primary `CFBundleExecutable` under the
/// bundle seal and covers `Info.plist`/`Resources/`).
///
/// The middle step is not redundant with the last: `codesign` on a bundle
/// only signs the one binary named by `CFBundleExecutable` in `Info.plist`
/// (`humm-gui`) and seals the rest by hash — it does **not** recursively
/// sign other executables sitting in `Contents/MacOS/` (task-11 review
/// finding). Without this step the secondary CLI binary
/// (`Contents/MacOS/humm`) keeps whatever ad-hoc signature `cargo build`
/// gave it; harmless under ad-hoc signing, but under a real `--identity` the
/// identity mismatch (build-time ad-hoc vs. the requested Developer ID)
/// would very likely fail notarization. Signing `humm-gui` here too (before
/// it's re-signed again as part of the bundle) is standard practice, not a
/// bug — codesign is idempotent per-file.
///
/// `identity` present → a real Developer ID identity with `--options
/// runtime` (hardened runtime, required for notarization); absent → ad-hoc
/// (`-s -`), consistent with [`bundle_closure`]'s own per-file ad-hoc
/// signing so an unsigned build stays internally consistent end to end.
pub fn sign_bundle(app: &Path, identity: Option<&str>) -> Result<()> {
    for path in sign_plan(app)? {
        sign_one(&path, identity)?;
    }
    Ok(())
}

/// The exact ordered list of paths [`sign_bundle`] signs: every file in
/// `Contents/Frameworks/` (sorted, for a deterministic plan), then every
/// file in `Contents/MacOS/` (sorted — this is what fixes the task-11
/// review finding: the secondary CLI binary gets signed here, individually,
/// not just swept up by the final bundle-level sign), then the `.app`
/// bundle itself, last. Split out from [`sign_bundle`] as a pure
/// (filesystem-reading, no `codesign` exec) planning step specifically so
/// this ordering is unit-testable without a real signing identity or a
/// sandbox lift — `sign_bundle`/`sign_one` remain the exec-wrapper code no
/// `#[cfg(test)]` path calls (module doc, Task 10 environment note, still
/// binding).
fn sign_plan(app: &Path) -> Result<Vec<PathBuf>> {
    let mut plan = Vec::new();

    let frameworks = app.join("Contents/Frameworks");
    if frameworks.exists() {
        let mut entries: Vec<PathBuf> = fs::read_dir(&frameworks)
            .with_context(|| format!("failed to read {}", frameworks.display()))?
            .filter_map(|entry| entry.ok())
            .map(|entry| entry.path())
            .filter(|path| path.is_file())
            .collect();
        entries.sort();
        plan.extend(entries);
    }

    let macos = app.join("Contents/MacOS");
    let mut entries: Vec<PathBuf> = fs::read_dir(&macos)
        .with_context(|| format!("failed to read {}", macos.display()))?
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.path())
        .filter(|path| path.is_file())
        .collect();
    entries.sort();
    plan.extend(entries);

    plan.push(app.to_path_buf());
    Ok(plan)
}

fn sign_one(path: &Path, identity: Option<&str>) -> Result<()> {
    let mut cmd = Command::new(CODESIGN);
    cmd.arg("--force");
    match identity {
        Some(id) => {
            cmd.args(["--options", "runtime", "-s", id]);
        }
        None => {
            cmd.args(["-s", "-"]);
        }
    }
    cmd.arg(path);
    let status = cmd
        .status()
        .with_context(|| format!("failed to launch codesign on {}", path.display()))?;
    if !status.success() {
        bail!("codesign {} failed: {status}", path.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    /// Canned `otool -L` text: a self-id first line, a `/usr/lib/libSystem`
    /// line, a `/System/Library` line, an `/opt/homebrew/opt/openblas`
    /// line, and an `@rpath` line. Only the homebrew path is a dependency
    /// that still needs fixing — everything else is either the file's own
    /// identity, a system dylib, or already bundle-relative.
    #[test]
    fn parse_skips_system_and_self_entries() {
        let output = "target/release/humm-gui:\n\
             \t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n\
             \t/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation (compatibility version 150.0.0, current version 1953.0.0)\n\
             \t/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib (compatibility version 0.0.0, current version 0.0.0)\n\
             \t@rpath/libomp.dylib (compatibility version 5.0.0, current version 5.0.0)\n";

        let libs = parse_otool_libs(output);

        assert_eq!(
            libs,
            vec![PathBuf::from(
                "/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib"
            )]
        );
    }

    /// A dylib's own `otool -L` output repeats its install name as its
    /// first dependency entry (`LC_ID_DYLIB` showing up in its own list) —
    /// that self-referential entry must be skipped too, not just the
    /// header line.
    #[test]
    fn parse_skips_dylib_self_referential_dependency_entry() {
        let output = "/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib:\n\
             \t/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib (compatibility version 0.0.0, current version 0.0.0)\n\
             \t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n\
             \t/opt/homebrew/opt/gcc/lib/gcc/current/libgfortran.5.dylib (compatibility version 6.0.0, current version 6.0.0)\n";

        let libs = parse_otool_libs(output);

        assert_eq!(
            libs,
            vec![PathBuf::from(
                "/opt/homebrew/opt/gcc/lib/gcc/current/libgfortran.5.dylib"
            )]
        );
    }

    /// The self-referential entry can be spelled differently from the
    /// queried path — a symlink alias (Homebrew's `opt/gcc/...` vs
    /// `Cellar/gcc/16.1.0/...` for the same real file) — and must still be
    /// recognized as self by filename, not missed by an exact string
    /// comparison.
    #[test]
    fn parse_skips_self_entry_even_under_a_different_symlink_alias() {
        let output = "/opt/homebrew/Cellar/gcc/16.1.0/lib/gcc/current/libgcc_s.1.1.dylib:\n\
             \t/opt/homebrew/opt/gcc/lib/gcc/current/libgcc_s.1.1.dylib (compatibility version 1.0.0, current version 1.1.0)\n\
             \t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n";

        let libs = parse_otool_libs(output);

        assert!(
            libs.is_empty(),
            "the differently-aliased self entry must not be treated as a real dependency: {libs:?}"
        );
    }

    /// Feeding already-rewritten `otool -L` output (a system entry plus an
    /// `@executable_path/../Frameworks/...` entry, no absolute Homebrew
    /// path left) through parse+plan must produce an empty plan — proving
    /// a second `bundle_closure` pass over already-fixed refs would not
    /// attempt to rewrite them again.
    #[test]
    fn closure_rewrite_plan_is_idempotent() {
        let output = "target/release/humm-gui:\n\
             \t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n\
             \t@executable_path/../Frameworks/libopenblas.0.dylib (compatibility version 0.0.0, current version 0.0.0)\n";

        let libs = parse_otool_libs(output);
        let plan = plan_rewrites(&libs, RefKind::MainBinary);

        assert!(
            plan.is_empty(),
            "already-rewritten refs must not be re-planned: {plan:?}"
        );
    }

    #[test]
    fn plan_rewrites_targets_executable_path_for_main_binary() {
        let libs = vec![PathBuf::from(
            "/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib",
        )];
        let plan = plan_rewrites(&libs, RefKind::MainBinary);
        assert_eq!(
            plan,
            vec![RewriteOp {
                old: PathBuf::from("/opt/homebrew/opt/openblas/lib/libopenblas.0.dylib"),
                new: "@executable_path/../Frameworks/libopenblas.0.dylib".to_string(),
            }]
        );
    }

    #[test]
    fn plan_rewrites_targets_loader_path_between_dylibs() {
        let libs = vec![PathBuf::from(
            "/opt/homebrew/opt/gcc/lib/gcc/current/libgfortran.5.dylib",
        )];
        let plan = plan_rewrites(&libs, RefKind::Dylib);
        assert_eq!(
            plan,
            vec![RewriteOp {
                old: PathBuf::from("/opt/homebrew/opt/gcc/lib/gcc/current/libgfortran.5.dylib"),
                new: "@loader_path/libgfortran.5.dylib".to_string(),
            }]
        );
    }

    /// The real-world case this module exists to catch: `libgfortran`
    /// references `libquadmath`/`libgcc_s` only via already-relative
    /// `@rpath` entries — [`parse_otool_libs`] correctly finds nothing to
    /// rewrite here, but [`parse_relative_dep_names`] must still surface
    /// both sibling names so `bundle_closure` can go find and copy them.
    #[test]
    fn parse_relative_dep_names_finds_rpath_siblings_parse_otool_libs_ignores() {
        let output = "/opt/homebrew/opt/gcc/lib/gcc/current/libgfortran.5.dylib:\n\
             \t@rpath/libgfortran.5.dylib (compatibility version 6.0.0, current version 6.0.0)\n\
             \t@rpath/libquadmath.0.dylib (compatibility version 1.0.0, current version 1.0.0)\n\
             \t@rpath/libgcc_s.1.1.dylib (compatibility version 1.0.0, current version 1.1.0)\n\
             \t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n";

        assert!(parse_otool_libs(output).is_empty());
        assert_eq!(
            parse_relative_dep_names(output),
            vec![
                "libquadmath.0.dylib".to_string(),
                "libgcc_s.1.1.dylib".to_string()
            ]
        );
    }

    /// Canned `otool -l` text for a file with one relative and two
    /// absolute (Homebrew) `LC_RPATH` entries — the actual
    /// `libopenblas.0.dylib` finding. Only the absolute ones are
    /// [`is_external_absolute`].
    #[test]
    fn parse_otool_rpaths_reads_every_lc_rpath_path_value() {
        let output = "Load command 10\n\
             \x20         cmd LC_RPATH\n\
             \x20     cmdsize 32\n\
             \x20        path @loader_path (offset 12)\n\
             Load command 11\n\
             \x20         cmd LC_RPATH\n\
             \x20     cmdsize 56\n\
             \x20        path /opt/homebrew/opt/gcc/lib/gcc/current (offset 12)\n\
             Load command 12\n\
             \x20         cmd LC_LOAD_DYLIB\n\
             \x20     cmdsize 56\n\
             \x20        name /usr/lib/libSystem.B.dylib (offset 24)\n";

        let rpaths = parse_otool_rpaths(output);

        assert_eq!(
            rpaths,
            vec![
                "@loader_path".to_string(),
                "/opt/homebrew/opt/gcc/lib/gcc/current".to_string(),
            ]
        );
        assert!(!is_external_absolute(&rpaths[0]));
        assert!(is_external_absolute(&rpaths[1]));
    }

    // --- task-11 review fixes: dedupe-by-filename collision detection,
    // dangling relative-ref verification, and signing order ---------------

    /// The common, legitimate case [`claim_destination`] must keep silent:
    /// two different source paths, same basename, byte-identical content
    /// (e.g. two binaries depending on the exact same real dylib via
    /// different symlink aliases) — the second claim is a no-op skip, not
    /// an error.
    #[test]
    fn duplicate_name_same_content_is_skipped() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src_a = dir.path().join("a/libfoo.dylib");
        let src_b = dir.path().join("b/libfoo.dylib");
        fs::create_dir_all(src_a.parent().unwrap()).unwrap();
        fs::create_dir_all(src_b.parent().unwrap()).unwrap();
        fs::write(&src_a, b"identical-bytes").unwrap();
        fs::write(&src_b, b"identical-bytes").unwrap();

        let mut queued: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
        let dest = dir.path().join("Frameworks/libfoo.dylib");

        assert!(
            claim_destination(&mut queued, &dest, &src_a).expect("first claim succeeds"),
            "first claim of an unclaimed destination must report true (caller must copy)"
        );
        assert!(
            !claim_destination(&mut queued, &dest, &src_b).expect("second claim succeeds"),
            "a byte-identical same-basename source must be silently skipped"
        );
    }

    /// The bug this finding is about: two *different* files sharing a
    /// basename must not be silently merged — [`claim_destination`] must
    /// hard-error, naming both source paths and the colliding destination,
    /// rather than picking one and discarding the other.
    #[test]
    fn duplicate_name_different_content_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let src_a = dir.path().join("a/libfoo.dylib");
        let src_b = dir.path().join("b/libfoo.dylib");
        fs::create_dir_all(src_a.parent().unwrap()).unwrap();
        fs::create_dir_all(src_b.parent().unwrap()).unwrap();
        fs::write(&src_a, b"version-one-content").unwrap();
        fs::write(&src_b, b"version-two-content-differs").unwrap();

        let mut queued: BTreeMap<PathBuf, PathBuf> = BTreeMap::new();
        let dest = dir.path().join("Frameworks/libfoo.dylib");

        assert!(claim_destination(&mut queued, &dest, &src_a).expect("first claim succeeds"));
        let err = claim_destination(&mut queued, &dest, &src_b)
            .expect_err("different-content same-basename collision must error");
        let msg = format!("{err:#}");
        assert!(
            msg.contains(&src_a.display().to_string()),
            "error must name the first source: {msg}"
        );
        assert!(
            msg.contains(&src_b.display().to_string()),
            "error must name the second source: {msg}"
        );
        assert!(
            msg.contains("libfoo.dylib"),
            "error must name the colliding basename/destination: {msg}"
        );
    }

    /// [`parse_relative_ref_targets`] + [`check_relative_refs_resolve`]:
    /// a `@rpath/<name>` entry whose file is present in `Frameworks/` is not
    /// flagged; a `@loader_path/<name>` entry whose file is absent next to
    /// the referencing dylib *is* flagged — canned `otool -L` text plus a
    /// tempdir standing in for the bundle, no real Mach-O or `otool`
    /// involved (the resolution logic, not the exec wrapper, is under
    /// test).
    #[test]
    fn verify_flags_dangling_relative_ref() {
        let output = "libgfortran.5.dylib:\n\
             \t@rpath/libgfortran.5.dylib (compatibility version 6.0.0, current version 6.0.0)\n\
             \t@rpath/libquadmath.0.dylib (compatibility version 1.0.0, current version 1.0.0)\n\
             \t@loader_path/libgcc_s.1.1.dylib (compatibility version 1.0.0, current version 1.1.0)\n\
             \t/usr/lib/libSystem.B.dylib (compatibility version 1.0.0, current version 1.0.0)\n";

        let targets = parse_relative_ref_targets(output);
        assert_eq!(
            targets,
            vec![
                (RelPrefix::Rpath, "libquadmath.0.dylib".to_string()),
                (RelPrefix::LoaderPath, "libgcc_s.1.1.dylib".to_string()),
            ],
            "self entry must be excluded, system entry must be excluded"
        );

        let dir = tempfile::tempdir().expect("tempdir");
        let frameworks = dir.path().join("Contents/Frameworks");
        fs::create_dir_all(&frameworks).unwrap();
        // The @rpath target exists in Frameworks/; the @loader_path target
        // (expected next to the referencing file itself) does not.
        fs::write(frameworks.join("libquadmath.0.dylib"), b"stub").unwrap();
        let file = frameworks.join("libgfortran.5.dylib");
        fs::write(&file, b"stub").unwrap();

        let offenders = check_relative_refs_resolve(&file, &targets, &frameworks);

        assert_eq!(
            offenders.len(),
            1,
            "only the missing libgcc_s.1.1.dylib @loader_path target should be flagged: {offenders:?}"
        );
        assert!(offenders[0].contains("libgcc_s.1.1.dylib"));
        assert!(offenders[0].contains("@loader_path"));
    }

    /// [`sign_plan`]: pins the signing order (spec §1.3 item 4, task-11
    /// review finding) — every `Contents/Frameworks/` file, then every
    /// `Contents/MacOS/` file (this is the fix: the secondary CLI binary
    /// must appear here, not be skipped), then the `.app` bundle itself,
    /// last. Dummy files stand in for real Mach-Os; no `codesign` involved.
    #[test]
    fn sign_plan_orders_frameworks_then_macos_then_bundle() {
        let staging = tempfile::tempdir().expect("tempdir");
        let app = staging.path().join("humm.app");
        let frameworks = app.join("Contents/Frameworks");
        let macos = app.join("Contents/MacOS");
        fs::create_dir_all(&frameworks).unwrap();
        fs::create_dir_all(&macos).unwrap();

        fs::write(frameworks.join("libopenblas.0.dylib"), b"stub").unwrap();
        fs::write(macos.join("humm-gui"), b"stub").unwrap();
        fs::write(macos.join("humm"), b"stub").unwrap();

        let plan = sign_plan(&app).expect("sign_plan succeeds");

        assert_eq!(
            plan,
            vec![
                frameworks.join("libopenblas.0.dylib"),
                macos.join("humm"),
                macos.join("humm-gui"),
                app.clone(),
            ],
            "Frameworks/* must precede MacOS/* (sorted), the .app bundle must be signed last, \
             and both MacOS binaries — including the secondary CLI — must appear individually"
        );
    }
}
