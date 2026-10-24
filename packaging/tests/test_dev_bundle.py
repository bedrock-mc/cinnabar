"""Run the macOS development bundle with fake builds and real companion discovery."""
import os
from pathlib import Path
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class DevBundleTests(unittest.TestCase):
    def test_shaderpack_client_finds_the_compiler_beside_the_bundled_executable(self):
        """The development launch must stage the compiler that its asset step built."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for name in ["Makefile", "Cargo.toml", "assets/vanilla-source.json",
                         "packaging/common/core-ldflags.sh", "packaging/common/stage-payload.sh",
                         "packaging/macos/Info.plist.in", "packaging/macos/build-app.sh"]:
                target = root / name
                target.parent.mkdir(parents=True, exist_ok=True)
                shutil.copyfile(ROOT / name, target)
            (root / "tools/localserver").mkdir(parents=True)
            binary = root / "bin"
            binary.mkdir()
            uname = binary / "uname"
            uname.write_text("#!/bin/sh\necho Darwin\n")
            uname.chmod(0o755)
            stub = root / "build.py"
            stub.write_text('''import pathlib, sys
args=sys.argv[2:]
if sys.argv[1]=='cargo':
    profile=args[args.index('--profile')+1]
    output=pathlib.Path('target') / ('debug' if profile=='dev' else profile)
    output.mkdir(parents=True,exist_ok=True)
    if '--bin' in args:
        name=args[args.index('--bin')+1]
        (output/name).write_text('#!/bin/sh\\nexit 0\\n')
        (output/name).chmod(0o755)
        pathlib.Path('compiler-name').write_text(name)
    else:
        client=output/'cinnabar'
        client.write_text("#!/usr/bin/env python3\\nimport pathlib, sys\\n"
                          "name=pathlib.Path('compiler-name').read_text()\\n"
                          "found=pathlib.Path(__file__).with_name(name).is_file()\\n"
                          "print('compiler available' if found else 'compiler missing')\\n"
                          "sys.exit(0 if found else 47)\\n")
        client.chmod(0o755)
''')
            command = f"{shlex.quote(sys.executable)} {shlex.quote(str(stub))}"
            result = subprocess.run(
                ["make", "--no-print-directory", "play", "SHADERPACKS=1", "PROFILE=dev",
                 "CINNABAR_BUILD_COMMIT=fixture", f"CARGO={command} cargo", f"GO={command} go"],
                cwd=root, env={**os.environ, "PATH": str(binary)+os.pathsep+os.environ["PATH"]},
                text=True, capture_output=True,
            )
            self.assertEqual(result.returncode, 0, result.stdout+result.stderr)
            self.assertIn("compiler available", result.stdout)


if __name__ == '__main__':
    unittest.main()
