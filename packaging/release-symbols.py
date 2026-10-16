#!/usr/bin/env python3
"""Release symbol archives: each shipped Rust binary beside its split debug file, plus a manifest.

Release builds keep line tables only in split files: a PDB on Windows, a `.dSYM` bundle on macOS
and, after `split`, a `.debug` file named by the binary's `.gnu_debuglink` on Linux. Symbolicators
(`devtool symbolicate`) look for those beside the binary, so the archive pairs them. Installers
never contain them: packagers stage binaries by name and check-payload.sh rejects other files.

The release tools may package an older source branch (CINNABAR_SOURCE_ROOT), so the binary table
and profile come from that checkout; a source whose release profile has no split debug info has no
symbol archive, and `collect` warns instead of failing the release.
"""
import argparse
import json
import os
from pathlib import Path
import shutil
import struct
import subprocess
import sys
import tomllib
import uuid
import zipfile

HERE = Path(__file__).resolve().parent
# The runtime binary table, relative to the packaged source checkout.
RUNTIME_BINARIES = Path("packaging/common/runtime-binaries.txt")
# The prep-kit compiler `make package-binaries` builds beside the runtime binaries; installers ship it.
PREP_KIT_COMPILER = "assetc"
# The archive's build description; `devtool symbolicate` reads it under the same configured name.
MANIFEST = json.loads((HERE / "release-assets.json").read_text())["symbols"]["manifest"]
MANIFEST_SCHEMA = 1
LINUX_DEBUG_SUFFIX = ".debug"


def source_root():
    """The packaged source checkout, as the packagers resolve it."""
    return Path(os.environ.get("CINNABAR_SOURCE_ROOT") or HERE.parent).resolve()


def has_symbols(source):
    """Whether packaging enables symbols, falling back to the selected source's profile."""
    debug = os.environ.get("CARGO_PROFILE_RELEASE_DEBUG")
    if debug is not None:
        return debug.lower() not in ("false", "0", "none", "")
    with (source / "Cargo.toml").open("rb") as manifest:
        profile = tomllib.load(manifest).get("profile", {}).get("release", {})
    return (profile.get("split-debuginfo", "off") != "off"
            and profile.get("debug", False) not in (False, 0, "none"))


def runtime_binaries(source):
    """(name, built by Cargo) for each runtime binary; Go rows have no Cargo package."""
    rows = []
    for line in (source / RUNTIME_BINARIES).read_text().splitlines():
        fields = line.split()
        if fields:
            rows.append((fields[0], fields[1] != "-"))
    return rows


def rust_binaries(source):
    """Every shipped binary the release profile builds, so every one with split debug info."""
    return [name for name, rust in runtime_binaries(source) if rust] + [PREP_KIT_COMPILER]


def pe_codeview(data):
    """(breakpad debug id: GUID then age, PDB file name) from a PE's RSDS record, or None."""
    if data[:2] != b"MZ" or len(data) < 0x40:
        return None
    pe = struct.unpack_from("<I", data, 0x3C)[0]
    if data[pe:pe + 4] != b"PE\0\0":
        return None
    sections = struct.unpack_from("<H", data, pe + 6)[0]
    optional_size = struct.unpack_from("<H", data, pe + 20)[0]
    optional = pe + 24
    directories = optional + (112 if struct.unpack_from("<H", data, optional)[0] == 0x20B else 96)
    debug_rva, debug_size = struct.unpack_from("<II", data, directories + 6 * 8)
    table = optional + optional_size

    def file_offset(virtual_address):
        for index in range(sections):
            size, address, raw_size, raw = struct.unpack_from("<IIII", data, table + 40 * index + 8)
            if address <= virtual_address < address + max(size, raw_size):
                return raw + virtual_address - address
        return None

    start = file_offset(debug_rva) if debug_size else None
    if start is None:
        return None
    for entry in range(start, start + debug_size, 28):
        kind, size, _, pointer = struct.unpack_from("<IIII", data, entry + 12)
        if kind != 2 or data[pointer:pointer + 4] != b"RSDS":
            continue
        data1, data2, data3 = struct.unpack_from("<IHH", data, pointer + 4)
        data4 = data[pointer + 12:pointer + 20].hex().upper()
        age = struct.unpack_from("<I", data, pointer + 20)[0]
        path = data[pointer + 24:pointer + size].split(b"\0", 1)[0].decode()
        return f"{data1:08X}{data2:04X}{data3:04X}{data4}{age:X}", path
    return None


def macho_uuid(data):
    """The LC_UUID of a thin 64-bit little-endian Mach-O, or None."""
    if len(data) < 32 or struct.unpack_from("<I", data, 0)[0] != 0xFEEDFACF:
        return None
    offset = 32
    for _ in range(struct.unpack_from("<I", data, 16)[0]):
        command, size = struct.unpack_from("<II", data, offset)
        if command == 0x1B:
            return str(uuid.UUID(bytes=bytes(data[offset + 8:offset + 24]))).upper()
        offset += size
    return None


def elf_build_id(data):
    """The NT_GNU_BUILD_ID of a 64-bit little-endian ELF, as hex, or None."""
    if data[:4] != b"\x7fELF" or data[4] != 2 or data[5] != 1:
        return None
    program_headers = struct.unpack_from("<Q", data, 0x20)[0]
    entry_size, count = struct.unpack_from("<HH", data, 0x36)
    align = lambda size: (size + 3) & ~3
    for index in range(count):
        kind, _, offset, _, _, size = struct.unpack_from("<IIQQQQ", data, program_headers + index * entry_size)
        if kind != 4:  # PT_NOTE
            continue
        position, end = offset, offset + size
        while position + 12 <= end:
            name_size, descriptor_size, note = struct.unpack_from("<III", data, position)
            name = position + 12
            descriptor = name + align(name_size)
            if note == 3 and data[name:name + name_size] == b"GNU\0":
                return bytes(data[descriptor:descriptor + descriptor_size]).hex()
            position = descriptor + align(descriptor_size)
    return None


def require(path, hint):
    if not path.exists():
        raise SystemExit(f"missing {path} ({hint})")
    return path


def first_existing(candidates, hint):
    for path in candidates:
        if path.exists():
            return path
    raise SystemExit("missing " + " or ".join(str(path) for path in candidates) + f" ({hint})")


def windows_entry(binaries, name):
    """The executable and the PDB it names; the PDB is archived under that name."""
    binary = require(binaries / f"{name}.exe", "build with make package-binaries")
    codeview = pe_codeview(binary.read_bytes())
    if codeview is None:
        raise SystemExit(f"{binary} names no PDB; the release profile must emit debug info")
    build_id, pdb_path = codeview
    pdb_name = pdb_path.replace("\\", "/").rsplit("/", 1)[-1]
    # Cargo links in deps/ and uplifts the PDB with underscores; the executable names the linked one.
    pdb = first_existing([binaries / "deps" / pdb_name, binaries / f"{name.replace('-', '_')}.pdb",
                          binaries / f"{name}.pdb"], "the release profile must emit a PDB")
    record = {"name": binary.name, "debug_file": pdb_name, "build_id": build_id, "build_id_kind": "pe-guid-age"}
    return [(binary.name, binary), (pdb_name, pdb)], record


def macos_entry(binaries, name):
    """The executable and its `.dSYM`, renamed to `<binary>.dSYM` where symbolicators look."""
    binary = require(binaries / name, "build with make package-binaries")
    dsym = first_existing([binaries / f"{name}.dSYM", binaries / f"{name.replace('-', '_')}.dSYM"],
                          "the release profile must emit split-debuginfo = packed")
    bundle = f"{name}.dSYM"
    # Cargo uplifts the bundle as a symlink to deps/; archive its contents.
    root = dsym.resolve()
    files = [(f"{bundle}/{path.relative_to(root).as_posix()}", path)
             for path in sorted(root.rglob("*")) if path.is_file()]
    if not files:
        raise SystemExit(f"{dsym} is empty")
    record = {"name": name, "debug_file": bundle, "build_id": macho_uuid(binary.read_bytes()),
              "build_id_kind": "macho-uuid"}
    return [(name, binary)] + files, record


def linux_entry(binaries, name):
    """The stripped executable and the `.debug` file its `.gnu_debuglink` names (see `split`)."""
    binary = require(binaries / name, "run release-symbols.py split first")
    debug = require(binaries / f"{name}{LINUX_DEBUG_SUFFIX}", "run release-symbols.py split first")
    record = {"name": name, "debug_file": debug.name, "build_id": elf_build_id(binary.read_bytes()),
              "build_id_kind": "elf-build-id"}
    return [(name, binary), (debug.name, debug)], record


ENTRIES = {"windows": windows_entry, "macos": macos_entry, "linux": linux_entry}


def collect(source, platform, arch, version, commit, binaries, out):
    """Writes the platform's symbol archive and returns True; every Rust release binary must have
    its debug file. A source without split debug info gets a workflow warning and no archive."""
    if not has_symbols(source):
        print(f"::warning::{source / 'Cargo.toml'} release profile has no split debug info; "
              f"publishing {platform}-{arch} without a symbol archive")
        return False
    files, records = [], []
    for name in rust_binaries(source):
        entry_files, record = ENTRIES[platform](binaries, name)
        files.extend(entry_files)
        records.append(record)
    manifest = {"schema": MANIFEST_SCHEMA, "version": version, "commit": commit,
                "platform": f"{platform}-{arch}", "binaries": records}
    out.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as archive:
        archive.writestr(MANIFEST, json.dumps(manifest, indent=2) + "\n")
        for name, path in files:
            archive.write(path, name)
    return True


def split(source, binaries, out):
    """Copies the runtime binaries and prep-kit compiler to `out`, moving each Rust binary's debug
    info into `<name>.debug` beside it, linked by `.gnu_debuglink`.

    Linux builds keep debug info in the binary (package.yml overrides split-debuginfo and strip):
    `--strip-debug` at link time would also drop the line tables a packed `.dwp` depends on.
    """
    out.mkdir(parents=True, exist_ok=True)
    rust = set(rust_binaries(source))
    for name in [name for name, _ in runtime_binaries(source)] + [PREP_KIT_COMPILER]:
        source = require(binaries / name, "build with make package-binaries")
        if name not in rust:
            shutil.copy2(source, out / name)
            continue
        debug = out / f"{name}{LINUX_DEBUG_SUFFIX}"
        subprocess.run(["objcopy", "--only-keep-debug", str(source), str(debug)], check=True)
        subprocess.run(["objcopy", "--strip-debug", f"--add-gnu-debuglink={debug}", str(source),
                        str(out / name)], check=True)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    splitting = commands.add_parser("split", help="Linux: strip debug info into linked .debug files")
    splitting.add_argument("binaries", type=Path)
    splitting.add_argument("out", type=Path)
    collecting = commands.add_parser("collect", help="zip the platform's binaries and debug files")
    collecting.add_argument("platform", choices=sorted(ENTRIES))
    collecting.add_argument("arch")
    collecting.add_argument("--version", required=True)
    collecting.add_argument("--commit", required=True)
    collecting.add_argument("--binaries", type=Path, required=True)
    collecting.add_argument("--out", type=Path, required=True)
    commands.add_parser("has-symbols", help="print true when the source release build splits debug info")
    args = parser.parse_args()
    source = source_root()
    if args.command == "split":
        split(source, args.binaries, args.out)
    elif args.command == "has-symbols":
        print("true" if has_symbols(source) else "false")
    else:
        archived = collect(source, args.platform, args.arch, args.version, args.commit, args.binaries,
                           args.out)
        if os.environ.get("GITHUB_OUTPUT"):
            with open(os.environ["GITHUB_OUTPUT"], "a") as output:
                output.write(f"archived={'true' if archived else 'false'}\n")


if __name__ == "__main__":
    sys.exit(main())
