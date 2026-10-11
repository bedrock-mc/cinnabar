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


def configured_wrapper(path: Path, seen: frozenset[Path] = frozenset()) -> tuple[str, Path] | None:
    """Read a wrapper from a Cargo config, preserving the origin of included values."""
    import tomllib
    path = path.absolute()
    identity = path.resolve()
    if identity in seen:
        raise ValueError("Cargo config include cycle")
    with path.open("rb") as source:
        config = tomllib.load(source)
    selected = None
    for entry in config.get("include", []):
        name = entry if isinstance(entry, str) else entry["path"]
        included = path.parent / name
        if isinstance(entry, dict) and entry.get("optional") and not included.exists():
            continue
        value = configured_wrapper(included, seen | {identity})
        if value is not None:
            selected = value
    value = config.get("build", {}).get("rustc-workspace-wrapper")
    return (value, path.parent.parent) if value is not None else selected


def cargo_configs(arguments: list[str]):
    """Yield explicit Cargo config overrides before the command's argument separator."""
    options = iter(arguments)
    for argument in options:
        if argument == "--":
            break
        if argument == "--config":
            value = next(options, None)
            if value is None:
                raise ValueError("--config requires a value")
            yield value
        elif argument.startswith("--config="):
            yield argument.split("=", 1)[1]


def effective_workspace_wrapper(arguments: list[str]) -> str:
    """Resolve Cargo's wrapper precedence and relative paths before adding isolation."""
    import tomllib
    cwd = Path.cwd().resolve()
    selected = None
    home = Path(os.environ.get("CARGO_HOME", str(Path.home() / ".cargo"))).absolute()
    directories = [home, *(directory / ".cargo" for directory in reversed((cwd, *cwd.parents)))]
    for directory in directories:
        legacy = directory / "config"
        path = legacy if legacy.exists() else directory / "config.toml"
        if path.exists():
            value = configured_wrapper(path)
            if value is not None:
                selected = value
    if "CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER" in os.environ:
        selected = (os.environ["CARGO_BUILD_RUSTC_WORKSPACE_WRAPPER"], cwd)
    for override in cargo_configs(arguments):
        if "=" in override:
            config = tomllib.loads(override)
            value = config.get("build", {}).get("rustc-workspace-wrapper")
            if value is not None:
                selected = (value, cwd)
        else:
            value = configured_wrapper(Path(override))
            if value is not None:
                selected = value
    # Cargo's dedicated wrapper variable takes priority over the build config key.
    if "RUSTC_WORKSPACE_WRAPPER" in os.environ:
        selected = (os.environ["RUSTC_WORKSPACE_WRAPPER"], cwd)
    value, origin = selected or ("", cwd)
    if not isinstance(value, str):
        raise ValueError("Cargo workspace wrapper must be a string")
    if value and ("/" in value or "\\" in value):
        # Wrappers may locate their files through the invoked path, so symlinks stay unresolved.
        return str((origin / value).absolute())
    return value


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
        invoked = Path(shutil.which(inner) or inner).absolute()
        # Canonical identities detect cycles; external wrappers still run through the invoked path.
        path = invoked.resolve()
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
            return str(invoked)
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
        inner = effective_workspace_wrapper(sys.argv[3:]) if sys.argv[2] == "--cargo-config" else sys.argv[2]
        print(workspace_wrapper(Path(sys.argv[1]), inner))
    except (OSError, ValueError) as error:
        sys.exit(str(error))
