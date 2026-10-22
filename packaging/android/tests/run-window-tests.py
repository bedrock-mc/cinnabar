#!/usr/bin/env python3
"""Build and run the system-bar regression fixture on an explicitly selected Android device."""

import argparse
import json
import os
import shlex
from pathlib import Path
import subprocess
import xml.etree.ElementTree as ET


def run(*args, **kwargs):
    """Run a tool and stop immediately if it fails."""
    return subprocess.run(args, check=True, **kwargs)


def main():
    """Package only the window fixture, leaving the installed game and its data untouched."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", default=os.environ.get("ANDROID_HOME"), required="ANDROID_HOME" not in os.environ)
    parser.add_argument("--serial", required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    here = Path(__file__).resolve().parent
    pins = json.loads((here.parent / "runtime.json").read_text())
    sdk = Path(args.sdk)
    tools = sdk / "build-tools" / pins["build_tools"]
    android = sdk / "platforms" / f"android-{pins['target_sdk']}" / "android.jar"
    adb = sdk / "platform-tools" / "adb"
    out = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)
    for child in ("src", "classes", "dex"):
        (out / child).mkdir(exist_ok=True)
    package = "app.cinnabar.windowtests"
    minimum_api = "30"
    source = (here.parent / "ImmersiveMode.java").read_text().replace("@APPLICATION_ID@", package)
    (out / "src" / "ImmersiveMode.java").write_text(source)
    run("javac", "--release", "8", "-Xlint:-options", "-classpath", str(android), "-d", str(out / "classes"),
        str(out / "src" / "ImmersiveMode.java"), *map(str, here.glob("*.java")))
    run(str(tools / "d8"), "--min-api", minimum_api, "--lib", str(android), "--output", str(out / "dex"),
        *map(str, (out / "classes").rglob("*.class")))
    manifest = ET.Element("manifest", {"xmlns:android": "http://schemas.android.com/apk/res/android", "package": package})
    application = ET.SubElement(manifest, "application", {
        "android:theme": "@android:style/Theme.Material.NoActionBar.Fullscreen",
        "android:label": "Android window tests", "android:debuggable": "true"})
    ET.SubElement(application, "activity", {"android:name": ".WindowActivity", "android:exported": "true",
        "android:screenOrientation": "landscape"})
    ET.SubElement(manifest, "instrumentation", {"android:name": ".WindowTests", "android:targetPackage": package})
    ET.ElementTree(manifest).write(out / "AndroidManifest.xml", encoding="utf-8", xml_declaration=True)
    run(str(tools / "aapt2"), "link", "-o", str(out / "unsigned.apk"), "-I", str(android),
        "--manifest", str(out / "AndroidManifest.xml"), "--min-sdk-version", minimum_api,
        "--target-sdk-version", str(pins["target_sdk"]))
    run("zip", "-q", str(out / "unsigned.apk"), "classes.dex", cwd=out / "dex")
    run(str(tools / "zipalign"), "-f", "4", str(out / "unsigned.apk"), str(out / "aligned.apk"))
    key = out / "window-tests.keystore"
    if not key.exists():
        run("keytool", "-genkeypair", "-keystore", str(key), "-storepass", "android", "-keypass", "android",
            "-alias", "window-tests", "-keyalg", "RSA", "-validity", "30", "-dname", "CN=Android window tests")
    apk = out / "window-tests.apk"
    run(str(tools / "apksigner"), "sign", "--ks", str(key), "--ks-pass", "pass:android", "--out", str(apk), str(out / "aligned.apk"))
    run(str(adb), "-s", args.serial, "install", "-r", str(apk))
    confirmation_key = "immersive_mode_confirmations"
    previous = run(str(adb), "-s", args.serial, "shell", "settings", "get", "secure", confirmation_key,
                   capture_output=True, text=True).stdout.strip()
    try:
        run(str(adb), "-s", args.serial, "shell", "settings", "put", "secure", confirmation_key, "confirmed")
        result = run(str(adb), "-s", args.serial, "shell", "am", "instrument", "-w", f"{package}/.WindowTests",
                     capture_output=True, text=True)
    finally:
        if previous == "null":
            run(str(adb), "-s", args.serial, "shell", "settings", "delete", "secure", confirmation_key)
        else:
            run(str(adb), "-s", args.serial, "shell", "settings", "put", "secure", confirmation_key, shlex.quote(previous))
    print(result.stdout, end="")
    if not any(line.startswith("PASS: ") for line in result.stdout.splitlines()) or "FAIL:" in result.stdout:
        raise SystemExit("window tests failed")
    for name in ("window-before.png", "window-after.png", "window-keyboard.png"):
        with (out / name).open("wb") as image:
            run(str(adb), "-s", args.serial, "exec-out", "run-as", package, "cat", f"files/{name}", stdout=image)


if __name__ == "__main__":
    main()
