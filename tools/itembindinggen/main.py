"""Regenerate reviewed icon bindings from pinned item definitions."""

import argparse
import hashlib
import json
import re
import struct
from pathlib import Path


def archive_entries(data):
    """Read bounded, uncompressed item entries from a resource archive."""
    if len(data) < 16 or data[:8] != bytes.fromhex("7d2725b1a0527026"):
        raise ValueError("invalid resource archive header")
    count, version = struct.unpack_from("<II", data, 8)
    start = 16 + count * 256
    if version != 1 or start > len(data):
        raise ValueError("invalid resource archive table")
    entries = {}
    for index in range(count):
        row = data[16 + index * 256 : 16 + (index + 1) * 256]
        length = row[0]
        offset, size = struct.unpack_from("<II", row, 248)
        if not 0 < length < 248 or start + offset + size > len(data):
            raise ValueError("invalid resource archive entry")
        name = row[1 : length + 1].decode("utf-8")
        if name in entries or "/" in name or "\\" in name:
            raise ValueError("duplicate or unsafe resource archive name")
        entries[name] = data[start + offset : start + offset + size]
    return entries


def item_json(raw):
    """Remove JSON comments while preserving quoted strings and source hashes."""
    pattern = r'"(?:[^"\\]|\\.)*"|//[^\n]*|/\*.*?\*/'
    text = re.sub(pattern, lambda match: match[0] if match[0].startswith('"') else " ",
                  raw.decode("utf-8-sig"), flags=re.DOTALL)
    return json.loads(text)


def regenerate(table, samples, archive):
    """Keep reviewed coverage and derive icon keys and hashes from its evidence."""
    data = archive.read_bytes()
    if hashlib.sha256(data).hexdigest() != table["native_item_witness"]["archive_sha256"]:
        raise ValueError("native item archive does not match its pinned hash")
    entries = archive_entries(data)
    for route in table["routes"]:
        evidence = route["evidence_file"]
        if evidence.startswith(table["native_item_witness"]["archive"] + "#"):
            raw = entries[evidence.split("#", 1)[1]]
        else:
            path = (samples / evidence).resolve()
            if not path.is_relative_to(samples.resolve()):
                raise ValueError("item evidence escapes the sample pack")
            raw = path.read_bytes()
        item = item_json(raw)["minecraft:item"]
        if item["description"]["identifier"] != route["identifier"]:
            raise ValueError("item evidence has a different identifier")
        icon = item["components"]["minecraft:icon"]
        if isinstance(icon, dict):
            icon = icon["textures"]["default"]
        if not isinstance(icon, str):
            raise ValueError("item evidence has no default icon key")
        route["default_alias"] = icon
        route["atlas_variant"] = 0
        route["evidence_sha256"] = hashlib.sha256(raw).hexdigest()
    return json.dumps(table, indent=2) + "\n"


def main():
    """Write regenerated bindings without copying any resource payloads."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--table", type=Path, required=True)
    parser.add_argument("--samples", type=Path, required=True)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    result = regenerate(json.loads(args.table.read_text()), args.samples, args.archive)
    args.out.write_text(result)


if __name__ == "__main__":
    main()
