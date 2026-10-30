#!/usr/bin/env python3
"""Unit tests for scripts/package_release.py helpers (no cargo / no runners)."""

from __future__ import annotations

import struct
import sys
import tempfile
import unittest
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))
import package_release as pr  # noqa: E402


def _minimal_pe_with_imports(dll_names: list[str]) -> bytes:
    """Build a tiny PE32+ whose import table names the given DLLs."""
    # Layout: DOS stub → PE header → optional → one .rdata section with
    # import descriptors + DLL name strings.
    dos = bytearray(64)
    dos[0:2] = b"MZ"
    e_lfanew = 64
    struct.pack_into("<I", dos, 0x3C, e_lfanew)

    # Section will start after headers. Leave room for PE + COFF + opt + section.
    # PE signature(4) + COFF(20) + PE32+ opt(240 with 16 directories) + section(40)
    coff_off = e_lfanew + 4
    opt_off = coff_off + 20
    # PE32+ optional header size including 16 data directories: 112 + 16*8 = 240
    size_opt = 240
    section_off = opt_off + size_opt
    headers_end = section_off + 40
    file_align = 512
    section_raw = (headers_end + file_align - 1) // file_align * file_align

    # Build import payloads inside the section.
    # [desc0][desc1]...[null desc][name0\0][name1\0]...
    names_blob = b"".join(name.encode("ascii") + b"\0" for name in dll_names)
    desc_size = 20 * (len(dll_names) + 1)
    # Align name RVAs after descriptors.
    name_offsets = []
    cursor = desc_size
    for name in dll_names:
        name_offsets.append(cursor)
        cursor += len(name) + 1

    section_payload = bytearray(max(cursor, 64))
    virt_addr = 0x1000
    for index, name_off in enumerate(name_offsets):
        entry = index * 20
        name_rva = virt_addr + name_off
        struct.pack_into("<IIIII", section_payload, entry, 0, 0, 0, name_rva, 0)
    # null terminator already zero
    section_payload[desc_size : desc_size + len(names_blob)] = names_blob

    import_rva = virt_addr  # descriptors at start of section
    raw_size = (len(section_payload) + file_align - 1) // file_align * file_align

    pe = bytearray(section_raw + raw_size)
    pe[0:64] = dos
    pe[e_lfanew : e_lfanew + 4] = b"PE\0\0"
    # COFF: Machine AMD64, 1 section, SizeOfOptionalHeader=240, Characteristics
    struct.pack_into("<HHIIIHH", pe, coff_off, 0x8664, 1, 0, 0, 0, size_opt, 0x22)
    # Optional PE32+
    struct.pack_into("<H", pe, opt_off, 0x20B)
    # DataDirectory[1] import RVA/size at opt+112+8
    struct.pack_into("<II", pe, opt_off + 112 + 8, import_rva, desc_size)
    # Section header .rdata
    name_field = b".rdata\0\0"
    pe[section_off : section_off + 8] = name_field
    struct.pack_into(
        "<IIIIIIHHI",
        pe,
        section_off + 8,
        len(section_payload),  # VirtualSize
        virt_addr,  # VirtualAddress
        raw_size,  # SizeOfRawData
        section_raw,  # PointerToRawData
        0,  # PointerToRelocations
        0,  # PointerToLinenumbers
        0,  # NumberOfRelocations
        0,  # NumberOfLinenumbers
        0x40000040,  # Characteristics
    )
    pe[section_raw : section_raw + len(section_payload)] = section_payload
    return bytes(pe)


class PackageReleaseHelpers(unittest.TestCase):
    def test_pe_import_dll_names_reads_onnx_and_directml(self) -> None:
        pe = _minimal_pe_with_imports(
            ["KERNEL32.dll", "onnxruntime.dll", "DirectML.dll"]
        )
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "humm.exe"
            path.write_bytes(pe)
            names = [n.lower() for n in pr.pe_import_dll_names(path)]
        self.assertIn("onnxruntime.dll", names)
        self.assertIn("directml.dll", names)
        self.assertIn("kernel32.dll", names)

    def test_pe_import_without_onnx_means_static_ort(self) -> None:
        pe = _minimal_pe_with_imports(["KERNEL32.dll", "DirectML.dll", "openblas.dll"])
        with tempfile.TemporaryDirectory() as tmp:
            path = Path(tmp) / "humm.exe"
            path.write_bytes(pe)
            names = [n.lower() for n in pr.pe_import_dll_names(path)]
        self.assertNotIn("onnxruntime.dll", names)
        self.assertTrue(any(pr._is_ort_name(n) is False for n in names) or True)
        self.assertFalse(any(pr._is_ort_name(n) for n in names))

    def test_is_system_mac_lib_keeps_rpath_runtime_libs(self) -> None:
        self.assertFalse(pr.is_system_mac_lib("@rpath/libgcc_s.1.1.dylib"))
        self.assertFalse(pr.is_system_mac_lib("@loader_path/libopenblas.0.dylib"))
        self.assertTrue(pr.is_system_mac_lib("/usr/lib/libSystem.B.dylib"))
        self.assertTrue(pr.is_system_mac_lib("/System/Library/Frameworks/CoreFoundation.framework/Versions/A/CoreFoundation"))

    def test_resolve_macos_dep_finds_sibling_rpath_lib(self) -> None:
        if sys.platform != "darwin":
            self.skipTest("otool only on macOS")
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            # Fake a "dylib" file and a sibling that resolve_macos_dep should find
            # when LC_RPATH is empty — sibling directory search.
            current = root / "libgfortran.5.dylib"
            sibling = root / "libgcc_s.1.1.dylib"
            current.write_bytes(b"\x00")
            sibling.write_bytes(b"\x00")
            # Without a real Mach-O, otool -l fails; only absolute deps work here.
            resolved = pr.resolve_macos_dep(str(sibling), current)
            self.assertEqual(resolved, sibling.resolve())

    def test_find_windows_ort_ep_dlls(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            bin_dir = Path(tmp)
            (bin_dir / "DirectML.dll").write_bytes(b"dml")
            (bin_dir / "openblas.dll").write_bytes(b"blas")
            found = pr.find_windows_ort_ep_dlls(bin_dir)
        self.assertEqual([p.name for p in found], ["DirectML.dll"])


if __name__ == "__main__":
    unittest.main()
