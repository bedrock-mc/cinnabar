#!/usr/bin/env python3
"""Hold a free build-directory lock across Cargo exec on Unix hosts."""
import fcntl
import os
import hashlib
import json
import subprocess
from pathlib import Path
import sys

from compiler_workspace_wrapper import cargo_configs, effective_workspace_wrapper, workspace_wrapper


def workspace_root(cargo: str, arguments: list[str]) -> Path:
    """Resolve the selected Cargo workspace before taking a build lock."""
    manifest = None
    metadata_options = []
    options = iter(arguments)
    for argument in options:
        if argument == "--":
            break
        if argument in {"--offline", "--locked", "--frozen"}:
            metadata_options.append(argument)
        if argument == "--manifest-path":
            manifest = next(options, None)
            if manifest is None:
                sys.exit("--manifest-path requires a value")
        elif argument.startswith("--manifest-path="):
            manifest = argument.split("=", 1)[1]
    metadata_arguments = ["metadata", "--no-deps", "--format-version=1", *metadata_options]
    if manifest is not None:
        metadata_arguments.extend(["--manifest-path", manifest])
    for override in cargo_configs(arguments):
        metadata_arguments.extend(["--config", override])
    if arguments and arguments[0].startswith("+"):
        metadata_arguments.insert(0, arguments[0])
    result = subprocess.run([cargo, *metadata_arguments], capture_output=True, text=True, check=False)
    if result.returncode:
        sys.stderr.write(result.stderr)
        sys.exit(result.returncode)
    return Path(json.loads(result.stdout)["workspace_root"]).resolve()


cargo = os.environ.get("CARGO_FREE_CARGO", "cargo")
workspace = workspace_root(cargo, sys.argv[1:])
inner = effective_workspace_wrapper(sys.argv[1:])

# fcntl.flock is also available on macOS, where the flock command is not installed.
root = Path(os.environ.get("CARGO_FREE_BUILD_ROOT", str(Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))) / "build")))
count = int(os.environ.get("CARGO_FREE_SLOTS", "3"))
if count < 1:
    sys.exit("CARGO_FREE_SLOTS must be positive")
root.mkdir(parents=True, exist_ok=True)
selected = root / "cinnabar"
for slot in range(1, count + 1):
    name = "cinnabar" if slot == 1 else f"cinnabar-{slot}"
    lock = os.open(root / f"{name}.cargo-free.lock", os.O_RDWR | os.O_CREAT, 0o600)
    try:
        fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    except BlockingIOError:
        os.close(lock)
        continue
    os.set_inheritable(lock, True)
    selected = root / name
    break
# Clippy replaces the workspace wrapper, so its intermediates need their own workspace cache.
if "clippy" in sys.argv[1:]:
    checkout = hashlib.sha256(str(workspace).encode()).hexdigest()
    selected = root / "clippy" / checkout
os.environ["CARGO_BUILD_BUILD_DIR"] = str(selected)
os.environ["RUSTC_WORKSPACE_WRAPPER"] = str(workspace_wrapper(workspace / "target/cargo-free/rustc-workspace-wrapper", inner))
os.execvp(cargo, [cargo, *sys.argv[1:]])
