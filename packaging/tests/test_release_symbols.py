"""Release symbol archives: build ids, archive layout and the Linux debug split."""

import importlib.util
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tempfile
import unittest
from unittest.mock import patch
import zipfile


PACKAGING = Path(__file__).resolve().parents[1]
SOURCE = PACKAGING.parent
SCRIPT = PACKAGING / "release-symbols.py"
SPEC = importlib.util.spec_from_file_location("release_symbols", SCRIPT)
symbols = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(symbols)

GUID = struct.pack("<IHH", 0xABCD1234, 0xF00D, 0xBEEF) + bytes(range(1, 9))
BREAKPAD_ID = "ABCD1234F00DBEEF01020304050607081"
UUID_BYTES = bytes(range(0x10, 0x20))
BUILD_ID = bytes(range(0xA0, 0xB4))


def pe_image(pdb_name):
    """A PE32+ image with one section holding a CodeView debug directory entry."""
    image = bytearray(0x400)
    image[0:2] = b"MZ"
    struct.pack_into("<I", image, 0x3C, 0x40)
    image[0x40:0x44] = b"PE\0\0"
    optional_size = 112 + 16 * 8
    struct.pack_into("<HH", image, 0x44, 0x8664, 1)
    struct.pack_into("<H", image, 0x54, optional_size)
    optional = 0x58
    struct.pack_into("<H", image, optional, 0x20B)
    struct.pack_into("<II", image, optional + 112 + 6 * 8, 0x1000, 28)
    section = optional + optional_size
    image[section:section + 8] = b".rdata\0\0"
    struct.pack_into("<IIII", image, section + 8, 0x200, 0x1000, 0x200, 0x200)
    record = b"RSDS" + GUID + struct.pack("<I", 1) + pdb_name.encode() + b"\0"
    struct.pack_into("<IIII", image, 0x200 + 12, 2, len(record), 0x1040, 0x240)
    image[0x240:0x240 + len(record)] = record
    return bytes(image)


def macho_image():
    """A thin 64-bit Mach-O whose second load command is LC_UUID."""
    commands = struct.pack("<II", 0x19, 16) + bytes(8) + struct.pack("<II", 0x1B, 24) + UUID_BYTES
    return struct.pack("<IIIIIIII", 0xFEEDFACF, 0x0100000C, 0, 2, 2, len(commands), 0, 0) + commands


def elf_image():
    """A 64-bit ELF whose PT_NOTE holds another vendor's note before the GNU build id."""
    def note(name, kind, descriptor):
        padded = lambda data: data + bytes(-len(data) % 4)
        return struct.pack("<III", len(name), len(descriptor), kind) + padded(name) + padded(descriptor)

    notes = note(b"Go\0\0", 4, b"go-build") + note(b"GNU\0", 3, BUILD_ID)
    header = bytearray(64)
    header[0:6] = b"\x7fELF\x02\x01"
    struct.pack_into("<Q", header, 0x20, 64)
    struct.pack_into("<HH", header, 0x36, 56, 1)
    program = struct.pack("<IIQQQQQQ", 4, 4, 120, 0, 0, len(notes), len(notes), 4)
    return bytes(header) + program + notes


class BuildIdTests(unittest.TestCase):
    def test_pe_codeview_names_guid_age_and_pdb(self):
        self.assertEqual(symbols.pe_codeview(pe_image(r"D:\a\target\release\deps\cinnabar-1.pdb")),
                         (BREAKPAD_ID, r"D:\a\target\release\deps\cinnabar-1.pdb"))

    def test_macho_uuid_is_read_from_its_load_command(self):
        self.assertEqual(symbols.macho_uuid(macho_image()), "10111213-1415-1617-1819-1A1B1C1D1E1F")

    def test_elf_build_id_skips_other_notes(self):
        self.assertEqual(symbols.elf_build_id(elf_image()), BUILD_ID.hex())

    def test_other_formats_have_no_id(self):
        for parse in (symbols.pe_codeview, symbols.macho_uuid, symbols.elf_build_id):
            with self.subTest(parse=parse.__name__):
                self.assertIsNone(parse(b"#!/bin/sh\n" + bytes(64)))

    def test_split_debug_info_follows_the_release_profile(self):
        self.assertTrue(symbols.has_symbols(SOURCE))
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            for profile, expected in [("", False), ('debug = "line-tables-only"\n', False),
                                      ('split-debuginfo = "packed"\n', False),
                                      ('debug = false\nsplit-debuginfo = "packed"\n', False),
                                      ('debug = 1\nsplit-debuginfo = "packed"\n', True)]:
                with self.subTest(profile=profile):
                    (source / "Cargo.toml").write_text("[profile.release]\n" + profile)
                    self.assertEqual(symbols.has_symbols(source), expected)

    def test_packaging_overrides_debug_generation_in_an_older_source_checkout(self):
        with tempfile.TemporaryDirectory() as temporary:
            source = Path(temporary)
            (source / "Cargo.toml").write_text("[profile.release]\ndebug = false\n")
            with patch.dict(os.environ, {"CARGO_PROFILE_RELEASE_DEBUG": "line-tables-only"}):
                self.assertTrue(symbols.has_symbols(source))
            with patch.dict(os.environ, {"CARGO_PROFILE_RELEASE_DEBUG": "false"}):
                self.assertFalse(symbols.has_symbols(SOURCE))

    def test_older_android_sources_keep_the_stripped_packaging_path(self):
        with tempfile.TemporaryDirectory() as temporary:
            for source, expected in [(Path(temporary), "false"), (SOURCE, "true")]:
                result = subprocess.run([sys.executable, str(SCRIPT), "android-supported"],
                    env={**os.environ, "CINNABAR_SOURCE_ROOT": str(source)}, capture_output=True, text=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout.strip(), expected)

    def test_every_cargo_runtime_binary_and_the_prep_kit_compiler_need_symbols(self):
        cargo = [name for name, rust in symbols.runtime_binaries(SOURCE) if rust]
        self.assertTrue(cargo)
        self.assertEqual(symbols.rust_binaries(SOURCE), cargo + [symbols.PREP_KIT_COMPILER])
        self.assertFalse(set(symbols.rust_binaries(SOURCE)) & {name for name, rust in symbols.runtime_binaries(SOURCE) if not rust})


class CollectTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.binaries = self.root / "release"
        (self.binaries / "deps").mkdir(parents=True)
        self.out = self.root / "out" / "symbols.zip"

    def collect(self, platform, source=SOURCE):
        output = self.root / "github-output"
        output.write_text("")
        result = subprocess.run(
            [sys.executable, str(SCRIPT), "collect", platform, "x86_64", "--version", "9.8.7",
             "--commit", "c0ffee", "--binaries", str(self.binaries), "--out", str(self.out)],
            text=True, capture_output=True,
            env={**os.environ, "CINNABAR_SOURCE_ROOT": str(source), "GITHUB_OUTPUT": str(output)},
        )
        self.archived = output.read_text()
        return result

    def archive(self):
        with zipfile.ZipFile(self.out) as archive:
            manifest = json.loads(archive.read(symbols.MANIFEST))
            return set(archive.namelist()), manifest

    def assert_manifest(self, manifest, platform):
        self.assertEqual(manifest["schema"], symbols.MANIFEST_SCHEMA)
        self.assertEqual((manifest["version"], manifest["commit"], manifest["platform"]),
                         ("9.8.7", "c0ffee", f"{platform}-x86_64"))
        self.assertEqual([record["name"] for record in manifest["binaries"]],
                         [name + (".exe" if platform == "windows" else "") for name in symbols.rust_binaries(SOURCE)])

    def test_windows_archives_each_pdb_under_the_name_its_executable_records(self):
        for name in symbols.rust_binaries(SOURCE):
            linked = f"{name.replace('-', '_')}-0123abcd.pdb"
            (self.binaries / f"{name}.exe").write_bytes(pe_image(rf"D:\a\target\release\deps\{linked}"))
            (self.binaries / "deps" / linked).write_bytes(b"pdb for " + name.encode())
        result = self.collect("windows")
        self.assertEqual(result.returncode, 0, result.stderr)
        names, manifest = self.archive()
        expected = {symbols.MANIFEST}
        for name in symbols.rust_binaries(SOURCE):
            expected |= {f"{name}.exe", f"{name.replace('-', '_')}-0123abcd.pdb"}
        self.assertEqual(names, expected)
        self.assert_manifest(manifest, "windows")
        self.assertEqual(self.archived, "archived=true\n")
        for record in manifest["binaries"]:
            self.assertEqual((record["build_id"], record["build_id_kind"]), (BREAKPAD_ID, "pe-guid-age"))

    def test_windows_without_a_pdb_fails_naming_it(self):
        for name in symbols.rust_binaries(SOURCE):
            (self.binaries / f"{name}.exe").write_bytes(pe_image(f"{name}.pdb"))
        result = self.collect("windows")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(f"{symbols.rust_binaries(SOURCE)[0]}.pdb", result.stderr)
        self.assertFalse(self.out.exists())

    def test_a_source_without_split_debug_info_publishes_no_archive_with_a_warning(self):
        source = self.root / "older-source"
        (source / "packaging/common").mkdir(parents=True)
        shutil.copyfile(SOURCE / symbols.RUNTIME_BINARIES, source / symbols.RUNTIME_BINARIES)
        (source / "Cargo.toml").write_text('[profile.release]\ncodegen-units = 1\nlto = "thin"\n')
        result = self.collect("macos", source)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("::warning::", result.stdout)
        self.assertEqual(self.archived, "archived=false\n")
        self.assertFalse(self.out.exists())

    def test_macos_archives_each_dsym_beside_its_binary_under_the_binary_name(self):
        for index, name in enumerate(symbols.rust_binaries(SOURCE)):
            (self.binaries / name).write_bytes(macho_image())
            # Cargo may uplift a bundle with underscores; the archive renames it.
            bundle = name if index % 2 == 0 else name.replace("-", "_")
            dwarf = self.binaries / f"{bundle}.dSYM/Contents/Resources/DWARF"
            dwarf.mkdir(parents=True)
            (dwarf / f"{bundle}-hash").write_bytes(b"dwarf")
        result = self.collect("macos")
        self.assertEqual(result.returncode, 0, result.stderr)
        names, manifest = self.archive()
        expected = {symbols.MANIFEST}
        for index, name in enumerate(symbols.rust_binaries(SOURCE)):
            bundle = name if index % 2 == 0 else name.replace("-", "_")
            expected |= {name, f"{name}.dSYM/Contents/Resources/DWARF/{bundle}-hash"}
        self.assertEqual(names, expected)
        self.assert_manifest(manifest, "macos")
        self.assertEqual({record["build_id"] for record in manifest["binaries"]},
                         {"10111213-1415-1617-1819-1A1B1C1D1E1F"})

    def test_linux_archives_split_binaries_and_their_debug_files(self):
        for name in symbols.rust_binaries(SOURCE):
            (self.binaries / name).write_bytes(elf_image())
            (self.binaries / f"{name}{symbols.LINUX_DEBUG_SUFFIX}").write_bytes(b"debug")
        result = self.collect("linux")
        self.assertEqual(result.returncode, 0, result.stderr)
        names, manifest = self.archive()
        expected = {symbols.MANIFEST}
        for name in symbols.rust_binaries(SOURCE):
            expected |= {name, f"{name}{symbols.LINUX_DEBUG_SUFFIX}"}
        self.assertEqual(names, expected)
        self.assert_manifest(manifest, "linux")
        self.assertEqual({record["build_id"] for record in manifest["binaries"]}, {BUILD_ID.hex()})


class SplitTests(unittest.TestCase):
    def test_rust_binaries_lose_debug_info_to_a_linked_file_and_go_binaries_are_copied(self):
        executable = Path(sys.executable).resolve()
        if shutil.which("objcopy") is None or executable.read_bytes()[:4] != b"\x7fELF":
            self.skipTest("missing fixture: objcopy and an ELF Python interpreter (Linux packaging host)")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binaries, out = root / "release", root / "shipped"
            binaries.mkdir()
            # A distribution interpreter may already carry a debug link; a linked Rust binary does not.
            fixture = root / "fixture"
            subprocess.run(["objcopy", "--remove-section=.gnu_debuglink", str(executable), str(fixture)],
                           check=True)
            names = [name for name, _ in symbols.runtime_binaries(SOURCE)] + [symbols.PREP_KIT_COMPILER]
            for name in names:
                shutil.copyfile(fixture, binaries / name)
            subprocess.run([sys.executable, str(SCRIPT), "split", str(binaries), str(out)], check=True,
                           env={**os.environ, "CINNABAR_SOURCE_ROOT": str(SOURCE)})
            rust = set(symbols.rust_binaries(SOURCE))
            for name in names:
                with self.subTest(binary=name):
                    shipped = (out / name).read_bytes()
                    if name in rust:
                        self.assertTrue((out / f"{name}{symbols.LINUX_DEBUG_SUFFIX}").is_file())
                        self.assertIn(b".gnu_debuglink", shipped)
                        self.assertIn(f"{name}{symbols.LINUX_DEBUG_SUFFIX}".encode(), shipped)
                    else:
                        self.assertEqual(shipped, fixture.read_bytes())
                        self.assertFalse((out / f"{name}{symbols.LINUX_DEBUG_SUFFIX}").exists())


if __name__ == "__main__":
    unittest.main()
