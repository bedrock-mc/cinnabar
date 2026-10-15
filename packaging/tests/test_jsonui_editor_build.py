"""Check the editor build's target-specific Cargo environment through its CLI."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import tomllib
import unittest


SOURCE = Path(__file__).resolve().parents[2]
BUILD = SOURCE / "tools/jsonui-editor/build.sh"


class EditorBuildTests(unittest.TestCase):
    def test_wasm_release_disables_native_split_debug_info(self):
        """Native packaging overrides must not reach the WebAssembly compiler."""
        lock = tomllib.loads((SOURCE / "Cargo.lock").read_text())
        version = next(package["version"] for package in lock["package"]
                       if package["name"] == "wasm-bindgen")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            binary = root / "bin"
            binary.mkdir()
            cargo = binary / "cargo"
            cargo.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
pathlib.Path(os.environ["EDITOR_CARGO_LOG"]).write_text(json.dumps({
    "args": sys.argv[1:],
    "split": os.environ.get("CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO"),
}))
''')
            bindgen = binary / "wasm-bindgen"
            bindgen.write_text(f'''#!/usr/bin/env python3
import pathlib, sys
if "--version" in sys.argv:
    print("wasm-bindgen {version}")
else:
    out = pathlib.Path(sys.argv[sys.argv.index("--out-dir") + 1])
    (out / "jsonui_editor.js").touch()
    (out / "jsonui_editor_bg.wasm").touch()
''')
            for executable in [cargo, bindgen]:
                executable.chmod(0o755)
            log = root / "cargo.json"
            env = dict(os.environ, PATH=str(binary) + os.pathsep + os.environ["PATH"],
                       CARGO=str(cargo), EDITOR_CARGO_LOG=str(log),
                       CARGO_PROFILE_RELEASE_SPLIT_DEBUGINFO="packed")
            result = subprocess.run(["bash", str(BUILD), str(root / "site")], env=env,
                                    text=True, capture_output=True, check=False)
            self.assertEqual(result.returncode, 0, result.stderr)
            invocation = json.loads(log.read_text())
            self.assertIn("--release", invocation["args"])
            self.assertEqual(invocation["args"][invocation["args"].index("--target") + 1],
                             "wasm32-unknown-unknown")
            self.assertEqual(invocation["split"], "off")
