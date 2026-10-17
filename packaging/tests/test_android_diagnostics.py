"""Compile the Android activities and exercise platform-independent exit diagnostics."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile
import unittest


ROOT = Path(__file__).resolve().parents[2]


class AndroidDiagnosticsTests(unittest.TestCase):
    def test_failed_exits_and_bounded_log_tails_remain_readable(self):
        """Unexpected exits produce diagnostics; normal exits and missing logs remain harmless."""
        sdk = os.environ.get("ANDROID_HOME") or os.environ.get("ANDROID_SDK_ROOT")
        if not sdk or not shutil.which("javac") or not shutil.which("java"):
            self.skipTest("missing fixture: Android SDK and Java toolchain")
        pins = json.loads((ROOT / "packaging/android/runtime.json").read_text())
        android_jar = Path(sdk) / "platforms" / f'android-{pins["target_sdk"]}' / "android.jar"
        if not android_jar.is_file():
            self.skipTest("missing fixture: pinned Android SDK platform")
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory)
            classes = output / "classes"
            classes.mkdir()
            rendered = subprocess.run([
                "bash", "-euc", '''
repo_root="$1"
out="$2"
pin() { python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$repo_root/packaging/android/runtime.json" "$1"; }
client_library=fixture_client
product=Fixture
version_code=1
version=0.0.1
source "$repo_root/packaging/android/render-template.sh"
for file in "$repo_root"/packaging/android/*.java; do
    render "$file" "$out/$(basename "$file")"
done
''', "android-java-test", str(ROOT), str(output),
            ], text=True, capture_output=True)
            self.assertEqual(rendered.returncode, 0, rendered.stderr)
            fixture = output / "DiagnosticsFixture.java"
            fixture.write_text("package " + pins["application_id"] + ";\n" + '''
import android.app.ApplicationExitInfo;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
public final class DiagnosticsFixture {
    public static void main(String[] args) throws Exception {
        for (int reason : new int[] {ApplicationExitInfo.REASON_CRASH,
                ApplicationExitInfo.REASON_CRASH_NATIVE, ApplicationExitInfo.REASON_ANR,
                ApplicationExitInfo.REASON_LOW_MEMORY}) {
            if (!StartupDiagnostics.unexpected(reason, 0)) throw new AssertionError(reason);
        }
        if (!StartupDiagnostics.unexpected(ApplicationExitInfo.REASON_EXIT_SELF, 1))
            throw new AssertionError("failed startup exit was ignored");
        for (int reason : new int[] {ApplicationExitInfo.REASON_EXIT_SELF,
                ApplicationExitInfo.REASON_USER_REQUESTED, ApplicationExitInfo.REASON_PACKAGE_UPDATED, -1}) {
            if (StartupDiagnostics.unexpected(reason, 0)) throw new AssertionError(reason);
        }
        File log = new File(args[0]);
        String input = "discarded prefix" + new String(new char[100000]).replace('\\0', 'x') + "end-marker";
        Files.write(log.toPath(), input.getBytes(StandardCharsets.UTF_8));
        String tail = StartupDiagnostics.logTail(log);
        if (!tail.endsWith("end-marker") || tail.length() >= input.length())
            throw new AssertionError("log tail lost its end or was not bounded");
        log.delete();
        if (StartupDiagnostics.logTail(log).isEmpty())
            throw new AssertionError("missing log was not explained");
    }
}
''')
            compiled = subprocess.run([
                "javac", "--release", "8", "-nowarn", "-classpath", str(android_jar),
                "-d", str(classes), *map(str, output.glob("*.java")),
            ], text=True, capture_output=True)
            self.assertEqual(compiled.returncode, 0, compiled.stderr)
            result = subprocess.run([
                "java", "-classpath", os.pathsep.join((str(classes), str(android_jar))),
                pins["application_id"] + ".DiagnosticsFixture", str(output / "client.log"),
            ], text=True, capture_output=True)
            self.assertEqual(result.returncode, 0, result.stderr)


if __name__ == "__main__":
    unittest.main()
