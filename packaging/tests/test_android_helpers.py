"""Exercise Android helper staging without compiling the Rust client or requiring an NDK."""

import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
PINS = json.loads((ROOT / "packaging/android/runtime.json").read_text())


class AndroidHelpersTests(unittest.TestCase):
    def setUp(self):
        """Use a Go build fixture that records its target, module and linker configuration."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.output = Path(temporary.name)
        tools = self.output / "bin"
        tools.mkdir()
        go = tools / "go"
        go.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
args = sys.argv[1:]
if os.environ.get("FAIL_HELPER") == args[-1]:
    sys.exit(42)
output = pathlib.Path(args[args.index("-o") + 1])
output.write_text(json.dumps({"args": args, "cwd": os.getcwd(),
    "env": {key: os.environ.get(key) for key in
            ("CGO_ENABLED", "GOOS", "GOARCH", "CC", "GOWORK")}}))
''')
        go.chmod(0o755)
        self.env = dict(os.environ, PATH=f"{tools}{os.pathsep}{os.environ['PATH']}",
                        GOWORK=str(ROOT / "go.work"))

    def build_helpers(self):
        """Run the same build function and manifest lookup used by the APK builder."""
        return subprocess.run(["bash", "-euc", '''
repo_root="$1"
here="$repo_root/packaging/android"
libs="$2"
toolchain="$3"
triple=aarch64-linux-android
pin() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$here/runtime.json" "$1"; }
min_sdk="$(pin min_sdk)"
page_flags=-Wl,-z,max-page-size=16384
source "$here/build-go-helpers.sh"
cd "$repo_root"
build_go_helpers
''', "android-helpers-test", str(ROOT), str(self.output), str(self.output / "ndk")],
                              env=self.env, text=True, capture_output=True)

    def test_both_executable_helpers_are_staged_for_android(self):
        result = self.build_helpers()
        self.assertEqual(result.returncode, 0, result.stderr)
        for key in ("core_library", "local_server_library"):
            with self.subTest(helper=key):
                path = self.output / PINS[key]
                self.assertTrue(path.name.startswith("lib") and path.suffix == ".so")
                built = json.loads(path.read_text())
                self.assertIn("-buildmode=pie", built["args"])
                self.assertIn("-trimpath", built["args"])
                flags = built["args"][built["args"].index("-ldflags") + 1]
                self.assertIn("-extldflags=-Wl,-z,max-page-size=16384", flags)
                self.assertIn("-checklinkname=0", flags)
                self.assertEqual(built["env"]["GOOS"], "android")
                self.assertEqual(built["env"]["GOARCH"], "arm64")
                self.assertEqual(built["env"]["CGO_ENABLED"], "1")
                self.assertEqual(built["env"]["CC"],
                                 str(self.output / "ndk/bin" / f'aarch64-linux-android{PINS["min_sdk"]}-clang'))
        local = json.loads((self.output / PINS["local_server_library"]).read_text())
        self.assertEqual(local["cwd"], str(ROOT / "tools/localserver"))
        self.assertEqual(local["env"]["GOWORK"], "off")

    def test_helper_build_failure_stops_apk_packaging(self):
        for package in ("./core/cmd/bedrock-core", "."):
            with self.subTest(package=package):
                self.env["FAIL_HELPER"] = package
                result = self.build_helpers()
                self.assertEqual(result.returncode, 42, result.stderr)


if __name__ == "__main__":
    unittest.main()
