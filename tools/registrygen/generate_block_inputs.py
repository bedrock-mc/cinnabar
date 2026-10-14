#!/usr/bin/env python3
"""Extract block property domains and component facts from the pinned sample archive."""

import argparse
import hashlib
import json
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def generate(archive, out):
    """Validate the archive and write only property definitions and selected component facts."""
    manifest = json.loads((ROOT / "assets/vanilla-source.json").read_text())
    if hashlib.sha256(archive.read_bytes()).hexdigest() != manifest["sha256"]:
        raise ValueError("sample archive does not match the pinned SHA-256")
    with zipfile.ZipFile(archive) as source:
        metadata = json.loads(source.read("metadata/vanilladata_modules/mojang-blocks.json"))
        properties = sorted(metadata["block_properties"], key=lambda p: p["name"])
        components = []
        for path in sorted(source.namelist()):
            if not path.startswith("behavior_pack/blocks/") or not path.endswith(".json"):
                continue
            block = json.loads(source.read(path))["minecraft:block"]
            values = block.get("components", {})
            facts = [name for name in ("minecraft:flammable", "minecraft:replaceable") if name in values]
            components.append((block["description"]["identifier"], ",".join(facts)))
    destination = out / "crates/assets/data"
    destination.mkdir(parents=True, exist_ok=True)
    document = {"schema": 1, "source_sha256": manifest["sha256"], "properties": properties}
    (destination / "block-properties-v2193.json").write_text(json.dumps(document, indent=2) + "\n")
    (destination / "block-components-v2193.tsv").write_text(
        "identifier\tcomponents\n" + "".join(f"{name}\t{facts or '-'}\n" for name, facts in sorted(components)))


def main():
    """Read explicit source and destination paths without fetching sample assets."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=ROOT)
    args = parser.parse_args()
    generate(args.archive, args.out)


if __name__ == "__main__":
    main()
