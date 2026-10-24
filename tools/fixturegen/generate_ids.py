"""Generate dependency-free Rust IDs from the checked protocol inputs."""

import argparse
import hashlib
import json
import re
import struct
import subprocess
import tempfile
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
OWNER = ROOT / "crates/protocol-ids"


def read_fields(data, offset, fields, name_key, type_key):
    """Read integer carrier fields in schema order and return the next offset."""
    formats = {"u8": "B", "u16": "H", "u32": "I", "u64": "Q", "i32": "i", "u32[8]": "8I"}
    values = {}
    for field in fields:
        format_string = "<" + formats[field[type_key]]
        raw = struct.unpack_from(format_string, data, offset)
        values[field[name_key]] = raw[0] if len(raw) == 1 else raw
        offset += struct.calcsize(format_string)
    return values, offset


def air_ids(target):
    """Resolve the unique air state from the hash-checked active block registry."""
    schema = json.loads((ROOT / "crates/assets/data/registry-schema.json").read_text())
    data = (ROOT / target["artifacts"]["block_registry"]).read_bytes()
    if hashlib.sha256(data).hexdigest() != target["hashes"]["block_registry"]:
        raise ValueError("active block registry hash mismatch")
    magic = schema["magic"].encode()
    if not data.startswith(magic):
        raise ValueError("unsupported block registry")
    metadata = next(entry for entry in schema["structs"] if entry["rust_name"] == "RegistryMetadata")
    counts, offset = read_fields(data, len(magic), metadata["fields"], "rust_name", "rust_type")
    if counts["protocol"] != target["wire_protocol"]:
        raise ValueError("active block registry protocol mismatch")
    collision = next(entry for entry in schema["structs"] if entry["rust_name"] == "CollisionBox")
    found = None
    for _ in range(counts["canonical_states"]):
        record, offset = read_fields(data, offset, schema["record_header"], "name", "type")
        for _ in range(record["box_count"]):
            _, offset = read_fields(data, offset, collision["fields"], "rust_name", "rust_type")
        name = data[offset:offset + record["name_len"]]
        if name == b"minecraft:air":
            if found is not None:
                raise ValueError("more than one air state")
            found = record["sequential_id"], record["network_hash"]
        offset += record["name_len"] + record["state_len"]
    if offset != len(data):
        raise ValueError("registry payload length mismatch")
    if found is None:
        raise ValueError("active registry has no air state")
    return found


def codec_directory(target):
    """Resolve the owning protocol manifest's exact pinned codec without network I/O."""
    manifest = ROOT / "crates/protocol/Cargo.toml"
    with tempfile.TemporaryFile() as stdout, tempfile.TemporaryFile() as stderr:
        try:
            result = subprocess.run(
                ["cargo", "metadata", "--locked", "--offline", "--format-version", "1",
                 "--manifest-path", str(manifest)],
                cwd=ROOT, stdout=stdout, stderr=stderr, timeout=120, check=False,
            )
        except subprocess.TimeoutExpired as error:
            raise ValueError("codec cargo metadata timed out after 120 seconds") from error
        output = bounded_output(stdout, "stdout", 32 * 1024 * 1024)
        error = bounded_output(stderr, "stderr", 256 * 1024)
    if result.returncode:
        raise ValueError(
            "codec cargo metadata failed; run cargo fetch --locked first: " + error.strip()
        )
    metadata = json.loads(output)
    owners = [package for package in metadata["packages"]
              if Path(package["manifest_path"]).resolve() == manifest.resolve()]
    if len(owners) != 1:
        raise ValueError("codec metadata must contain exactly one canonical protocol owner")
    name, revision = target["codec_crate"], target["protocolgen_commit"]
    source = f"git+https://github.com/cinnabar-mc/protocolgen?rev={revision}"
    resolved_source = f"{source}#{revision}"
    dependencies = [entry for entry in owners[0]["dependencies"]
                    if entry["name"] == name or entry.get("rename") == name]
    if len(dependencies) != 1:
        raise ValueError("protocol must declare exactly one pinned generated codec dependency")
    dependency = dependencies[0]
    if (dependency["name"] != name or dependency.get("rename") is not None
            or dependency["source"] not in (source, resolved_source)
            or dependency.get("path") is not None or dependency["kind"] is not None
            or dependency["target"] is not None or dependency["optional"] is not False):
        raise ValueError("protocol codec dependency differs from the canonical target pin")
    packages = [package for package in metadata["packages"] if package["name"] == name]
    if len(packages) != 1 or packages[0]["source"] != resolved_source:
        raise ValueError("resolved codec package differs from the canonical target pin")
    return Path(packages[0]["manifest_path"]).resolve(strict=True).parent


def bounded_output(stream, label, maximum_bytes):
    """Bound metadata decoding while keeping subprocess output off the Python heap."""
    if stream.tell() > maximum_bytes:
        raise ValueError(f"codec cargo metadata {label} exceeds {maximum_bytes} bytes")
    stream.seek(0)
    return stream.read().decode("utf-8")


def enchantments(target):
    """Read enchantment values from Cargo's resolved generated codec source."""
    path = codec_directory(target) / "src/enums.rs"
    body = path.read_text().split("impl EnchantType {", 1)[1].split("Self::Unknown(value)", 1)[0]
    return {name: int(raw) for name, raw in re.findall(r"Self::(\w+) => (\d+),", body)}


def declarations():
    """Render all named wire values and air constants from their owning inputs."""
    fixture = json.loads((OWNER / "data/protocol-ids.json").read_text())
    extensions = json.loads((OWNER / "data/actor-data-extensions.json").read_text())
    target = json.loads((ROOT / "assets/bedrock-target.json").read_text())
    raw_types = {
        "ActorDataKey": "u32", "ActorFlag": "u32", "CommandArgType": "u32",
        "CommandArgFlag": "u32", "ContainerName": "u8", "WindowType": "i8",
        "Effect": "i32", "LevelEvent": "i32", "WindowID": "i32",
    }
    groups = {}
    for name, values in fixture["groups"].items():
        values = dict(values)
        if name == "ActorDataKey":
            if values.keys() & extensions.keys():
                raise ValueError("actor data extensions duplicate a pinned name")
            values.update(extensions)
        groups[name] = raw_types[name], dict(sorted(values.items()))
    movement = json.loads((OWNER / "data/movement-contract.json").read_text())
    for name, values in movement["numeric"].items():
        raw_type, pinned = groups.get(name, ("u8" if name == "PlayerFlag" else "u32", {}))
        if pinned.keys() & values.keys():
            raise ValueError("movement extensions duplicate a pinned name")
        groups[name] = raw_type, dict(sorted((pinned | values).items()))
    groups["Enchantment"] = "u8", enchantments(target)
    lines = ["// Generated by tools/fixturegen/generate_ids.py; do not edit."]
    for name, (raw_type, values) in groups.items():
        lines.extend(["", f"wire_ids!({name}, {raw_type}, {{"])
        lines.extend(f"    {variant} => {raw}," for variant, raw in values.items())
        lines.append("});")
    lines.extend(["", "/// Named movement and health attribute contract.",
                  "#[derive(Clone, Copy, Debug, Eq, PartialEq)]", "pub enum Attribute {"])
    lines.extend(f"    {name}," for name in movement["attributes"])
    lines.extend(["}", "impl Attribute {", "    /// Every attribute named by the movement contract.",
                  "    pub const ALL: &'static [Self] = &["])
    lines.extend(f"        Self::{name}," for name in movement["attributes"])
    lines.extend(["    ];", "    /// Returns the server attribute name without allocating.",
                  "    pub const fn wire_name(self) -> &'static str {", "        match self {"])
    lines.extend(f"            Self::{name} => {json.dumps(value)}," for name, value in movement["attributes"].items())
    lines.extend(["        }", "    }", "}", "", "/// Identity of the sprint speed modifier.",
                  f"pub const SPRINT_SPEED_MODIFIER_ID: &str = {json.dumps(movement['sprint_speed_modifier_id'])};",
                  "", "/// Identity of the Speed effect's movement modifier.",
                  f"pub const SPEED_EFFECT_MODIFIER_ID: &str = {json.dumps(movement['speed_effect_modifier_id'])};"])
    sequential, hashed = air_ids(target)
    lines.extend([
        "", "/// Sequential air identity derived from the active block registry.",
        f"pub const SEQUENTIAL_AIR_NETWORK_ID: u32 = {sequential};",
        "/// Hashed air identity derived from the active block registry.",
        f"pub const HASHED_AIR_NETWORK_ID: u32 = {hashed};",
    ])
    return "\n".join(lines) + "\n"


def main():
    """Write formatted Rust declarations or reject a stale generated file."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args()
    result = subprocess.run(
        ["rustfmt", "--edition", "2024", "--emit", "stdout"],
        input=declarations(), text=True, capture_output=True, check=True,
    )
    destination = OWNER / "src/generated.rs"
    if args.check:
        if destination.read_text() != result.stdout:
            raise SystemExit("protocol ID declarations drift; run tools/fixturegen/generate_ids.py")
    else:
        destination.write_text(result.stdout)


if __name__ == "__main__":
    main()
