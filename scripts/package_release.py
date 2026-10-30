#!/usr/bin/env python3
"""Stage a humm release directory with bundled load-time shared libraries.

Copies the named release binaries plus OpenBLAS / ONNX Runtime (and the
transitive non-system closure those need on macOS/Linux) into ``--stage``,
rewrites macOS install names and Linux RPATH so the tree is self-contained,
and fail-closes if ``otool -L`` / ``ldd`` still show Homebrew absolute paths
or unresolved libraries.

Windows places DLLs next to the exe (the loader search path). This script is
build-time packaging only — it uses OS tools already on the runners
(``otool`` / ``install_name_tool`` / ``codesign``, ``ldd`` / ``patchelf``,
``dumpbin`` when present). It does not add Cargo dependencies.
"""

from __future__ import annotations

import argparse
import os
import shutil
import subprocess
import sys
from pathlib import Path


SYSTEM_LIB_PREFIXES_UNIX = (
    "/lib/",
    "/lib64/",
    "/usr/lib/",
    "/usr/lib64/",
    "/System/",
    "/usr/lib/system/",
    "/Library/Apple/",
)

LINUX_SYSTEM_SONAMES = {
    "linux-vdso.so.1",
    "ld-linux-x86-64.so.2",
    "ld-linux-aarch64.so.1",
    "ld-linux.so.2",
    "libc.so.6",
    "libm.so.6",
    "libdl.so.2",
    "librt.so.1",
    "libpthread.so.0",
    "libresolv.so.2",
    "libutil.so.1",
    "libgcc_s.so.1",
    "libstdc++.so.6",
    "libnss_files.so.2",
    "libnss_dns.so.2",
}


def run(cmd: list[str], **kwargs) -> subprocess.CompletedProcess[str]:
    return subprocess.run(cmd, check=True, text=True, capture_output=True, **kwargs)


def which(name: str) -> str | None:
    return shutil.which(name)


def copy_file(src: Path, dst: Path) -> None:
    dst.parent.mkdir(parents=True, exist_ok=True)
    if dst.exists() or dst.is_symlink():
        dst.unlink()
    shutil.copy2(src, dst, follow_symlinks=True)


def resolve_binary(bin_dir: Path, name: str, exe_suffix: str) -> Path:
    path = bin_dir / f"{name}{exe_suffix}"
    if not path.is_file():
        raise RuntimeError(f"missing release binary: {path}")
    return path


def find_ort_libs(bin_dir: Path) -> list[Path]:
    """Locate ONNX Runtime shared libs staged by ort-sys copy-dylibs."""
    patterns = (
        "onnxruntime.dll",
        "onnxruntime_*.dll",
        "libonnxruntime.so",
        "libonnxruntime.so.*",
        "libonnxruntime.dylib",
        "libonnxruntime.*.dylib",
    )
    found: list[Path] = []
    for pattern in patterns:
        found.extend(p for p in bin_dir.glob(pattern) if p.is_file() or p.is_symlink())
    # Prefer real files; resolve symlinks to the cache so the archive is self-contained.
    resolved: list[Path] = []
    seen: set[str] = set()
    for path in found:
        real = path.resolve()
        key = real.name
        if key in seen:
            continue
        seen.add(key)
        resolved.append(real)
    return resolved


def find_windows_openblas_dlls(vcpkg_prefix: Path | None) -> list[Path]:
    if vcpkg_prefix is None:
        raise RuntimeError("Windows packaging requires --vcpkg-prefix")
    bin_dir = vcpkg_prefix / "bin"
    if not bin_dir.is_dir():
        raise RuntimeError(f"vcpkg bin directory missing: {bin_dir}")
    # Dynamic triplets ship openblas.dll (and often libopenblas.dll aliases)
    # plus MinGW runtime companions. Copy every DLL from that bin dir that
    # openblas or its Fortran/OpenMP stack commonly needs — fail if none match.
    wanted_prefixes = (
        "openblas",
        "libopenblas",
        "libgfortran",
        "libquadmath",
        "libgcc",
        "libgomp",
        "libwinpthread",
        "libstdc++",
    )
    selected: list[Path] = []
    for path in sorted(bin_dir.glob("*.dll")):
        lower = path.name.lower()
        if any(lower.startswith(prefix) for prefix in wanted_prefixes):
            selected.append(path)
    if not any(p.name.lower().startswith(("openblas", "libopenblas")) for p in selected):
        raise RuntimeError(
            f"no openblas*.dll under {bin_dir}; install openblas via vcpkg first"
        )
    return selected


def parse_otool_libs(output: str) -> list[str]:
    libs: list[str] = []
    for line in output.splitlines()[1:]:
        text = line.strip()
        if not text:
            continue
        # "path (compatibility version …)"
        path = text.split(" (", 1)[0].strip()
        if path:
            libs.append(path)
    return libs


def is_system_mac_lib(path: str) -> bool:
    if path.startswith("@") or path.startswith("/usr/lib/") or path.startswith("/System/"):
        return True
    return False


def collect_macos_closure(roots: list[Path]) -> dict[str, Path]:
    """Map basename -> absolute source path for non-system Mach-O deps."""
    queue = list(roots)
    seen_files: set[Path] = set()
    closure: dict[str, Path] = {}
    otool = "/usr/bin/otool"
    while queue:
        current = queue.pop(0)
        current = current.resolve()
        if current in seen_files:
            continue
        seen_files.add(current)
        result = run([otool, "-L", str(current)])
        for dep in parse_otool_libs(result.stdout):
            if is_system_mac_lib(dep):
                continue
            dep_path = Path(dep)
            if not dep_path.is_file():
                # Relative / @loader_path refs are handled after rewrite verify.
                continue
            name = dep_path.name
            if name not in closure:
                closure[name] = dep_path.resolve()
                queue.append(dep_path)
    return closure


def rewrite_macos_stage(stage: Path, binaries: list[Path], dylibs: list[Path]) -> None:
    install_name_tool = "/usr/bin/install_name_tool"
    otool = "/usr/bin/otool"
    codesign = "/usr/bin/codesign"

    staged_names = {p.name for p in binaries + dylibs}

    for dylib in dylibs:
        run([install_name_tool, "-id", f"@loader_path/{dylib.name}", str(dylib)])

    for mach_o in binaries + dylibs:
        result = run([otool, "-L", str(mach_o)])
        for dep in parse_otool_libs(result.stdout):
            base = Path(dep).name
            if base == mach_o.name:
                continue
            if base in staged_names:
                new = f"@loader_path/{base}"
                if dep != new:
                    run([install_name_tool, "-change", dep, new, str(mach_o)])

        # Drop absolute LC_RPATH entries that would reintroduce Homebrew.
        rpaths = run([otool, "-l", str(mach_o)]).stdout
        current_rpath = None
        for line in rpaths.splitlines():
            stripped = line.strip()
            if stripped.startswith("path "):
                # "path /opt/homebrew/... (offset N)"
                current_rpath = stripped[5:].split(" (", 1)[0].strip()
                if current_rpath.startswith("/") and not current_rpath.startswith(
                    "/usr/lib"
                ):
                    try:
                        run([install_name_tool, "-delete_rpath", current_rpath, str(mach_o)])
                    except subprocess.CalledProcessError:
                        # Already removed or not deletable; verify step catches leftovers.
                        pass

        run([codesign, "-s", "-", "-f", str(mach_o)])


def verify_macos(stage: Path, binaries: list[str]) -> None:
    otool = "/usr/bin/otool"
    for name in binaries:
        path = stage / name
        result = run([otool, "-L", str(path)])
        for dep in parse_otool_libs(result.stdout):
            if dep.startswith("/opt/homebrew") or dep.startswith("/usr/local/opt"):
                raise RuntimeError(
                    f"{name} still references Homebrew absolute path: {dep}"
                )
            if dep.startswith("/") and not is_system_mac_lib(dep):
                # Allowed only if the basename exists beside the binary.
                if not (stage / Path(dep).name).is_file():
                    raise RuntimeError(f"{name} depends on unpackaged {dep}")


def collect_linux_closure(roots: list[Path]) -> dict[str, Path]:
    closure: dict[str, Path] = {}
    for root in roots:
        result = run(["ldd", str(root)])
        for line in result.stdout.splitlines():
            text = line.strip()
            if "=>" not in text:
                continue
            left, right = text.split("=>", 1)
            soname = left.strip()
            if soname in LINUX_SYSTEM_SONAMES:
                continue
            right = right.strip()
            if right == "not found":
                raise RuntimeError(f"ldd: {soname} not found for {root}")
            path_str = right.split(" (", 1)[0].strip()
            if not path_str or path_str.startswith("("):
                continue
            if any(path_str.startswith(prefix) for prefix in ("/lib/", "/lib64/", "/usr/lib")):
                # Keep OpenBLAS and onnxruntime even under /usr/lib; skip the rest.
                base = Path(path_str).name
                if not (
                    base.startswith("libopenblas")
                    or base.startswith("libonnxruntime")
                    or "openblas" in base
                    or "onnxruntime" in base
                    or base.startswith("libgfortran")
                    or base.startswith("libquadmath")
                    or base.startswith("libgomp")
                ):
                    continue
            path = Path(path_str)
            if path.is_file():
                closure[path.name] = path.resolve()
    return closure


def rewrite_linux_stage(binaries: list[Path], libs: list[Path]) -> None:
    patchelf = which("patchelf")
    if patchelf is None:
        raise RuntimeError("patchelf is required on Linux release runners")
    for path in binaries + libs:
        run([patchelf, "--set-rpath", "$ORIGIN", str(path)])


def verify_linux(stage: Path, binaries: list[str]) -> None:
    env = dict(os.environ)
    # Force the loader to use only the staged tree + system defaults.
    env.pop("LD_LIBRARY_PATH", None)
    if not any(stage.glob("libopenblas*")) and not any(stage.glob("*openblas*.so*")):
        raise RuntimeError("staged Linux tree is missing libopenblas*")
    if not any(stage.glob("libonnxruntime*")):
        raise RuntimeError("staged Linux tree is missing libonnxruntime*")
    stage_resolved = stage.resolve()
    for name in binaries:
        path = stage / name
        result = subprocess.run(
            ["ldd", str(path)], check=True, text=True, capture_output=True, env=env
        )
        for line in result.stdout.splitlines():
            if "not found" in line:
                raise RuntimeError(f"ldd unresolved for {name}: {line.strip()}")
            lower = line.lower()
            if "openblas" in lower or "onnxruntime" in lower:
                if "=>" not in line:
                    continue
                right = line.split("=>", 1)[1].strip().split(" (", 1)[0].strip()
                if not right or right.startswith("("):
                    continue
                resolved = Path(right).resolve()
                if stage_resolved not in resolved.parents and resolved.parent != stage_resolved:
                    raise RuntimeError(
                        f"{name} resolves {line.strip()} outside the stage ({stage})"
                    )


def verify_windows(stage: Path, binaries: list[str]) -> None:
    required = ["onnxruntime.dll"]
    openblas = list(stage.glob("openblas*.dll")) + list(stage.glob("libopenblas*.dll"))
    if not openblas:
        raise RuntimeError("staged Windows tree is missing openblas*.dll")
    for name in required:
        if not (stage / name).is_file():
            raise RuntimeError(f"staged Windows tree is missing {name}")
    dumpbin = which("dumpbin")
    if dumpbin:
        for binary in binaries:
            result = run([dumpbin, "/dependents", str(stage / binary)])
            text = result.stdout.lower()
            if "onnxruntime.dll" not in text:
                raise RuntimeError(
                    f"dumpbin: {binary} does not list onnxruntime.dll as a dependent"
                )


def smoke_help(stage: Path, binary: str) -> None:
    env = dict(os.environ)
    for key in (
        "LD_LIBRARY_PATH",
        "DYLD_LIBRARY_PATH",
        "DYLD_FALLBACK_LIBRARY_PATH",
        "DYLD_INSERT_LIBRARIES",
    ):
        env.pop(key, None)
    # Prefer the staged directory first so sibling DLLs win on Windows.
    env["PATH"] = str(stage) + os.pathsep + env.get("PATH", "")
    exe = stage / binary
    result = subprocess.run(
        [str(exe), "--help"],
        text=True,
        capture_output=True,
        env=env,
        cwd=str(stage),
    )
    combined = ((result.stdout or "") + (result.stderr or "")).strip()
    if result.returncode not in (0, 1, 2) or not combined:
        raise RuntimeError(
            f"smoke {binary} --help exited {result.returncode}: {combined[-2000:]}"
        )


def write_runtime_txt(stage: Path, include_gui: bool) -> None:
    lines = [
        "humm release archive",
        "",
        "This archive bundles load-time OpenBLAS and ONNX Runtime libraries",
        "next to the binaries. Keep those shared libraries beside humm when",
        "you move or install it — launching without them fails before main.",
        "",
        "Still required on PATH (not bundled):",
        "- ffmpeg and yt-dlp for media decode and URL import",
        "",
        "Linux GUI builds also need ordinary desktop shared libraries",
        "(ALSA, and a portal/zenity for file dialogs) from the OS.",
        "",
        "macOS binaries are unsigned (no Developer ID / notarization).",
        "Gatekeeper may block first launch: right-click → Open after you trust",
        "the download, or clear the quarantine attribute.",
        "",
        "Windows may show SmartScreen for an unsigned exe.",
        "",
        "Verify the archive against SHA256SUMS on the GitHub Release",
        "(Linux: sha256sum -c SHA256SUMS; macOS: shasum -a 256 -c SHA256SUMS).",
    ]
    if not include_gui:
        lines.extend(
            [
                "",
                "This archive is CLI-only (no humm-gui).",
            ]
        )
    (stage / "RUNTIME.txt").write_text("\n".join(lines) + "\n", encoding="utf-8")


def package(
    stage: Path,
    bin_dir: Path,
    bin_names: list[str],
    exe_suffix: str,
    vcpkg_prefix: Path | None,
    license_path: Path,
) -> None:
    if stage.exists():
        shutil.rmtree(stage)
    stage.mkdir(parents=True)

    sources: list[Path] = []
    binaries: list[Path] = []
    for name in bin_names:
        src = resolve_binary(bin_dir, name, exe_suffix)
        sources.append(src)
        dst = stage / f"{name}{exe_suffix}"
        copy_file(src, dst)
        binaries.append(dst)

    ort_libs = find_ort_libs(bin_dir)
    if not ort_libs:
        raise RuntimeError(
            f"no ONNX Runtime shared library next to {bin_dir}; "
            "ort copy-dylibs should place libonnxruntime / onnxruntime.dll there"
        )

    bundled_libs: list[Path] = []
    if sys.platform == "darwin":
        for lib in ort_libs:
            dst = stage / lib.name
            copy_file(lib, dst)
            bundled_libs.append(dst)
        # Walk the pre-rewrite binaries in bin_dir (Homebrew absolute paths)
        # plus the staged ort dylib so the full OpenBLAS closure is collected.
        closure = collect_macos_closure(sources + bundled_libs)
        for name, src in closure.items():
            dst = stage / name
            if not dst.exists():
                copy_file(src, dst)
                bundled_libs.append(dst)
        rewrite_macos_stage(stage, binaries, bundled_libs)
        verify_macos(stage, [f"{n}{exe_suffix}" for n in bin_names])
    elif sys.platform.startswith("linux"):
        for lib in ort_libs:
            dst = stage / lib.name
            copy_file(lib, dst)
            bundled_libs.append(dst)
        # Collect from originals in bin_dir where ort copy-dylibs / system
        # OpenBLAS still resolve under ldd before RPATH rewrite.
        closure = collect_linux_closure(sources)
        if not any("openblas" in name for name in closure):
            raise RuntimeError(
                "ldd did not report an OpenBLAS dependency; cannot bundle a runnable archive"
            )
        for name, src in closure.items():
            dst = stage / name
            if not dst.exists():
                copy_file(src, dst)
                bundled_libs.append(dst)
        rewrite_linux_stage(binaries, bundled_libs)
        verify_linux(stage, [f"{n}{exe_suffix}" for n in bin_names])
    elif sys.platform.startswith("win"):
        for lib in ort_libs:
            dst = stage / lib.name
            copy_file(lib, dst)
            bundled_libs.append(dst)
        for dll in find_windows_openblas_dlls(vcpkg_prefix):
            dst = stage / dll.name
            copy_file(dll, dst)
            bundled_libs.append(dst)
        verify_windows(stage, [f"{n}{exe_suffix}" for n in bin_names])
    else:
        raise RuntimeError(f"unsupported packaging platform: {sys.platform}")

    copy_file(license_path, stage / "LICENSE")
    include_gui = any(name == "humm-gui" for name in bin_names)
    contents = ["gui=yes" if include_gui else "gui=no"]
    if include_gui:
        contents.append("humm-gui is included in this archive.")
    else:
        contents.append("This archive is CLI-only (humm-gui was not built for this target).")
    (stage / "CONTENTS.txt").write_text("\n".join(contents) + "\n", encoding="utf-8")
    write_runtime_txt(stage, include_gui=include_gui)

    smoke_help(stage, f"humm{exe_suffix}")
    print(f"Packaged {stage} with {len(binaries)} binary(ies) and {len(bundled_libs)} lib(s)")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--stage", type=Path, required=True)
    parser.add_argument("--bin-dir", type=Path, required=True)
    parser.add_argument(
        "--bin",
        dest="bins",
        action="append",
        required=True,
        help="Binary crate exe name without suffix (repeatable)",
    )
    parser.add_argument("--exe-suffix", default="")
    parser.add_argument("--vcpkg-prefix", type=Path, default=None)
    parser.add_argument(
        "--license",
        dest="license_path",
        type=Path,
        default=Path("LICENSE"),
    )
    args = parser.parse_args()
    try:
        package(
            stage=args.stage,
            bin_dir=args.bin_dir,
            bin_names=args.bins,
            exe_suffix=args.exe_suffix,
            vcpkg_prefix=args.vcpkg_prefix,
            license_path=args.license_path,
        )
    except Exception as error:  # noqa: BLE001 — CLI boundary
        print(f"package_release failed: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
