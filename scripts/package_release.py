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


def _is_ort_name(name: str) -> bool:
    lower = name.lower()
    return "onnxruntime" in lower


def _dedupe_libs(paths: list[Path]) -> list[Path]:
    resolved: list[Path] = []
    seen: set[str] = set()
    for path in paths:
        if not path.exists() and not path.is_symlink():
            continue
        real = path.resolve() if path.exists() or path.is_symlink() else path
        if not real.is_file():
            continue
        key = real.name
        if key in seen:
            continue
        seen.add(key)
        resolved.append(real)
    return resolved


def _ort_cache_roots() -> list[Path]:
    home = Path.home()
    return [
        home / ".cache" / "ort.pyke.io",
        home / "Library" / "Caches" / "ort.pyke.io",
        Path(os.environ.get("LOCALAPPDATA", "")) / "ort.pyke.io",
        Path(os.environ.get("ORT_DOWNLOAD_CACHE", "")),
    ]


def find_ort_libs_in_tree(bin_dir: Path) -> list[Path]:
    """Search cargo target dirs and ort download caches for ONNX Runtime libs."""
    patterns = (
        "onnxruntime.dll",
        "onnxruntime_*.dll",
        "libonnxruntime.so",
        "libonnxruntime.so.*",
        "libonnxruntime.dylib",
        "libonnxruntime.*.dylib",
    )
    search_roots = [
        bin_dir,
        bin_dir / "deps",
        bin_dir.parent,  # e.g. target/
    ]
    # Also scan build script OUT_DIR parents under target/<profile>/build/ort-sys-*
    build_root = bin_dir / "build"
    if build_root.is_dir():
        search_roots.extend(sorted(build_root.glob("ort-sys-*"))[:8])
    cache_roots = [root for root in _ort_cache_roots() if str(root) and root.is_dir()]

    found: list[Path] = []
    for root in search_roots:
        if not root.exists():
            continue
        for pattern in patterns:
            # Non-recursive first (fast), then shallow recursive for build outs.
            found.extend(root.glob(pattern))
            found.extend(root.glob(f"*/{pattern}"))
            found.extend(root.glob(f"*/*/{pattern}"))
    # ort.pyke.io caches nest the shared library a few directories deep.
    for root in cache_roots:
        for path in root.rglob("*onnxruntime*"):
            name = path.name.lower()
            if not path.is_file() and not path.is_symlink():
                continue
            if name.endswith(".dll") or name.endswith(".dylib") or ".so" in name:
                found.append(path)
    return _dedupe_libs(found)


def find_ort_libs_via_loader(binaries: list[Path]) -> list[Path]:
    """Ask the dynamic loader which onnxruntime image each binary needs."""
    found: list[Path] = []
    for binary in binaries:
        if sys.platform == "darwin":
            result = run(["/usr/bin/otool", "-L", str(binary)])
            for dep in parse_otool_libs(result.stdout):
                if _is_ort_name(Path(dep).name) and Path(dep).is_file():
                    found.append(Path(dep))
        elif sys.platform.startswith("linux"):
            result = subprocess.run(
                ["ldd", str(binary)], check=False, text=True, capture_output=True
            )
            for line in result.stdout.splitlines():
                if "onnxruntime" not in line.lower():
                    continue
                if "=>" not in line:
                    continue
                right = line.split("=>", 1)[1].strip()
                if right == "not found" or right.startswith("("):
                    continue
                path_str = right.split(" (", 1)[0].strip()
                path = Path(path_str)
                if path.is_file():
                    found.append(path)
        elif sys.platform.startswith("win"):
            # dumpbin lists dependents by basename; search PATH + bin_dir later.
            dumpbin = which("dumpbin")
            if dumpbin is None:
                continue
            result = run([dumpbin, "/dependents", str(binary)])
            for line in result.stdout.splitlines():
                name = line.strip()
                if _is_ort_name(name) and name.lower().endswith(".dll"):
                    # Resolve beside the binary first, then PATH.
                    candidate = binary.parent / name
                    if candidate.is_file():
                        found.append(candidate)
                        continue
                    located = which(name)
                    if located:
                        found.append(Path(located))
    return _dedupe_libs(found)


def pe_import_dll_names(binary: Path) -> list[str]:
    """Return PE import DLL basenames without dumpbin (Git Bash packaging)."""
    import struct

    data = binary.read_bytes()
    if len(data) < 0x40 or data[:2] != b"MZ":
        return []
    e_lfanew = struct.unpack_from("<I", data, 0x3C)[0]
    if e_lfanew + 24 >= len(data) or data[e_lfanew : e_lfanew + 4] != b"PE\0\0":
        return []
    opt_off = e_lfanew + 24
    magic = struct.unpack_from("<H", data, opt_off)[0]
    # DataDirectory[1] = import table RVA. PE32 optional header is 96 bytes
    # before directories; PE32+ is 112.
    if magic == 0x10B:
        dir_off = opt_off + 96
    elif magic == 0x20B:
        dir_off = opt_off + 112
    else:
        return []
    if dir_off + 16 > len(data):
        return []
    import_rva = struct.unpack_from("<I", data, dir_off + 8)[0]
    if import_rva == 0:
        return []

    # Map RVA → file offset via section headers.
    num_sections = struct.unpack_from("<H", data, e_lfanew + 6)[0]
    size_opt = struct.unpack_from("<H", data, e_lfanew + 20)[0]
    section_off = e_lfanew + 24 + size_opt

    def rva_to_offset(rva: int) -> int | None:
        for i in range(num_sections):
            off = section_off + i * 40
            if off + 40 > len(data):
                return None
            virt_size, virt_addr, raw_size, raw_ptr = struct.unpack_from(
                "<IIII", data, off + 8
            )
            size = max(virt_size, raw_size)
            if virt_addr <= rva < virt_addr + size:
                return raw_ptr + (rva - virt_addr)
        return None

    import_off = rva_to_offset(import_rva)
    if import_off is None:
        return []

    names: list[str] = []
    entry = import_off
    # IMAGE_IMPORT_DESCRIPTOR is 20 bytes; null terminator ends the list.
    while entry + 20 <= len(data):
        lookup, _ts, _chain, name_rva, _iat = struct.unpack_from("<IIIII", data, entry)
        if lookup == 0 and name_rva == 0:
            break
        name_off = rva_to_offset(name_rva)
        if name_off is not None and name_off < len(data):
            end = data.find(b"\0", name_off)
            if end == -1:
                end = min(name_off + 256, len(data))
            raw = data[name_off:end]
            try:
                names.append(raw.decode("ascii"))
            except UnicodeDecodeError:
                pass
        entry += 20
    return names


def binary_needs_ort(binaries: list[Path]) -> bool:
    """True when any binary's dynamic deps mention onnxruntime."""
    for binary in binaries:
        if sys.platform == "darwin":
            result = run(["/usr/bin/otool", "-L", str(binary)])
            if any(_is_ort_name(Path(dep).name) for dep in parse_otool_libs(result.stdout)):
                return True
        elif sys.platform.startswith("linux"):
            result = subprocess.run(
                ["ldd", str(binary)], check=False, text=True, capture_output=True
            )
            if "onnxruntime" in result.stdout.lower():
                return True
            # Also check DT_NEEDED via readelf when ldd says not found.
            readelf = which("readelf")
            if readelf:
                needed = subprocess.run(
                    [readelf, "-d", str(binary)],
                    check=False,
                    text=True,
                    capture_output=True,
                )
                if "onnxruntime" in needed.stdout.lower():
                    return True
        elif sys.platform.startswith("win"):
            dumpbin = which("dumpbin")
            if dumpbin:
                result = run([dumpbin, "/dependents", str(binary)])
                if "onnxruntime" in result.stdout.lower():
                    return True
                continue
            # Git Bash release packaging often has no dumpbin on PATH.
            # ort 2.x on Windows ships a static onnxruntime.lib plus an EP
            # DLL (DirectML.dll) next to the exe — that is NOT a load-time
            # onnxruntime.dll dependency. Only require the DLL when the PE
            # actually imports it, or when ort copy-dylibs left it beside
            # the binary.
            if (binary.parent / "onnxruntime.dll").is_file():
                return True
            if any(_is_ort_name(name) for name in pe_import_dll_names(binary)):
                return True
    return False


def find_windows_ort_ep_dlls(bin_dir: Path) -> list[Path]:
    """Copy ort execution-provider DLLs that sit next to the release exe.

    Static Windows ort still drops DirectML.dll via copy-dylibs; the loader
    needs it at runtime even when onnxruntime.dll is absent.
    """
    wanted = ("DirectML.dll", "DirectML.Debug.dll")
    found: list[Path] = []
    for name in wanted:
        path = bin_dir / name
        if path.is_file():
            found.append(path)
    return found


def find_ort_libs(bin_dir: Path, binaries: list[Path]) -> list[Path]:
    """Locate ONNX Runtime shared libs for packaging.

    Prefer the loader's resolved path (ldd/otool), then ort-sys copy-dylibs
    outputs under the cargo target tree, then the ort.pyke.io download cache.
    """
    found = find_ort_libs_via_loader(binaries)
    if found:
        return found
    return find_ort_libs_in_tree(bin_dir)


def find_windows_openblas_dlls(vcpkg_prefix: Path | None) -> list[Path]:
    if vcpkg_prefix is None:
        raise RuntimeError("Windows packaging requires --vcpkg-prefix")
    bin_dir = vcpkg_prefix / "bin"
    if not bin_dir.is_dir():
        raise RuntimeError(f"vcpkg bin directory missing: {bin_dir}")
    # Dynamic triplets ship openblas.dll (and often libopenblas.dll aliases)
    # plus MinGW runtime companions. Stock vcpkg OpenBLAS is BLAS-only
    # (BUILD_WITHOUT_LAPACK=ON); LAPACK comes from *static* vcpkg clapack
    # (lapack.lib + libf2c.lib linked at build time — no clapack DLLs).
    # Copy only DLLs that actually exist under bin/.
    wanted_prefixes = (
        "openblas",
        "libopenblas",
        "lapack",  # only if a future dynamic LAPACK appears
        "libf2c",
        "f2c",
        "clapack",
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
    # Bare @rpath/@loader_path/@executable_path refs are NOT system — they are
    # often Homebrew gcc runtimes (libgcc_s, libquadmath) that must be bundled.
    if path.startswith("/usr/lib/") or path.startswith("/System/"):
        return True
    if path.startswith("@") and not path.startswith(
        ("@rpath/", "@loader_path/", "@executable_path/")
    ):
        return True
    return False


def macos_lc_rpaths(mach_o: Path) -> list[Path]:
    """Absolute LC_RPATH directories recorded on a Mach-O (skip @-relative)."""
    otool = "/usr/bin/otool"
    result = run([otool, "-l", str(mach_o)])
    rpaths: list[Path] = []
    lines = result.stdout.splitlines()
    for index, line in enumerate(lines):
        if line.strip() != "cmd LC_RPATH":
            continue
        for follow in lines[index + 1 : index + 8]:
            stripped = follow.strip()
            if stripped.startswith("path "):
                raw = stripped[5:].split(" (", 1)[0].strip()
                if raw.startswith("/") and Path(raw).is_dir():
                    rpaths.append(Path(raw))
                break
    return rpaths


def resolve_macos_dep(dep: str, current: Path) -> Path | None:
    """Resolve an otool -L dependency to a real file, including @rpath."""
    if is_system_mac_lib(dep):
        return None
    if not dep.startswith("@"):
        path = Path(dep)
        return path.resolve() if path.is_file() else None

    # @rpath/libfoo.dylib, @loader_path/../lib/libfoo.dylib, etc.
    if dep.startswith("@loader_path/"):
        candidate = (current.parent / dep[len("@loader_path/") :]).resolve()
        return candidate if candidate.is_file() else None
    if dep.startswith("@executable_path/"):
        candidate = (current.parent / dep[len("@executable_path/") :]).resolve()
        return candidate if candidate.is_file() else None
    if not dep.startswith("@rpath/"):
        return None

    name = Path(dep).name
    search_dirs = [current.parent, *macos_lc_rpaths(current)]
    for directory in search_dirs:
        candidate = directory / name
        if candidate.is_file():
            return candidate.resolve()
        # Homebrew gcc often nests libgcc_s one directory deeper than gfortran.
        nested = list(directory.glob(f"*/{name}")) + list(directory.glob(f"*/*/{name}"))
        for hit in nested:
            if hit.is_file():
                return hit.resolve()
    return None


def macos_stage_basename(dep: str, resolved: Path) -> str:
    """Basename used when copying a Mach-O dep into the stage.

    Prefer the ``otool -L`` install-name basename so ``rewrite_macos_stage``
    can ``install_name_tool -change`` against staged files. ``Path.resolve()``
    may collapse Homebrew versioned symlinks (``libopenblas.0.dylib`` →
    ``libopenblas.dylib``); staging under the resolved name leaves the
    absolute Homebrew install name on the binary and fails verify.
    """
    install_base = Path(dep).name
    if install_base and install_base not in (".", "/"):
        return install_base
    return resolved.name


def collect_macos_closure(roots: list[Path]) -> dict[str, Path]:
    """Map install-name basename -> absolute source path for non-system deps."""
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
            dep_path = resolve_macos_dep(dep, current)
            if dep_path is None:
                continue
            name = macos_stage_basename(dep, dep_path)
            if name not in closure:
                closure[name] = dep_path
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
    staged = {p.name for p in stage.iterdir() if p.is_file()}
    # Walk binaries and every staged dylib so @rpath leftovers on
    # libgfortran → libgcc_s are caught before smoke.
    targets = [stage / name for name in binaries]
    targets.extend(
        sorted(p for p in stage.iterdir() if p.suffix == ".dylib" and p.is_file())
    )
    for path in targets:
        result = run([otool, "-L", str(path)])
        for dep in parse_otool_libs(result.stdout):
            if dep.startswith("/opt/homebrew") or dep.startswith("/usr/local/opt"):
                raise RuntimeError(
                    f"{path.name} still references Homebrew absolute path: {dep}"
                )
            if dep.startswith("@rpath/") or dep.startswith("@loader_path/"):
                base = Path(dep).name
                if base != path.name and base not in staged:
                    raise RuntimeError(
                        f"{path.name} depends on unpackaged {dep} (missing {base})"
                    )
            if dep.startswith("/") and not is_system_mac_lib(dep):
                # Allowed only if the basename exists beside the binary.
                if not (stage / Path(dep).name).is_file():
                    raise RuntimeError(f"{path.name} depends on unpackaged {dep}")


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


def verify_linux(stage: Path, binaries: list[str], require_ort: bool = True) -> None:
    env = dict(os.environ)
    # Force the loader to use only the staged tree + system defaults.
    env.pop("LD_LIBRARY_PATH", None)
    if not any(stage.glob("libopenblas*")) and not any(stage.glob("*openblas*.so*")):
        raise RuntimeError("staged Linux tree is missing libopenblas*")
    if require_ort and not any(stage.glob("libonnxruntime*")):
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


def verify_windows(stage: Path, binaries: list[str], require_ort: bool = True) -> None:
    openblas = list(stage.glob("openblas*.dll")) + list(stage.glob("libopenblas*.dll"))
    if not openblas:
        raise RuntimeError("staged Windows tree is missing openblas*.dll")
    # clapack is static-only in vcpkg — do not require lapack.dll / libf2c.dll.
    if require_ort and not (stage / "onnxruntime.dll").is_file():
        raise RuntimeError("staged Windows tree is missing onnxruntime.dll")
    dumpbin = which("dumpbin")
    if dumpbin and require_ort:
        for binary in binaries:
            result = run([dumpbin, "/dependents", str(stage / binary)])
            text = result.stdout.lower()
            if "onnxruntime.dll" not in text:
                raise RuntimeError(
                    f"dumpbin: {binary} does not list onnxruntime.dll as a dependent"
                )


def smoke_help(stage: Path, binary: str) -> None:
    # Absolute paths: a relative stage/binary with cwd=stage makes the kernel
    # look for stage/stage/binary (FileNotFoundError on the dry-run runners).
    stage = stage.resolve()
    exe = stage / binary
    if not exe.is_file():
        listing = sorted(p.name for p in stage.iterdir()) if stage.is_dir() else []
        raise RuntimeError(
            f"smoke: staged binary missing: {exe} (stage contents: {listing})"
        )
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


def write_runtime_txt(stage: Path, include_gui: bool, bundled_ort: bool) -> None:
    lines = [
        "humm release archive",
        "",
        "This archive bundles load-time OpenBLAS next to the binaries (and",
        "ONNX Runtime when the build linked it as a shared library). Keep",
        "those shared libraries beside humm when you move or install it —",
        "launching without them fails before main.",
        "",
    ]
    if bundled_ort:
        lines.append("ONNX Runtime shared libraries are included in this archive.")
    else:
        lines.extend(
            [
                "This build linked ONNX Runtime statically (or does not need a",
                "separate onnxruntime shared library at load time).",
            ]
        )
    lines.extend(
        [
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
    )
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
    # Resolve early so smoke/verify never mix a relative stage with cwd=stage.
    stage = stage.resolve()
    bin_dir = bin_dir.resolve()
    license_path = license_path.resolve()
    if vcpkg_prefix is not None:
        vcpkg_prefix = vcpkg_prefix.resolve()

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
        if not dst.is_file():
            raise RuntimeError(f"failed to stage binary: {dst}")
        binaries.append(dst)

    needs_ort = binary_needs_ort(sources)
    ort_libs = find_ort_libs(bin_dir, sources)
    if needs_ort and not ort_libs:
        listing = sorted(p.name for p in bin_dir.iterdir())[:40] if bin_dir.is_dir() else []
        raise RuntimeError(
            "binaries dynamically depend on ONNX Runtime, but no libonnxruntime / "
            f"onnxruntime.dll was found via ldd/otool, {bin_dir}, deps/, or the "
            f"ort.pyke.io cache. target/release sample: {listing}"
        )
    if not needs_ort:
        print(
            "note: no onnxruntime DT_NEEDED on packaged binaries; "
            "skipping ONNX Runtime bundle",
            file=sys.stderr,
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
        # If ldd resolved onnxruntime after we staged a copy, ensure it is present.
        if needs_ort and not any("onnxruntime" in p.name for p in bundled_libs):
            raise RuntimeError(
                "ONNX Runtime was required but not staged after closure collection"
            )
        rewrite_linux_stage(binaries, bundled_libs)
        verify_linux(stage, [f"{n}{exe_suffix}" for n in bin_names], require_ort=needs_ort)
    elif sys.platform.startswith("win"):
        for lib in ort_libs:
            dst = stage / lib.name
            copy_file(lib, dst)
            bundled_libs.append(dst)
        # Static ort still needs its DirectML EP DLL next to the exe.
        for dll in find_windows_ort_ep_dlls(bin_dir):
            dst = stage / dll.name
            if not dst.exists():
                copy_file(dll, dst)
                bundled_libs.append(dst)
        for dll in find_windows_openblas_dlls(vcpkg_prefix):
            dst = stage / dll.name
            copy_file(dll, dst)
            bundled_libs.append(dst)
        verify_windows(stage, [f"{n}{exe_suffix}" for n in bin_names], require_ort=needs_ort)
    else:
        raise RuntimeError(f"unsupported packaging platform: {sys.platform}")

    copy_file(license_path, stage / "LICENSE")
    include_gui = any(name == "humm-gui" for name in bin_names)
    bundled_ort = any("onnxruntime" in p.name.lower() for p in bundled_libs)
    contents = ["gui=yes" if include_gui else "gui=no"]
    if include_gui:
        contents.append("humm-gui is included in this archive.")
    else:
        contents.append("This archive is CLI-only (humm-gui was not built for this target).")
    contents.append(f"onnxruntime_shared={'yes' if bundled_ort else 'no'}")
    (stage / "CONTENTS.txt").write_text("\n".join(contents) + "\n", encoding="utf-8")
    write_runtime_txt(stage, include_gui=include_gui, bundled_ort=bundled_ort)

    for path in binaries:
        if not path.is_file():
            raise RuntimeError(f"staged binary disappeared before smoke: {path}")
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
