#!/usr/bin/env python3
"""Shared release asset names, standalone installer rendering, and payload validation."""
import argparse
import hashlib
import json
from pathlib import Path
import re

HERE = Path(__file__).resolve().parent


def manifest():
    return json.loads((HERE / "release-assets.json").read_text())


def render_installer():
    config = manifest()
    template = (HERE / "install.sh.in").read_text()
    values = {"REPOSITORY": config["repository"], "CHECKSUM_ASSET": config["checksums"],
              "LINUX_X86_64_ASSET": config["assets"]["linux"]["x86_64"]}
    for key, value in values.items():
        template = template.replace(f"@{key}@", value)
    if re.search(r"@[A-Z_0-9]+@", template):
        raise ValueError("installer template contains an unresolved substitution")
    return template


def checksum(directory):
    config = manifest()
    expected = [name for arches in config["assets"].values() for name in arches.values()]
    expected.extend(config["additional_assets"])
    expected.append(config["install_script"])
    missing = [name for name in expected if not (directory / name).is_file()]
    if missing:
        raise ValueError("release is missing required assets: " + ", ".join(missing))
    files = sorted(p for p in directory.iterdir() if p.is_file() and p.name != config["checksums"])
    lines = []
    for p in files:
        with p.open('rb') as source:
            lines.append(f"{hashlib.file_digest(source, 'sha256').hexdigest()}  {p.name}\n")
    (directory / config["checksums"]).write_text("".join(lines))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    commands = parser.add_subparsers(dest="command", required=True)
    asset = commands.add_parser("asset")
    asset.add_argument("platform")
    asset.add_argument("arch")
    install = commands.add_parser("install-script")
    install.add_argument("--output", type=Path, required=True)
    environment = commands.add_parser("env")
    environment.add_argument("platform", nargs="?")
    environment.add_argument("arch", nargs="?")
    update = commands.add_parser("update-artifacts")
    update.add_argument("base")
    update.add_argument("directory", type=Path)
    checksums = commands.add_parser("checksums")
    checksums.add_argument("directory", type=Path)
    args = parser.parse_args()
    if args.command == "asset":
        print(manifest()["assets"][args.platform][args.arch])
    elif args.command == "env":
        config = manifest()
        for key, field in [("INSTALL_SCRIPT", "install_script"), ("UPDATE_MANIFEST", "update_manifest"),
                           ("CHECKSUM_ASSET", "checksums")]:
            print(f"{key}={config[field]}")
        if args.platform:
            print(f'RELEASE_ASSET={config["assets"][args.platform][args.arch]}')
            if args.platform == "windows":
                print(f'AUXILIARY_ASSET={config["additional_assets"][0]}')
    elif args.command == "update-artifacts":
        for platform, arches in manifest()["assets"].items():
            for arch, filename in arches.items():
                print(f"{platform}-{arch}={args.base}/{filename}={args.directory / filename}")
    elif args.command == "install-script":
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(render_installer())
        args.output.chmod(0o755)
    else:
        checksum(args.directory)


if __name__ == "__main__":
    main()
