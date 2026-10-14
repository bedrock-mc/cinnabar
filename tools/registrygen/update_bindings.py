"""Refresh derived carrier hashes after running registrygen."""

import argparse
import hashlib
import json
import re
import subprocess
import tempfile
from pathlib import Path


def digest(path):
    """Hash a generated artifact without changing its contents."""
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write_json(path, value):
    """Write a stable manifest beneath the selected output directory."""
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=2) + "\n")


def data_files(root):
    """Find tracked carrier inputs without including fetched or ignored content."""
    paths = subprocess.check_output([
        "git", "-C", str(root), "ls-files", "-z", "--", "assets",
        "crates/assets/data", "crates/asset-compiler/data",
    ])
    return [Path(path.decode()) for path in paths.split(b"\0") if path]


def json_bindings(value, registry_path):
    """Read registry pins from carrier fields and path-based manifest references."""
    if isinstance(value, list):
        for child in value:
            yield from json_bindings(child, registry_path)
    elif isinstance(value, dict):
        for key in ("registry_sha256", "breg_sha256"):
            if key in value:
                yield value[key]
        if value.get("path") == registry_path and "sha256" in value:
            yield value["sha256"]
        artifacts = value.get("artifacts", {})
        for name, path in (artifacts.items() if isinstance(artifacts, dict) else ()):
            if path == registry_path:
                yield value["hashes"][name]
        outputs = value.get("outputs", {})
        for name, path in (outputs.items() if isinstance(outputs, dict) else ()):
            if path == registry_path:
                yield value["projection_bindings"][name]["sha256"]
        for child in value.values():
            yield from json_bindings(child, registry_path)


def check_bindings(root, output=None):
    """Reject stale or unrecognized registry pins, including binary carrier headers."""
    def source(path):
        """Prefer a newly generated artifact over its tracked input."""
        generated = output / path if output is not None else root / path
        return generated if generated.is_file() else root / path

    target = json.loads(source(Path("assets/bedrock-target.json")).read_text())
    registry_path = target["artifacts"]["block_registry"]
    expected = digest(source(Path(registry_path)))
    protocol = target["wire_protocol"]
    files = {}
    pins = {}
    for path in data_files(root):
        data = source(path).read_bytes()
        files[path] = data
        values = []
        if path.suffix == ".json" and b"sha256" in data:
            value = json.loads(data)
            if not isinstance(value, dict) or value.get("protocol", protocol) == protocol:
                values.extend(json_bindings(value, registry_path))
        elif path.suffix == ".tsv" and f"v{protocol}" in path.name:
            values.extend(match.decode() for match in re.findall(
                rb"(?m)^# registry_sha256=([0-9a-f]{64})\r?$", data))
        elif path == Path(registry_path + ".sha256"):
            values.append(data.split()[0].decode())
        elif data[:4] in (b"LREG", b"PREG") and len(data) >= 48:
            if int.from_bytes(data[8:12], "little") == protocol:
                values.append(data[16:48].hex())
        if values:
            pins[path] = values
    known = {expected, *(pin for values in pins.values() for pin in values)}
    errors = [f"{path}: registry pin {pin}, expected {expected}"
              for path, values in pins.items() for pin in values if pin != expected]
    for path, data in files.items():
        if path not in pins and any(
                pin.encode() in data or bytes.fromhex(pin) in data for pin in known):
            errors.append(f"{path}: unrecognized block-registry binding format")
    if errors:
        raise ValueError("\n".join(errors))


def generate_tables(root, output, target, projection):
    """Regenerate Go-owned registry dependents against the refreshed target bytes."""
    registry_path = Path(target["artifacts"]["block_registry"])
    routes_path = Path(target["artifacts"]["block_item_routes"])
    palette_path = Path(f"crates/assets/data/server-defined-blocks-v{target['wire_protocol']}.tsv")
    (output / routes_path).parent.mkdir(parents=True, exist_ok=True)
    module = root / "tools/registrygen"
    subprocess.run([
        "go", "-C", str(module), "run", ".", "-block-item-breg",
        str(root / registry_path), "-block-item-out", str(output / routes_path),
    ], check=True)
    # The generator reads one coherent target without changing its source checkout.
    with tempfile.TemporaryDirectory() as directory:
        stage = Path(directory)
        write_json(stage / "assets/bedrock-target.json", target)
        write_json(stage / f"assets/block-projection-v{target['wire_protocol']}.json", projection)
        vanilla = json.loads((root / "assets/vanilla-source.json").read_text())
        cache = stage / vanilla["cache_dir"]
        cache.parent.mkdir(parents=True, exist_ok=True)
        cache.symlink_to(root / vanilla["cache_dir"], target_is_directory=True)
        write_json(stage / "assets/vanilla-source.json", vanilla)
        registry = stage / registry_path
        registry.parent.mkdir(parents=True, exist_ok=True)
        registry.symlink_to(root / registry_path)
        subprocess.run([
            "go", "-C", str(module), "run", "./cmd/serverdefined", "-root",
            str(stage), "-out", str(output / palette_path),
        ], check=True)


def refresh(root, output):
    """Refresh Go tables and manifests, then verify every compiled registry binding."""
    root, output = root.resolve(), output.resolve()
    target_path = Path("assets/bedrock-target.json")
    target = json.loads((root / target_path).read_text())
    protocol = target["wire_protocol"]
    projection_path = Path(f"assets/block-projection-v{protocol}.json")
    projection = json.loads((root / projection_path).read_text())
    registry_hash = digest(root / target["artifacts"]["block_registry"])
    target["hashes"]["block_registry"] = registry_hash
    projection["output"]["sha256"] = registry_hash
    generate_tables(root, output, target, projection)
    for name in target["hashes"]:
        artifact = Path(target["artifacts"][name])
        generated = output / artifact
        target["hashes"][name] = digest(generated if generated.is_file() else root / artifact)
    write_json(output / target_path, target)
    write_json(output / projection_path, projection)
    foundation_path = Path(f"assets/registry-foundation-v{protocol}.json")
    foundation = json.loads((root / foundation_path).read_text())
    foundation["sources"]["protocolgen"] = projection["source"]
    for name, binding in foundation["projection_bindings"].items():
        binding["sha256"] = digest(root / foundation["outputs"][name])
    write_json(output / foundation_path, foundation)
    fallback_path = Path(f"assets/vanilla-fallback-source-v{protocol}.json")
    fallback = json.loads((root / fallback_path).read_text())
    for name in ("inventory", "input_inventory", "registry", "legacy_registry"):
        fallback[name]["sha256"] = digest(root / fallback[name]["path"])
    write_json(output / fallback_path, fallback)
    check_bindings(root, output)


def main():
    """Refresh bindings in place or reproduce them in a scratch directory."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[2])
    parser.add_argument("--out", type=Path)
    parser.add_argument("--check", action="store_true", help="verify every tracked registry binding")
    args = parser.parse_args()
    if args.check:
        check_bindings(args.root.resolve())
    elif args.out is not None:
        refresh(args.root, args.out)
    else:
        parser.error("provide --out or --check")


if __name__ == "__main__":
    main()
