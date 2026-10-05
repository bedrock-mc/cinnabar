#!/usr/bin/python3
"""Restricted SSH receiver: publish static files and their Go server atomically."""

import fcntl
import os
from pathlib import Path
import re
import shutil
import sys
import tarfile
import tempfile


PUBLIC_FILES = {"index.html", "app.js", "downloads.js", "title.png", "texture.svg"}
FILES = PUBLIC_FILES | {"server"}
LIMIT = 16 * 1024 * 1024


def deploy(root, release, stream):
    if not re.fullmatch(r"[a-f0-9]{40}-[0-9]+-[0-9]+", release):
        raise ValueError("Invalid release identifier")
    root.mkdir(parents=True, exist_ok=True)
    with (root / ".deploy.lock").open("w") as lock:
        fcntl.flock(lock, fcntl.LOCK_EX)
        releases = root / "releases"
        releases.mkdir(exist_ok=True)
        destination = releases / release
        if destination.exists():
            raise ValueError("Release already exists")
        with tempfile.TemporaryDirectory(prefix=".incoming-", dir=releases) as temporary:
            staged = Path(temporary)
            (staged / "public").mkdir(mode=0o755)
            seen = set()
            total = 0
            with tarfile.open(fileobj=stream, mode="r|gz") as archive:
                for member in archive:
                    if member.name not in FILES or member.name in seen or not member.isfile():
                        raise ValueError("Archive contains an unexpected file")
                    total += member.size
                    if total > LIMIT:
                        raise ValueError("Website payload exceeds size limit")
                    with archive.extractfile(member) as source:
                        contents = source.read()
                    if member.name == "server" and contents[:4] != b"\x7fELF":
                        raise ValueError("Server is not a Linux executable")
                    output = staged / member.name if member.name == "server" else staged / "public" / member.name
                    output.write_bytes(contents)
                    output.chmod(0o755 if member.name == "server" else 0o644)
                    seen.add(member.name)
            if seen != FILES:
                raise ValueError("Website payload is incomplete")
            staged.chmod(0o755)
            staged.rename(destination)
            # TemporaryDirectory needs its path to remain present for cleanup.
            staged.mkdir()
        pending = root / ".current-next"
        pending.unlink(missing_ok=True)
        pending.symlink_to(Path("releases") / release)
        pending.replace(root / "current")
        for old in sorted(releases.iterdir(), key=lambda path: path.stat().st_mtime, reverse=True)[5:]:
            if old.is_dir() and not old.is_symlink():
                shutil.rmtree(old)
    print("Published " + release)


if __name__ == "__main__":
    command = os.environ.get("SSH_ORIGINAL_COMMAND", "").split()
    if len(command) != 2 or command[0] != "deploy":
        sys.exit("Only website deployments are allowed")
    try:
        deploy(Path.home() / "site", command[1], sys.stdin.buffer)
    except (ValueError, tarfile.TarError, OSError) as error:
        sys.exit(str(error))
