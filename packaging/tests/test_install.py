"""Run the Linux installer offline against release/download/runtime fixtures."""

import hashlib
import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import time
import unittest


ROOT = Path(__file__).resolve().parents[2]
CONFIG = json.loads((ROOT / "packaging/release-assets.json").read_text())
ASSET = CONFIG["assets"]["linux"]["x86_64"]
BASE = f'https://github.com/{CONFIG["repository"]}/releases'

# Serves the release fixtures as either curl or wget, logging each request.
DOWNLOADER = '''
import json
import os
from pathlib import Path
import shutil
import sys
tool = Path(sys.argv[0]).name
args = sys.argv[1:]
root = Path(os.environ['FIXTURE_ROOT'])
with (root / 'requests').open('a') as log:
    log.write(json.dumps([tool] + args) + '\\n')
base = os.environ['FIXTURE_BASE']
tag = os.environ['FIXTURE_TAG']
url = args[-1]
failed = 22 if tool == 'curl' else 8
if url == base + '/latest':
    if os.environ.get('FIXTURE_NO_RELEASE'):
        sys.stderr.write('HTTP/1.1 404 Not Found\\n')
        sys.exit(failed)
    if tool == 'curl':
        print(base + '/tag/' + tag, end='')
        sys.exit(0)
    assert '--max-redirect=0' in args and '--spider' in args
    sys.stderr.write('  HTTP/1.1 302 Found\\r\\n  Location: ' + base + '/tag/' + tag + '\\r\\n')
    sys.exit(failed)
prefix = base + '/download/' + tag + '/'
if not url.startswith(prefix):
    sys.exit(failed)
asset = root / 'release' / tag / url[len(prefix):]
if not asset.is_file():
    asset = root / 'release' / url[len(prefix):]
if not asset.is_file() or asset.name == os.environ.get('FIXTURE_FAIL_ASSET'):
    sys.exit(failed)
if tool == 'curl':
    output = args[args.index('--output') + 1]
else:
    output = next(a for a in args if a.startswith('--output-document='))[len('--output-document='):]
shutil.copyfile(asset, output)
'''

# Real tools the installer and fixtures may use; curl and wget are deliberately absent.
SYSTEM_TOOLS = ("sh", "awk", "sed", "mktemp", "sha256sum", "shasum", "cp", "mv", "rm", "chmod",
                "mkdir", "cat", "ln", "readlink", "dirname", "tr", "head", "env", "pwd", "sleep", "find")

# Holds the run given FIXTURE_PAUSE_DIR just before it repoints app/current.
PAUSING_MV = '''
import os
from pathlib import Path
import sys
import time
pause = os.environ.get('FIXTURE_PAUSE_DIR')
if pause and sys.argv[-1].endswith('/cinnabar/app/current'):
    Path(pause, 'paused').touch()
    while not Path(pause, 'resume').exists():
        time.sleep(0.05)
real = os.environ['FIXTURE_REAL_MV']
os.execv(real, [real] + sys.argv[1:])
'''

APPIMAGE = '''#!/bin/sh
if [ "$1" = --appimage-extract ]; then
    printf 'extract %s\\n' "$PWD" >> "$FIXTURE_ROOT/runtime"
    [ "${FIXTURE_EXTRACT_FAIL:-}" != 1 ] || exit 17
    mkdir -p squashfs-root
    printf 'icon' > squashfs-root/cinnabar.png
    printf '#!/bin/sh\\nprintf "%%s\\\\n" "$@" > "$FIXTURE_ROOT/launch"\\n' > squashfs-root/AppRun
    chmod 755 squashfs-root/AppRun
else
    printf 'mounted\\n' >> "$FIXTURE_ROOT/runtime"
fi
'''


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.script = self.root / "install.sh"
        subprocess.run([sys.executable, str(ROOT / "packaging/release-downloads.py"),
                        "install-script", "--output", str(self.script)], check=True)
        self.home = self.root / "a home with ' $ and % \\ and \""
        self.data = self.home / "custom data"
        self.app = self.data / "cinnabar/app"
        self.bin = self.home / "custom bin"
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.system = self.root / "system"
        self.system.mkdir()
        for name in SYSTEM_TOOLS:
            found = shutil.which(name)
            if found:
                (self.system / name).symlink_to(found)
        self.release = self.root / "release"
        self.release.mkdir()
        self.image = self.release / ASSET
        self.image.write_text(APPIMAGE)
        self.write_checksums()
        self.add_tool("curl", f"#!{sys.executable}\n{DOWNLOADER}")
        self.add_tool("uname", """#!/bin/sh
case "$1" in
  -s) printf '%s\\n' "${FIXTURE_OS:-Linux}" ;;
  -m) printf '%s\\n' "${FIXTURE_ARCH:-x86_64}" ;;
  *) exit 1 ;;
esac
""")
        self.env = dict(os.environ, HOME=str(self.home), XDG_DATA_HOME=str(self.data),
                        CINNABAR_BIN_DIR=str(self.bin), FIXTURE_ROOT=str(self.root),
                        FIXTURE_BASE=BASE, FIXTURE_TAG="v1.2.3",
                        PATH=f"{self.tools}:{self.system}")
        self.env.pop("APPIMAGE_EXTRACT_AND_RUN", None)

    def add_tool(self, name, contents):
        path = self.tools / name
        path.write_text(contents)
        path.chmod(0o755)

    def write_checksums(self, digest=None, copies=1):
        digest = digest or self.digest()
        (self.release / CONFIG["checksums"]).write_text(f"{digest}  {ASSET}\n" * copies)

    def digest(self):
        return hashlib.sha256(self.image.read_bytes()).hexdigest()

    def install(self, *args, **env):
        return subprocess.run(["sh", str(self.script), *args], env=dict(self.env, **env),
                              capture_output=True, text=True)

    def launch(self, *args):
        return subprocess.run([str(self.bin / "cinnabar"), *args], env=self.env,
                              capture_output=True, text=True)

    def request_log(self):
        path = self.root / "requests"
        return [json.loads(line) for line in path.read_text().splitlines()] if path.exists() else []

    def requests(self):
        return [entry[-1] for entry in self.request_log()]

    def runtime(self):
        path = self.root / "runtime"
        return path.read_text().splitlines() if path.exists() else []

    def builds(self):
        return sorted(path.name for path in self.app.iterdir()
                      if path.is_dir() and not path.name.startswith("."))

    def preserve_existing(self):
        image = self.app / "Cinnabar.AppImage"
        image.parent.mkdir(parents=True)
        image.write_bytes(b"previous install")
        data = self.data / "cinnabar/worlds/keep"
        data.parent.mkdir(parents=True)
        data.write_bytes(b"game data")
        return image, data

    def test_stable_install_resolves_one_tag_and_handles_quoted_paths(self):
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(self.requests(), [BASE + "/latest",
                         f'{BASE}/download/v1.2.3/{CONFIG["checksums"]}',
                         f"{BASE}/download/v1.2.3/{ASSET}"])
        self.assertEqual((self.data / "icons/hicolor/256x256/apps/cinnabar.png").read_text(), "icon")
        desktop = (self.data / "applications/cinnabar.desktop").read_text()
        self.assertIn('Exec="', desktop)
        self.assertIn(r'\\$ and %% \\\\ and \\"', desktop)
        launched = self.launch("--argument", "two words")
        self.assertEqual(launched.returncode, 0, launched.stderr)
        self.assertEqual((self.root / "launch").read_text(), "--argument\ntwo words\n")
        self.assertFalse(list(self.data.rglob(".install.*")))

    def test_unpacks_once_into_a_versioned_folder_on_the_data_filesystem(self):
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        build = f"v1.2.3-{self.digest()[:12]}"
        self.assertEqual((self.app / "current").read_text(), build + "\n")
        self.assertEqual(self.builds(), [build])
        self.assertFalse(list(self.app.rglob("*.AppImage")))
        [extract] = self.runtime()
        self.assertTrue(extract.startswith(f"extract {self.app}/.install."), extract)
        for _ in range(2):
            self.assertEqual(self.launch().returncode, 0)
        self.assertEqual(self.runtime(), [extract])

    def test_wget_is_used_when_curl_is_missing(self):
        (self.tools / "curl").unlink()
        self.add_tool("wget", f"#!{sys.executable}\n{DOWNLOADER}")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({entry[0] for entry in self.request_log()}, {"wget"})
        self.assertEqual(self.requests(), [BASE + "/latest",
                         f'{BASE}/download/v1.2.3/{CONFIG["checksums"]}',
                         f"{BASE}/download/v1.2.3/{ASSET}"])
        self.assertEqual((self.app / "current").read_text(), f"v1.2.3-{self.digest()[:12]}\n")
        self.image.write_text(APPIMAGE + "# tampered\n")
        tampered = self.install("--version", "v1.2.3")
        self.assertIn("SHA-256 mismatch", tampered.stderr)
        no_release = self.install(FIXTURE_NO_RELEASE="1")
        self.assertIn("No stable release is available", no_release.stderr)

    def test_curl_is_preferred_over_wget(self):
        self.add_tool("wget", f"#!{sys.executable}\n{DOWNLOADER}")
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual({entry[0] for entry in self.request_log()}, {"curl"})

    def test_missing_downloaders_name_both_before_network(self):
        (self.tools / "curl").unlink()
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("Install curl or wget", result.stderr)
        self.assertEqual(self.requests(), [])
        self.assertFalse(self.data.exists())

    def test_updates_keep_the_replaced_build_and_prune_older_ones(self):
        _, data = self.preserve_existing()
        builds = []
        for tag in ("v1.2.3", "v1.2.4", "v1.2.5"):
            self.image.write_text(APPIMAGE + f"# {tag}\n")
            self.write_checksums()
            result = self.install(FIXTURE_TAG=tag)
            self.assertEqual(result.returncode, 0, result.stderr)
            builds.append(f"{tag}-{self.digest()[:12]}")
            self.assertEqual((self.app / "current").read_text(), builds[-1] + "\n")
            self.assertEqual(self.builds(), sorted(builds[-2:]))
            self.assertFalse((self.app / "Cinnabar.AppImage").exists())
        self.assertEqual(data.read_bytes(), b"game data")
        self.assertEqual(self.launch("hello").returncode, 0)
        self.assertEqual((self.root / "launch").read_text(), "hello\n")

    def test_reinstalling_a_build_reuses_it_and_a_rebuilt_tag_unpacks_again(self):
        for _ in range(2):
            self.assertEqual(self.install("--channel", "nightly", FIXTURE_TAG="nightly").returncode, 0)
        self.assertEqual(len(self.runtime()), 1)
        self.image.write_text(APPIMAGE + "# rebuilt\n")
        self.write_checksums()
        self.assertEqual(self.install("--channel", "nightly", FIXTURE_TAG="nightly").returncode, 0)
        self.assertEqual(len(self.runtime()), 2)
        self.assertEqual((self.app / "current").read_text(), f"nightly-{self.digest()[:12]}\n")
        self.assertEqual(len(self.builds()), 2)

    def test_reinstall_repairs_a_build_that_lost_its_app_run(self):
        self.assertEqual(self.install().returncode, 0)
        build = f"v1.2.3-{self.digest()[:12]}"
        (self.app / build / "AppRun").unlink()
        self.assertNotEqual(self.launch().returncode, 0)
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(len(self.runtime()), 2)
        self.assertEqual(self.builds(), [build])
        self.assertEqual(self.launch("again").returncode, 0)
        self.assertEqual((self.root / "launch").read_text(), "again\n")

    def test_reinstalling_the_current_build_keeps_the_replaced_one(self):
        builds = []
        for tag in ("v1.2.3", "v1.2.4", "v1.2.4"):
            self.image.write_text(APPIMAGE + f"# {tag}\n")
            self.write_checksums()
            result = self.install(FIXTURE_TAG=tag)
            self.assertEqual(result.returncode, 0, result.stderr)
            builds.append(f"{tag}-{self.digest()[:12]}")
        self.assertEqual(self.builds(), sorted(set(builds)))
        self.assertEqual((self.app / "previous").read_text(), builds[0] + "\n")

    def tagged_release(self, tag):
        image = self.release / tag / ASSET
        image.parent.mkdir()
        image.write_text(APPIMAGE + f"# {tag}\n")
        digest = hashlib.sha256(image.read_bytes()).hexdigest()
        (image.parent / CONFIG["checksums"]).write_text(f"{digest}  {ASSET}\n")
        return f"{tag}-{digest[:12]}"

    def wait_for(self, condition, what):
        for _ in range(600):
            if condition():
                return
            time.sleep(0.05)
        self.fail(f"timed out waiting for {what}")

    def overlapping_installs(self, flock):
        """A paused run must not prune the build an overlapping run publishes, or vice versa."""
        first = self.tagged_release("v1.2.3")
        self.assertEqual(self.install("--version", "v1.2.3").returncode, 0)
        paused, newest = self.tagged_release("v1.2.4"), self.tagged_release("v1.2.5")
        self.add_tool("mv", f"#!{sys.executable}\n{PAUSING_MV}")
        pause = self.root / "pause"
        pause.mkdir()
        env = dict(self.env, FIXTURE_REAL_MV=shutil.which("mv"))
        logs = [self.root / "first.log", self.root / "second.log"]
        outputs = [log.open("w") for log in logs]
        runs = []
        try:
            runs.append(subprocess.Popen(
                ["sh", str(self.script), "--version", "v1.2.4"], stdout=outputs[0],
                stderr=subprocess.STDOUT,
                env=dict(env, FIXTURE_TAG="v1.2.4", FIXTURE_PAUSE_DIR=str(pause))))
            self.wait_for(lambda: (pause / "paused").exists() or runs[0].poll() is not None,
                          "the first install to publish")
            self.assertTrue((pause / "paused").exists(), logs[0].read_text())
            runs.append(subprocess.Popen(
                ["sh", str(self.script), "--version", "v1.2.5"], stdout=outputs[1],
                stderr=subprocess.STDOUT, env=dict(env, FIXTURE_TAG="v1.2.5")))
            if flock:
                time.sleep(2)  # flock waits silently
            else:
                self.wait_for(lambda: runs[1].poll() is not None
                              or "Waiting" in logs[1].read_text(),
                              "the second install to reach the lock")
        finally:
            (pause / "resume").touch()
            for run in runs:
                run.wait(timeout=60)
            for output in outputs:
                output.close()
        for run, log in zip(runs, logs):
            self.assertEqual(run.returncode, 0, log.read_text())
        self.assertEqual((self.app / "current").read_text(), newest + "\n")
        self.assertEqual((self.app / "previous").read_text(), paused + "\n")
        self.assertEqual(self.builds(), sorted([paused, newest]))
        self.assertNotIn(first, self.builds())
        self.assertEqual(self.launch().returncode, 0)
        self.assertFalse((self.app / ".lock.pid").is_symlink())
        self.assertEqual((self.app / ".lock").exists(), flock)

    def test_overlapping_installs_serialize_without_flock(self):
        self.overlapping_installs(flock=False)

    def test_overlapping_installs_serialize_with_flock(self):
        flock = shutil.which("flock")
        if flock:
            (self.system / "flock").symlink_to(flock)
        else:
            # Same semantics as util-linux `flock FD`: the lock follows the inherited descriptor.
            self.add_tool("flock", f"#!{sys.executable}\nimport fcntl, sys\n"
                                   "fcntl.flock(int(sys.argv[1]), fcntl.LOCK_EX)\n")
        self.overlapping_installs(flock=True)

    def dead_lock(self, age):
        dead = subprocess.Popen(["true"])
        dead.wait()
        self.app.mkdir(parents=True)
        lock = self.app / ".lock.pid"
        lock.symlink_to(str(dead.pid))
        stamp = time.time() - age
        os.utime(lock, (stamp, stamp), follow_symlinks=False)
        return lock

    def test_a_crashed_install_lock_is_reclaimed_after_ten_minutes(self):
        lock = self.dead_lock(age=11 * 60)
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(lock.is_symlink())
        self.assertFalse(list(self.app.glob(".lock.stale.*")))

    def test_a_recent_lock_with_a_dead_owner_is_not_reclaimed(self):
        lock = self.dead_lock(age=60)
        with self.assertRaises(subprocess.TimeoutExpired) as waited:
            subprocess.run(["sh", str(self.script)], env=self.env, capture_output=True, timeout=5)
        self.assertIn(b"Waiting for another Cinnabar install", waited.exception.stdout or b"")
        self.assertTrue(lock.is_symlink())
        self.assertFalse((self.app / "current").exists())

    def test_bin_dir_overlapping_the_app_folder_is_rejected_before_changes(self):
        for bin_dir in (self.app, self.app / "bin", self.data):
            with self.subTest(bin_dir=bin_dir):
                result = self.install(CINNABAR_BIN_DIR=str(bin_dir) + "/")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("CINNABAR_BIN_DIR must not", result.stderr)
                self.assertEqual(self.requests(), [])
                self.assertFalse(self.data.exists())

    def test_pruning_removes_only_obsolete_builds_and_the_legacy_image(self):
        image, _ = self.preserve_existing()
        keep = [self.app / "notes", self.app / "custom-0123456789ab.txt"]
        keep[0].mkdir()
        keep[1].write_text("user file")
        stale = self.app / "v0.0.1-0123456789ab"
        stale.mkdir()
        result = self.install()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertFalse(stale.exists())
        self.assertFalse(image.exists())
        for path in keep:
            self.assertTrue(path.exists(), path)

    def test_launcher_reports_an_incomplete_install(self):
        self.assertEqual(self.install().returncode, 0)
        (self.app / "current").unlink()
        launched = self.launch()
        self.assertNotEqual(launched.returncode, 0)
        self.assertIn("run the installer again", launched.stderr)

    def test_checksum_mismatch_preserves_app_and_data_without_executing_download(self):
        previous, data = self.preserve_existing()
        self.image.write_text(APPIMAGE + "# tampered\n")
        result = self.install()
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("SHA-256 mismatch", result.stderr)
        self.assertEqual(previous.read_bytes(), b"previous install")
        self.assertEqual(data.read_bytes(), b"game data")
        self.assertEqual(self.runtime(), [])
        self.assertFalse(self.bin.exists())

    def test_missing_or_duplicate_checksum_never_downloads_or_replaces_app(self):
        previous, _ = self.preserve_existing()
        for copies in (0, 2):
            with self.subTest(copies=copies):
                self.write_checksums(copies=copies)
                result = self.install("--version", "v1.2.3")
                self.assertNotEqual(result.returncode, 0)
                self.assertIn("no unique valid checksum", result.stderr)
                self.assertEqual(previous.read_bytes(), b"previous install")
                self.assertNotIn(f"{BASE}/download/v1.2.3/{ASSET}", self.requests())

    def test_failed_download_and_extraction_preserve_existing_install(self):
        previous, _ = self.preserve_existing()
        for failure in ({"FIXTURE_FAIL_ASSET": ASSET}, {"FIXTURE_EXTRACT_FAIL": "1"}):
            with self.subTest(failure=failure):
                result = self.install(**failure)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(previous.read_bytes(), b"previous install")
                self.assertFalse((self.bin / "cinnabar").exists())
                self.assertFalse((self.app / "current").exists())
                self.assertEqual(self.builds(), [])

    def test_unsupported_arch_and_invalid_tag_fail_before_network(self):
        for args, env in (((), {"FIXTURE_ARCH": "aarch64"}),
                          (("--version", "../other"), {})):
            with self.subTest(args=args, env=env):
                result = self.install(*args, **env)
                self.assertNotEqual(result.returncode, 0)
                self.assertEqual(self.requests(), [])

    def test_macos_points_to_native_installer_without_network(self):
        result = self.install(FIXTURE_OS="Darwin")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("install the DMG", result.stderr)
        self.assertEqual(self.requests(), [])

    def test_no_stable_release_returns_useful_error(self):
        result = self.install(FIXTURE_NO_RELEASE="1")
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("No stable release is available", result.stderr)
        self.assertFalse(self.data.exists())

    def test_nightly_and_exact_tag_do_not_query_latest(self):
        for args, tag in ((("--channel", "nightly"), "nightly"),
                          (("--version", "v2.0.1"), "v2.0.1")):
            with self.subTest(args=args):
                result = self.install(*args, FIXTURE_TAG=tag)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertNotIn(BASE + "/latest", self.requests())
                self.assertIn(f"{BASE}/download/{tag}/{ASSET}", self.requests())


if __name__ == "__main__":
    unittest.main()
