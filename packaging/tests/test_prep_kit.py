"""Stage the preparation kit as the packagers do and check it against the generated list."""

from pathlib import Path
import os
import shutil
import subprocess
import sys
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]
LISTED = [line for line in (ROOT / "packaging/common/prep-kit-files.txt").read_text().splitlines() if line]
STAGE = ROOT / "packaging/common/stage-payload.sh"
HOST = {"darwin": "macos", "linux": "linux"}.get(sys.platform)
FOREIGN = "linux" if HOST == "macos" else "macos"


def files_below(root):
    return sorted(path.relative_to(root).as_posix() for path in root.rglob("*") if path.is_file())


def bash(script, *args, env=None):
    return subprocess.run(
        ["bash", "-c", f'source "{STAGE}"; {script}', "bash", *args],
        env={**os.environ, "CINNABAR_SOURCE_ROOT": str(ROOT), **(env or {})},
        capture_output=True,
        text=True,
    )


class PrepKitTests(unittest.TestCase):
    def setUp(self):
        self.temp = Path(tempfile.mkdtemp())
        self.addCleanup(shutil.rmtree, self.temp)

    def compiler(self, name, script):
        path = self.temp / "bin" / name
        path.parent.mkdir(exist_ok=True)
        path.write_text(script)
        path.chmod(0o755)
        return path

    def test_the_list_names_unique_tracked_files_in_byte_order(self):
        self.assertEqual(LISTED, sorted(set(LISTED), key=str.encode))
        for file in LISTED:
            self.assertTrue((ROOT / file).is_file(), file)

    def test_unix_staging_ships_the_compiler_and_exactly_the_listed_files(self):
        kit = self.temp / "kit"
        result = bash('stage_prep_kit "$1" "$2"', str(kit), str(self.compiler("assetc", "")))
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(files_below(kit), sorted(["bin/assetc", *LISTED]))
        for file in LISTED:
            self.assertEqual((kit / file).read_bytes(), (ROOT / file).read_bytes(), file)
        manifest = bash("resource_manifest linux").stdout.splitlines()
        self.assertEqual(
            sorted(line for line in manifest if line.startswith("prep-kit/")),
            sorted(f"prep-kit/{file}" for file in ["bin/assetc", *LISTED]),
        )

    @unittest.skipUnless(shutil.which("pwsh"), "pwsh is required for the Windows staging script")
    def test_windows_staging_matches_unix_staging(self):
        kit = self.temp / "kit"
        assetc = self.compiler("assetc.exe", "")
        subprocess.run(
            ["pwsh", "-NoProfile", "-File", str(ROOT / "packaging/windows/stage-prep-kit.ps1"),
             "-Root", str(ROOT), "-Kit", str(kit), "-Assetc", str(assetc)],
            check=True,
        )
        self.assertEqual(files_below(kit), sorted(["bin/assetc.exe", *LISTED]))

    def payload(self, platform, compiler_script):
        root = self.temp / platform
        if platform == "linux":
            entries = ["AppRun", "cinnabar.desktop", "cinnabar.png"]
            entries += [f"usr/bin/{name}" for name in ["cinnabar", "bedrock-core", "bedrock-local-server"]]
            resources = root / "usr/share/cinnabar"
        else:
            contents = "Cinnabar.app/Contents/"
            entries = ["Applications", f"{contents}Info.plist", f"{contents}Resources/Cinnabar.icns"]
            entries += [f"{contents}MacOS/{name}" for name in ["cinnabar", "bedrock-core", "bedrock-local-server"]]
            resources = root / contents / "Resources"
        for entry in entries:
            (root / entry).parent.mkdir(parents=True, exist_ok=True)
            (root / entry).write_text("")
        result = bash('stage_resources "$1" "$2"', str(resources), str(self.compiler("assetc", compiler_script)))
        self.assertEqual(result.returncode, 0, result.stderr)
        return root, resources / "prep-kit"

    def check_payload(self, platform, root):
        return subprocess.run(
            ["bash", str(ROOT / "packaging/check-payload.sh"), platform, str(root)],
            env={**os.environ, "CINNABAR_SOURCE_ROOT": str(ROOT), "CINNABAR_UPDATE_URL": ""},
            capture_output=True,
            text=True,
        )

    @unittest.skipUnless(HOST, "the payload check runs shipped compilers on macOS and Linux hosts")
    def test_payload_check_plans_a_first_run_with_the_shipped_compiler(self):
        # Stands in for assetc: the plan fails unless the kit it is handed holds every listed file.
        script = (
            '#!/bin/sh\n[ "$1" = --help ] && exit 0\n'
            'kit="$3"\nwhile IFS= read -r file; do\n'
            '  [ -f "$kit/$file" ] || { echo "read $kit/$file: missing" >&2; exit 1; }\n'
            f'done < "{ROOT}/packaging/common/prep-kit-files.txt"\n'
            'echo \'{"current":false}\'\n'
        )
        root, kit = self.payload(HOST, script)
        result = self.check_payload(HOST, root)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("prep kit satisfies its compiler", result.stdout)
        (kit / LISTED[-1]).unlink()
        result = self.check_payload(HOST, root)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(f"cannot plan from the {HOST} prep kit", result.stderr)

    @unittest.skipUnless(HOST, "the payload check runs shipped compilers on macOS and Linux hosts")
    def test_only_foreign_artifacts_and_other_architecture_dmgs_skip_the_plan(self):
        unrunnable = "#!/bin/sh\nexit 126\n"
        root, _ = self.payload(FOREIGN, unrunnable)
        result = self.check_payload(FOREIGN, root)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn(f"skipping the {FOREIGN} prep-kit plan", result.stdout)
        root, _ = self.payload(HOST, unrunnable)
        result = self.check_payload(HOST, root)
        if HOST == "macos":
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("skipping the macos prep-kit plan", result.stdout)
        else:
            self.assertNotEqual(result.returncode, 0)
            self.assertIn("cannot plan from the linux prep kit", result.stderr)


if __name__ == "__main__":
    unittest.main()
