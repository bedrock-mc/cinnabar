#!/usr/bin/env python3
"""Extract block property domains and component facts from the pinned sample archive."""

import argparse
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path
import zipfile

ROOT = Path(__file__).resolve().parents[2]


def state_facts(path=None):
    """Read the pinned state export, generating an isolated copy when none was supplied."""
    if path is None:
        with tempfile.TemporaryDirectory(prefix="cinnabar-block-inputs-") as scratch:
            path = Path(scratch) / "facts.json"
            env = dict(os.environ, GOWORK="off", GOMAXPROCS="2", GOFLAGS="-p=2")
            subprocess.run(["go", "-C", "tools/registrygen", "run", ".", "-facts-out", str(path)], cwd=ROOT, env=env, check=True)
            return state_facts(path)
    facts = json.loads(path.read_text())
    projection = json.loads((ROOT / "assets/block-projection-v2193.json").read_text())
    if (facts["protocol"] != projection["protocol"] or facts["game_version"] != projection["game_version"]
            or facts["source_lock_sha256"] != projection["source"]["source_lock_sha256"]):
        raise ValueError("state export does not match the pinned registry source")
    return facts


def complete_domains(properties, facts):
    """Include valid palette values omitted from the sample metadata, including Education states."""
    by_name = {prop["name"]: prop for prop in properties}
    changed = set()
    for block in facts["blocks"]:
        for name, scalar in json.loads(block["state"]).items():
            prop = by_name.get(name)
            if prop is None:
                continue
            value = scalar["value"]
            if prop["type"] == "bool" and scalar["type"] in ("byte", "bool"):
                value = bool(value)
            elif scalar["type"] != prop["type"]:
                raise ValueError(f"inconsistent palette property type: {name}")
            if not any(entry["value"] == value for entry in prop["values"]):
                prop["values"].append({"value": value})
                changed.add(name)
    for name in changed:
        by_name[name]["values"].sort(key=lambda entry: entry["value"])


def narrow_domains(properties, facts):
    """Record blocks whose palette admits only part of a shared property's domain."""
    by_name = {prop["name"]: prop for prop in properties}
    admitted = {}
    for block in facts["blocks"]:
        for name, scalar in json.loads(block["state"]).items():
            if name in by_name:
                admitted.setdefault(name, {}).setdefault(block["name"], set()).add(scalar["value"])
    for name, blocks in admitted.items():
        prop = by_name[name]
        full = {entry["value"] for entry in prop["values"]}
        groups = {}
        for block, values in blocks.items():
            if values != full:
                groups.setdefault(tuple(sorted(values)), []).append(block)
        if groups and prop["type"] == "bool":
            raise ValueError(f"boolean property narrows per block: {name}")
        if groups:
            prop["block_domains"] = [{"values": list(values), "blocks": sorted(names)} for values, names in sorted(groups.items())]


def generate(archive, out, facts_path=None):
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
    facts = state_facts(facts_path)
    complete_domains(properties, facts)
    narrow_domains(properties, facts)
    destination = out / "crates/assets/data"
    destination.mkdir(parents=True, exist_ok=True)
    document = {"schema": 1, "source_sha256": manifest["sha256"], "state_source_lock_sha256": facts["source_lock_sha256"], "properties": properties}
    (destination / "block-properties-v2193.json").write_text(json.dumps(document, indent=2) + "\n")
    (destination / "block-components-v2193.tsv").write_text(
        "identifier\tcomponents\n" + "".join(f"{name}\t{facts or '-'}\n" for name, facts in sorted(components)))


def main():
    """Read explicit source and destination paths without fetching sample assets."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--archive", type=Path, required=True)
    parser.add_argument("--out", type=Path, default=ROOT)
    parser.add_argument("--facts", type=Path, help="optional pinned registry facts export")
    args = parser.parse_args()
    generate(args.archive, args.out, args.facts)


if __name__ == "__main__":
    main()
