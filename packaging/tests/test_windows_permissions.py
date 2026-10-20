"""Check Windows permission manifests and installer launch policy."""

from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest
import xml.etree.ElementTree as ET


ROOT = Path(__file__).resolve().parents[2]
WINDOWS = ROOT / "packaging/windows"
ASM = "urn:schemas-microsoft-com:asm.v3"


class WindowsPermissionTests(unittest.TestCase):
    def test_shipped_manifest_requests_normal_invoker_permissions(self):
        """The shared manifest explicitly opts out of administrator prompts and UI access."""
        manifest = ET.parse(WINDOWS / "as-invoker.manifest")
        levels = manifest.findall(f".//{{{ASM}}}requestedExecutionLevel")
        self.assertEqual(len(levels), 1)
        self.assertEqual(levels[0].attrib, {"level": "asInvoker", "uiAccess": "false"})

    def test_installers_never_launch_or_restart_the_game(self):
        """Installer authoring leaves every game restart to the separate update helper."""
        ns = {"w": "http://wixtoolset.org/schemas/v4/wxs"}
        msi = ET.parse(WINDOWS / "cinnabar.wxs")
        restart = msi.find('.//w:Property[@Id="MSIDISABLERMRESTART"]', ns)
        self.assertIsNotNone(restart)
        self.assertEqual(restart.attrib["Value"], "1")
        for name in ("cinnabar.wxs", "setup.wxs"):
            xml = ET.parse(WINDOWS / name)
            self.assertEqual(xml.findall(".//w:CustomAction", ns), [])
            for element in xml.iter():
                self.assertFalse(any(key.startswith("LaunchTarget") for key in element.attrib))

    @unittest.skipUnless(shutil.which("pwsh"), "missing fixture: PowerShell pwsh")
    def test_manifest_checker_rejects_missing_or_unsafe_embedded_resources(self):
        """An SDK extractor fixture exercises the real packaging check, including failure paths."""
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            exe = root / "client with spaces.exe"
            exe.touch()
            embedded = exe.with_suffix(".xml")
            extractor = root / "extractor.py"
            extractor.write_text(
                f"#!{sys.executable}\n"
                "from pathlib import Path\nimport sys\n"
                "args = sys.argv[1:]\n"
                "exe = Path(next(arg.split(':', 1)[1].split(';')[0] for arg in args if arg.startswith('-inputresource:')))\n"
                "source = exe.with_suffix('.xml')\n"
                "if not source.exists(): sys.exit(1)\n"
                "out = Path(next(arg.split(':', 1)[1] for arg in args if arg.startswith('-out:')))\n"
                "out.write_bytes(source.read_bytes())\n"
            )
            if sys.platform == "win32":
                mt = root / "mt.cmd"
                mt.write_text(f'@"{sys.executable}" "{extractor}" %*\n')
            else:
                mt = extractor
                mt.chmod(0o755)
            manifest = (WINDOWS / "as-invoker.manifest").read_text()
            cases = [
                (None, False),
                (manifest, True),
                (manifest.replace("asInvoker", "requireAdministrator"), False),
                (manifest.replace('uiAccess="false"', 'uiAccess="true"'), False),
                (manifest.replace("requestedExecutionLevel", "other"), False),
                (manifest.replace('<requestedExecutionLevel level="asInvoker" uiAccess="false" />',
                                  '<requestedExecutionLevel level="asInvoker" uiAccess="false" />' * 2), False),
            ]
            for content, accepted in cases:
                with self.subTest(content=content, accepted=accepted):
                    if content is not None:
                        embedded.write_text(content)
                    result = subprocess.run(
                        ["pwsh", "-NoProfile", "-File", str(WINDOWS / "execution-level.ps1"),
                         "-Executable", str(exe), "-Mt", str(mt)],
                        capture_output=True, text=True,
                    )
                    self.assertEqual(result.returncode == 0, accepted, result.stdout + result.stderr)

    @unittest.skipUnless(sys.platform == "win32" and shutil.which("pwsh"),
                         "missing fixture: Windows host with PowerShell and SDK")
    def test_native_embedding_preserves_the_executable_and_passes_resource_check(self):
        """Read the actual PE resource after embedding into a disposable native executable."""
        with tempfile.TemporaryDirectory() as directory:
            exe = Path(directory) / "client.exe"
            shutil.copyfile(sys.executable, exe)
            subprocess.run(
                ["pwsh", "-NoProfile", "-File", str(WINDOWS / "execution-level.ps1"),
                 "-Executable", str(exe), "-Embed"],
                check=True,
            )
            subprocess.run(
                ["pwsh", "-NoProfile", "-File", str(WINDOWS / "execution-level.ps1"),
                 "-Executable", str(exe)],
                check=True,
            )


if __name__ == "__main__":
    unittest.main()
