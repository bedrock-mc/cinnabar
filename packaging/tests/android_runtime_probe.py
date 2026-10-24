"""Run an original Linux guest in an ordinary Android application sandbox."""

import json
import os
from pathlib import Path
import shutil
import subprocess
import zipfile


def run_app_probe(directory, guest, libraries, serial, runtime):
    """Build a disposable instrumentation APK, execute the guest, and remove the APK."""
    sdk = Path(os.environ['ANDROID_HOME'])
    build_tools = sdk / 'build-tools' / runtime['build_tools']
    android_jar = sdk / 'platforms' / f"android-{runtime['target_sdk']}" / 'android.jar'
    application = runtime['application_id'] + '.runtimeprobe'
    native = runtime['bds_translation']['native_runtime']['libraries']
    loader = next(item['packaged'] for item in native if item.get('loader'))
    preload = ', '.join(json.dumps(item['packaged']) for item in native if not item.get('loader'))
    source = directory / 'Probe.java'
    source.write_text('''package runtimeprobe;
import android.app.Instrumentation;
import android.os.Bundle;
import java.io.*;
import java.util.*;
import java.util.concurrent.TimeUnit;

/** Executes the runtime under the target application's UID and SELinux policy. */
public final class Probe extends Instrumentation {
    @Override public void onCreate(Bundle arguments) { super.onCreate(arguments); start(); }
    @Override public void onStart() {
        Bundle result = new Bundle();
        try {
            File root = getTargetContext().getFilesDir();
            File guest = new File(root, "guest");
            try (InputStream input = getTargetContext().getAssets().open("guest");
                 OutputStream output = new FileOutputStream(guest)) {
                byte[] bytes = new byte[8192];
                int count;
                while ((count = input.read(bytes)) != -1) output.write(bytes, 0, count);
            }
            if (!guest.setExecutable(true, true)) throw new IOException("Cannot mark the guest executable");
            String directory = getTargetContext().getApplicationInfo().nativeLibraryDir;
            String[] libraries = { @PRELOAD@ };
            for (int i = 0; i < libraries.length; ++i) libraries[i] = directory + "/" + libraries[i];
            List<String> command = Arrays.asList(directory + "/@LOADER@", "--inhibit-cache",
                "--library-path", directory, "--preload", String.join(":", libraries),
                directory + "/@HELPER@", guest.getAbsolutePath(), new File(root, "save").getAbsolutePath());
            File log = new File(root, "output");
            Process process = new ProcessBuilder(command).directory(root).redirectErrorStream(true)
                .redirectOutput(log).start();
            if (!process.waitFor(30, TimeUnit.SECONDS)) {
                process.destroyForcibly();
                throw new IOException("Linux guest did not finish within the diagnostic deadline");
            }
            StringBuilder output = new StringBuilder();
            try (BufferedReader lines = new BufferedReader(new FileReader(log))) {
                String line;
                while ((line = lines.readLine()) != null) output.append(line).append('\\n');
            }
            result.putString("stream", output.toString());
            result.putInt("guest_exit", process.exitValue());
            finish(process.exitValue(), result);
        } catch (Exception error) {
            result.putString("stream", error.toString());
            finish(1, result);
        }
    }
}
'''.replace('@PRELOAD@', preload).replace('@LOADER@', loader)
                      .replace('@HELPER@', runtime['bds_translation']['helper_library']))
    manifest = directory / 'AndroidManifest.xml'
    manifest.write_text(f'''<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="{application}">
<uses-sdk android:minSdkVersion="{runtime['min_sdk']}" android:targetSdkVersion="{runtime['target_sdk']}"/>
<uses-permission android:name="android.permission.INTERNET"/>
<application android:debuggable="true" android:extractNativeLibs="true" android:allowBackup="false"/>
<instrumentation android:name="runtimeprobe.Probe" android:targetPackage="{application}"/>
</manifest>''')
    classes, dex = directory / 'classes', directory / 'dex'
    classes.mkdir()
    dex.mkdir()

    def run(command):
        """Bound fixture setup and retain diagnostics on a failed tool invocation."""
        return subprocess.run([str(arg) for arg in command], check=True, capture_output=True,
                              text=True, timeout=60)

    run(['javac', '--release', '8', '-classpath', android_jar, '-d', classes, source])
    run([build_tools / 'd8', '--min-api', str(runtime['min_sdk']), '--lib', android_jar,
         '--output', dex, classes / 'runtimeprobe/Probe.class'])
    unsigned, aligned, apk = (directory / name for name in ['unsigned.apk', 'aligned.apk', 'probe.apk'])
    run([build_tools / 'aapt2', 'link', '--manifest', manifest, '-I', android_jar, '-o', unsigned])
    with zipfile.ZipFile(unsigned, 'a') as archive:
        archive.write(dex / 'classes.dex', 'classes.dex')
        archive.write(guest, 'assets/guest')
        for name in [runtime['bds_translation']['helper_library']] + [item['packaged'] for item in native]:
            archive.write(Path(libraries) / name, 'lib/arm64-v8a/' + name)
    run([build_tools / 'zipalign', '-P', '16', '-f', '4', unsigned, aligned])
    key = directory / 'test.keystore'
    run(['keytool', '-genkeypair', '-keystore', key, '-storepass', 'android', '-alias', 'test',
         '-keypass', 'android', '-keyalg', 'RSA', '-keysize', '2048', '-validity', '1',
         '-dname', 'CN=Runtime Test'])
    run([build_tools / 'apksigner', 'sign', '--ks', key, '--ks-pass', 'pass:android', '--out', apk, aligned])
    adb = [shutil.which('adb'), '-s', serial]
    try:
        run(adb + ['install', '-r', apk])
        return run(adb + ['shell', 'am', 'instrument', '-w', '-r', application + '/runtimeprobe.Probe'])
    finally:
        run(adb + ['uninstall', application])
