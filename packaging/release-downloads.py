#!/usr/bin/env python3
"""Shared release asset names, standalone installer rendering, and payload validation."""
import argparse
import hashlib
import json
from pathlib import Path
import re
import shutil

HERE = Path(__file__).resolve().parent


def manifest():
    return json.loads((HERE / "release-assets.json").read_text())


def download_assets(config):
    """All installer downloads, including platforms installed outside the desktop updater."""
    return config["assets"] | config["manual_assets"]


def render_installer():
    config = manifest()
    template = (HERE / "install.sh.in").read_text()
    tagged = next(name for name, release in config["update_channels"].items()
                  if release["track"] == "tagged")
    rolling, release = next((name, release) for name, release in config["update_channels"].items()
                            if release["track"] == "rolling")
    values = {"REPOSITORY": config["repository"], "CHECKSUM_ASSET": config["checksums"],
              "LINUX_X86_64_ASSET": config["assets"]["linux"]["x86_64"],
              "TAGGED_CHANNEL": tagged, "TAGGED_CHANNEL_LABEL": tagged.capitalize(),
              "ROLLING_CHANNEL": rolling, "ROLLING_TAG": release["tag"]}
    for key, value in values.items():
        template = template.replace(f"@{key}@", value)
    if re.search(r"@[A-Z_0-9]+@", template):
        raise ValueError("installer template contains an unresolved substitution")
    return template


def symbols_archive(config, version, platform, arch):
    """The release asset holding one platform's split debug files for `version`."""
    return config["symbols"]["archive"].format(version=version, platform=platform, arch=arch)


def checksum(directory, version, symbols=True):
    """Checksums the release; `symbols` is False only for a source build without split debug info
    (release-symbols.py has-symbols), which publishes no symbol archives."""
    config = manifest()
    expected = [name for arches in download_assets(config).values() for name in arches.values()]
    expected.extend(name for arches in config["update_assets"].values() for name in arches.values())
    if symbols:
        expected.extend(symbols_archive(config, version, platform, arch)
                        for platform, arches in config["assets"].items() for arch in arches)
    expected.append(config["install_script"])
    missing = [name for name in expected if not (directory / name).is_file()]
    if missing:
        raise ValueError("release is missing required assets: " + ", ".join(missing))
    # By name, so the order is the same on case-insensitive Windows paths.
    files = sorted((p for p in directory.iterdir() if p.is_file() and p.name != config["checksums"]),
                   key=lambda p: p.name)
    lines = []
    for p in files:
        with p.open('rb') as source:
            lines.append(f"{hashlib.file_digest(source, 'sha256').hexdigest()}  {p.name}\n")
    (directory / config["checksums"]).write_text("".join(lines))


def update_payloads(directory, out):
    """Copies each platform's update artifact to a content-addressed name, so an upload never
    replaces a payload that a published manifest still names."""
    config = manifest()
    out.mkdir(parents=True, exist_ok=True)
    payloads = []
    for platform, arches in config["assets"].items():
        for arch, filename in arches.items():
            filename = config["update_assets"].get(platform, {}).get(arch, filename)
            source = directory / filename
            with source.open("rb") as payload:
                digest = hashlib.file_digest(payload, "sha256").hexdigest()
            target = out / f'{config["update_payload_prefix"]}{digest[:16]}-{filename}'
            shutil.copyfile(source, target)
            payloads.append((f"{platform}-{arch}", target))
    return payloads


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
    environment.add_argument("--channel", help="also name this channel's signed manifest and its release")
    environment.add_argument("--tag", help="version tag for a tagged release channel")
    environment.add_argument("--version", help="app version naming the platform's symbol archive")
    update = commands.add_parser("update-payloads")
    update.add_argument("base")
    update.add_argument("directory", type=Path)
    update.add_argument("out", type=Path)
    checksums = commands.add_parser("checksums")
    checksums.add_argument("directory", type=Path)
    checksums.add_argument("--version", required=True, help="app version naming the symbol archives")
    checksums.add_argument("--without-symbols", action="store_true",
                           help="the source build has no split debug info, so no symbol archives")
    args = parser.parse_args()
    if args.command == "asset":
        print(download_assets(manifest())[args.platform][args.arch])
    elif args.command == "env":
        config = manifest()
        for key, field in [("INSTALL_SCRIPT", "install_script"), ("CHECKSUM_ASSET", "checksums"),
                           ("UPDATE_PAYLOAD_PREFIX", "update_payload_prefix")]:
            print(f"{key}={config[field]}")
        if args.channel:
            release = config["update_channels"].get(args.channel)
            if release is None:
                parser.error(f"channel {args.channel} is not published")
            tag = args.tag if release["track"] == "tagged" else release["tag"]
            if not tag:
                parser.error("a tagged channel requires --tag")
            print(f'UPDATE_MANIFEST={release["manifest"]}')
            print(f'UPDATE_MANIFEST_TAG={tag}')
        if args.platform:
            if not args.version:
                parser.error("a platform requires --version for its symbol archive")
            print(f'RELEASE_ASSET={download_assets(config)[args.platform][args.arch]}')
            if args.platform in config["assets"]:
                print(f"SYMBOLS_ASSET={symbols_archive(config, args.version, args.platform, args.arch)}")
            if args.platform == "windows":
                print(f'AUXILIARY_ASSET={config["update_assets"][args.platform][args.arch]}')
    elif args.command == "update-payloads":
        for platform, path in update_payloads(args.directory, args.out):
            print(f"{platform}={args.base}/{path.name}={path}")
    elif args.command == "install-script":
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(render_installer())
        args.output.chmod(0o755)
    else:
        checksum(args.directory, args.version, symbols=not args.without_symbols)


if __name__ == "__main__":
    main()
