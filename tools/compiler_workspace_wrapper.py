#!/usr/bin/env python3
"""Create checkout-specific compiler wrappers with a file-owned forwarding target."""
import hashlib
import json
import os
from pathlib import Path
import shutil
import sys
import tempfile

MARKER = "cinnabar-workspace-wrapper: "
DIGEST_LENGTH = 16


def owned_path(path: Path) -> bool:
    """Recognize reserved wrapper filenames without rejecting custom external wrappers."""
    stem, separator, digest = path.stem.rpartition("-")
    generated = bool(separator) and len(digest) == DIGEST_LENGTH and all(
        char in "0123456789abcdef" for char in digest
    )
    if path.parent.name == "devtool":
        return path.name in {"rustc.py", "rustc.cmd"} or (
            generated and stem == "rustc" and path.suffix in {".py", ".cmd"}
        )
    return path.parent.name == "cargo-free" and (
        path.name == "rustc-workspace-wrapper" or (
            generated and stem == "rustc-workspace-wrapper" and not path.suffix
        )
    )


def real_inner(inner: str | None) -> str:
    """Unwrap our recorded targets, rejecting cycles and obsolete wrappers."""
    seen = set()
    while inner:
        path = Path(shutil.which(inner) or inner).resolve()
        if path in seen:
            raise ValueError("compiler wrapper cycle")
        seen.add(path)
        owned = owned_path(path)
        try:
            with path.open("rb") as source:
                lines = [source.readline(4096), source.readline(4096)]
        except FileNotFoundError:
            if owned:
                raise ValueError("missing compiler wrapper record") from None
            return inner  # External wrappers may be commands resolved through PATH.
        marker = MARKER.encode()
        record = next((line.split(marker, 1)[1] for line in lines
                       if line.startswith((b"# " + marker, b"@rem " + marker))), None)
        if record is None:
            if owned:
                raise ValueError("obsolete compiler wrapper: remove it and retry")
            return str(path)
        inner = json.loads(record)
        if not isinstance(inner, str):
            raise ValueError("invalid compiler wrapper target")
    return ""


def workspace_wrapper(wrapper: Path, inner: str | None) -> Path:
    """Write an immutable wrapper for the real inner target and return its path."""
    inner = real_inner(inner)
    encoded = json.dumps(inner, ensure_ascii=True)
    # Different forwarding targets cannot rewrite a wrapper another invocation is using.
    digest = hashlib.sha256(encoded.encode()).hexdigest()[:DIGEST_LENGTH]
    wrapper = wrapper.with_name(wrapper.stem + "-" + digest + wrapper.suffix)
    if wrapper.suffix == ".cmd":
        prefix = '"' + inner.replace("%", "%%") + '" ' if inner else ""
        script = (
            "@rem " + MARKER + encoded + "\r\n@setlocal DisableDelayedExpansion\r\n@"
            + prefix + "%*\r\n@exit /b %errorlevel%\r\n"
        )
    else:
        script = (
            "#!/usr/bin/env python3\n# " + MARKER + encoded + "\n"
            "import json, os, sys\n"
            f"inner = json.loads({encoded!r})\n"
            "command = ([inner] if inner else []) + sys.argv[1:]\n"
            "os.execvp(command[0], command)\n"
        )
    wrapper.parent.mkdir(parents=True, exist_ok=True)
    if not wrapper.exists():
        descriptor, name = tempfile.mkstemp(dir=wrapper.parent)
        temporary = Path(name)
        try:
            with os.fdopen(descriptor, "w", newline="") as output:
                output.write(script)
            temporary.chmod(0o700)
            os.replace(temporary, wrapper)
        finally:
            temporary.unlink(missing_ok=True)
    return wrapper


if __name__ == "__main__":
    try:
        print(workspace_wrapper(Path(sys.argv[1]), sys.argv[2]))
    except (OSError, ValueError) as error:
        sys.exit(str(error))
