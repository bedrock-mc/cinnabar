#!/usr/bin/env python3
"""Prepare an immutable spectator runtime bundle; generated artwork stays outside git."""
import argparse
import gzip
import hashlib
import json
from pathlib import Path
import shutil
import stat

parser = argparse.ArgumentParser()
parser.add_argument("directory", type=Path, help="directory containing the locally compiled carriers")
parser.add_argument("--actors", type=Path, help="compiled actor carrier to stage as actors.mcbeact")
parser.add_argument("--particles", type=Path, help="compiled particle carrier to stage as particles.mcbept")
args = parser.parse_args()
root = Path(__file__).resolve().parents[2]
target = json.loads((root / "assets/bedrock-target.json").read_text())
source = json.loads((root / "assets/vanilla-source.json").read_text())
bundle = args.directory.resolve()
shutil.copyfile(root / target["artifacts"]["block_registry"], bundle / "registry.bin")
carriers = (
    ("world", "world.mcbea"), ("registry", "registry.bin"),
    ("entities", "entities.mcbeent"), ("equipment", "equipment.mcbeeqp"),
    ("hud", "hud.mcbehud"), ("icons", "icons.mcbeico"),
    ("ui", "ui.mcbeui"), ("font", "font.mcbefont"),
    ("actors", "actors.mcbeact"), ("particles", "particles.mcbept"),
)
for name, filename, carrier_source in (
    ("actors", "actors.mcbeact", args.actors),
    ("particles", "particles.mcbept", args.particles),
):
    if carrier_source is not None:
        if not carrier_source.is_file() or not stat.S_ISREG(carrier_source.lstat().st_mode):
            parser.error(f"{name} carrier source must be a regular file: {carrier_source}")
        destination = bundle / filename
        if carrier_source.resolve() != destination.resolve():
            shutil.copyfile(carrier_source, destination)
    staged = bundle / filename
    if not staged.is_file() or not stat.S_ISREG(staged.lstat().st_mode):
        parser.error(f"{name} carrier must be staged at {staged} or supplied with --{name}")
records = []
for name, filename in carriers:
    path = bundle / filename
    with path.open("rb") as raw:
        digest = hashlib.file_digest(raw, "sha256").hexdigest()
    with path.open("rb") as raw, gzip.open(str(path) + ".gz", "wb", compresslevel=6) as zipped:
        shutil.copyfileobj(raw, zipped)
    records.append(dict(name=name, file=filename, sha256=digest, size=path.stat().st_size))
manifest = dict(version=1, protocol=target["wire_protocol"], source=source["tag"], files=records)
(bundle / "manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
print(f"Prepared {len(records)} carriers ({sum(r['size'] for r in records)} bytes) at {bundle}")
