#!/usr/bin/env python3
"""Hold a free build-directory lock across Cargo exec on Unix hosts."""
import fcntl
import os
import hashlib
import subprocess
from pathlib import Path
import sys

from compiler_workspace_wrapper import workspace_wrapper


def worktree_root() -> Path:
    """Find the owning Git checkout, or use the invocation directory for standalone packages."""
    result = subprocess.run(["git", "rev-parse", "--show-toplevel"], capture_output=True, text=True, check=False)
    return Path(result.stdout.strip()).resolve() if result.returncode == 0 else Path.cwd().resolve()


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
# Clippy replaces the workspace wrapper, so its intermediates need their own checkout cache.
if "clippy" in sys.argv[1:]:
    checkout = hashlib.sha256(str(worktree_root()).encode()).hexdigest()
    selected = root / "clippy" / checkout
os.environ["CARGO_BUILD_BUILD_DIR"] = str(selected)
inner = os.environ.get("RUSTC_WORKSPACE_WRAPPER", os.environ.get("CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"))
os.environ["RUSTC_WORKSPACE_WRAPPER"] = str(workspace_wrapper(worktree_root() / "target/cargo-free/rustc-workspace-wrapper", inner))
cargo = os.environ.get("CARGO_FREE_CARGO", "cargo")
os.execvp(cargo, [cargo, *sys.argv[1:]])
