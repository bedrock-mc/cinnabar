"""Exercise Android Linux-runtime staging and an optional emulator execution probe."""

import json
import os
from pathlib import Path
import platform
import shlex
import shutil
import subprocess
import sys
import tempfile
import unittest

from android_runtime_probe import run_app_probe


ROOT = Path(__file__).resolve().parents[2]
RUNTIME = json.loads((ROOT / "packaging/android/runtime.json").read_text())
PIN = RUNTIME["bds_translation"]


class AndroidBox64Tests(unittest.TestCase):
    def test_guest_library_pin_is_verified_against_the_build_source(self):
        """Reject a stale size or digest before packaging the translated runtime."""
        import hashlib

        cmake = shutil.which("cmake")
        if not cmake:
            self.skipTest("missing fixture: CMake for guest library pin verification")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            library = root / PIN["guest_directory"] / PIN["guest_library"]
            library.parent.mkdir(parents=True)
            payload = b"original guest library fixture"
            library.write_bytes(payload)
            pin = dict(PIN, guest_bytes=len(payload), guest_sha256=hashlib.sha256(payload).hexdigest())
            for field, replacement in [(None, None), ("guest_bytes", len(payload) + 1),
                                       ("guest_sha256", "0" * 64)]:
                with self.subTest(field=field):
                    changed = dict(pin)
                    if field:
                        changed[field] = replacement
                    manifest = root / "runtime.json"
                    manifest.write_text(json.dumps({"bds_translation": changed}))
                    result = subprocess.run([
                        cmake, "-DMANIFEST=" + str(manifest), "-DSOURCE_DIR=" + str(root),
                        "-P", str(ROOT / "packaging/android/box64/verify-guest.cmake"),
                    ], capture_output=True, text=True)
                    self.assertEqual(result.returncode == 0, field is None, result.stdout + result.stderr)

    def build_fixture(self, missing=False, replacement=False):
        """Run production staging with compiler outputs supplied by a deterministic fixture."""
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        directory = Path(temporary.name)
        tools = directory / "tools"
        tools.mkdir()
        libs = directory / "libs"
        libs.mkdir()
        (libs / PIN["helper_library"]).write_bytes(b"stale helper")
        cmake = tools / "cmake"
        cmake.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
if "--build" not in sys.argv:
    sys.exit(0)
root = pathlib.Path(sys.argv[sys.argv.index("--build") + 1])
pin = json.loads(pathlib.Path(os.environ["FIXTURE_MANIFEST"]).read_text())["bds_translation"]
(root / "build").mkdir(parents=True)
(root / "build/box64").write_bytes(b"translator fixture")
for library in pin["native_runtime"]["libraries"]:
    if os.environ.get("MISSING_RUNTIME") == library["source"]:
        continue
    path = root / "glibc" / pin["native_runtime"]["prefix"] / library["source"]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(library["source"])
''')
        strip = tools / "llvm-strip"
        strip.write_text('''#!/usr/bin/env python3
import pathlib, shutil, sys
shutil.copyfile(sys.argv[-1], sys.argv[sys.argv.index("-o") + 1])
''')
        cmake.chmod(0o755)
        strip.chmod(0o755)
        env = dict(os.environ, PATH=f"{tools}{os.pathsep}{os.environ['PATH']}",
                   FIXTURE_MANIFEST=str(ROOT / "packaging/android/runtime.json"))
        env.pop("CINNABAR_ANDROID_BDS_NATIVE_DIR", None)
        if missing:
            env["MISSING_RUNTIME"] = PIN["native_runtime"]["libraries"][-1]["source"]
        if replacement:
            custom = directory / "replacement"
            custom.mkdir()
            for library in PIN["native_runtime"]["libraries"]:
                (custom / library["source"]).write_text("modified " + library["source"])
            env["CINNABAR_ANDROID_BDS_NATIVE_DIR"] = str(custom)
        result = subprocess.run(["bash", "-euc", '''
here="$1/packaging/android"
out="$2"
libs="$out/libs"
toolchain="$out"
page_flags=-Wl,-z,max-page-size=16384
mkdir "$out/bin"
ln -s "$out/tools/llvm-strip" "$out/bin/llvm-strip"
source "$here/build-box64.sh"
build_box64
''', "android-bds-staging-test", str(ROOT), str(directory)], env=env,
                                capture_output=True, text=True)
        return result, libs

    def test_complete_native_runtime_is_installed_as_executable_apk_libraries(self):
        result, libs = self.build_fixture()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual((libs / PIN["helper_library"]).read_bytes(), b"translator fixture")
        for library in PIN["native_runtime"]["libraries"]:
            with self.subTest(library=library["source"]):
                path = libs / library["packaged"]
                self.assertEqual(path.read_text(), library["source"])
                self.assertTrue(path.stat().st_mode & 0o100)

    def test_incomplete_native_runtime_stops_packaging(self):
        result, _ = self.build_fixture(missing=True)
        self.assertNotEqual(result.returncode, 0)

    def test_modified_native_libraries_can_replace_the_bundled_runtime(self):
        result, libs = self.build_fixture(replacement=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        for library in PIN["native_runtime"]["libraries"]:
            self.assertEqual((libs / library["packaged"]).read_text(), "modified " + library["source"])

    def verify_fixture(self, missing):
        """Check dependency admission using the dynamic metadata exposed by readelf."""
        cmake = shutil.which("cmake")
        if not cmake:
            self.skipTest("missing fixture: CMake for native runtime dependency verification")
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            readelf = root / "readelf"
            readelf.write_text('''#!/usr/bin/env python3
import json, os, pathlib, sys
pin = json.loads(pathlib.Path(os.environ["FIXTURE_MANIFEST"]).read_text())["bds_translation"]
name = pathlib.Path(sys.argv[-1]).name
print("Type: DYN (Shared object file)\\nMachine: AArch64")
for library in pin["native_runtime"]["libraries"]:
    if library["packaged"] == name:
        print("Library soname: [" + library["source"] + "]")
if name == pin["helper_library"]:
    dependency = "not-bundled.so" if os.environ.get("MISSING_DEPENDENCY") else next(
        item["source"] for item in pin["native_runtime"]["libraries"] if not item.get("loader"))
    print("Shared library: [" + dependency + "]")
''')
            readelf.chmod(0o755)
            env = dict(os.environ, FIXTURE_MANIFEST=str(ROOT / "packaging/android/runtime.json"))
            if missing:
                env["MISSING_DEPENDENCY"] = "1"
            result = subprocess.run([cmake, "-DMANIFEST=" + env["FIXTURE_MANIFEST"],
                                     "-DDIRECTORY=" + str(root), "-DREADELF=" + str(readelf),
                                     "-P", str(ROOT / "packaging/android/box64/verify.cmake")],
                                    env=env, capture_output=True, text=True)
            return result

    def test_native_dependencies_resolve_from_the_bundled_libraries(self):
        result = self.verify_fixture(False)
        self.assertEqual(result.returncode, 0, result.stderr)

    def test_a_new_unbundled_native_dependency_stops_packaging(self):
        result = self.verify_fixture(True)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn("not packaged", result.stderr)

    def test_loopback_is_routable_and_kernel_interfaces_are_preserved(self):
        """Exercise the real adapter with only loopback, online, down and empty interfaces."""
        if sys.platform != "linux":
            self.skipTest("missing fixture: Linux host for the Android interface adapter")
        compiler = shutil.which("c++")
        if not compiler:
            self.skipTest("missing fixture: C++ compiler for the Android interface adapter")
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "adapter"
            result = subprocess.run([
                compiler, "-std=c++17", "-fno-exceptions", "-fno-rtti",
                *shlex.split(os.environ.get("CINNABAR_HOST_CXXFLAGS", "")),
                "-include", str(ROOT / "packaging/android/box64/network.h"),
                "-Dgetifaddrs=cinnabar_getifaddrs", "-Dfreeifaddrs=cinnabar_freeifaddrs",
                str(ROOT / "packaging/android/box64/network.cpp"),
                str(ROOT / "packaging/tests/android_network_adapter.cpp"), "-o", str(executable),
            ], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([str(executable)], capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn("routed loopback adapter ok", result.stdout)

    def test_android_dns_proxy_protocol(self):
        """Serve the real resolver adapter a scripted Android resolver on a temporary socket.

        CINNABAR_HOST_CXXFLAGS adds host compiler flags such as sanitizers.
        """
        if sys.platform != "linux":
            self.skipTest("missing fixture: Linux host for the Android resolver adapter")
        compiler = shutil.which("c++")
        if not compiler:
            self.skipTest("missing fixture: C++ compiler for the Android resolver adapter")
        box64 = ROOT / "packaging/android/box64"
        with tempfile.TemporaryDirectory() as temporary:
            executable = Path(temporary) / "dns-adapter"
            result = subprocess.run([
                compiler, "-std=c++17", "-fno-exceptions", "-fno-rtti", "-pthread",
                *shlex.split(os.environ.get("CINNABAR_HOST_CXXFLAGS", "")), "-I", str(box64),
                str(box64 / "dns.cpp"), str(ROOT / "packaging/tests/android_dns_adapter.cpp"),
                "-o", str(executable),
            ], capture_output=True, text=True)
            self.assertEqual(result.returncode, 0, result.stderr)
            result = subprocess.run([str(executable)], capture_output=True, text=True, timeout=120)
            self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
            self.assertIn("android resolver adapter ok", result.stdout)
            self.assertEqual(result.stderr.count("Android resolver unavailable"), 1, result.stderr)
            missing = next((line for line in result.stdout.splitlines() if "missing fixture:" in line), None)
            if missing:
                self.skipTest(missing)

    def test_linux_guest_starts_on_android(self):
        """Check Linux startup, locale, threads, storage and sockets on a selected emulator."""
        serial = os.environ.get("CINNABAR_ANDROID_TEST_SERIAL")
        libraries = os.environ.get("CINNABAR_ANDROID_BDS_LIBS")
        adb = shutil.which("adb")
        compiler = os.environ.get("CC") or shutil.which("cc")
        if not serial or not libraries or not adb or not compiler or not os.environ.get("ANDROID_HOME"):
            self.skipTest("missing fixture: Android SDK, serial, built BDS libraries, adb and C compiler")
        if not all(shutil.which(tool) for tool in ("javac", "java", "keytool")):
            self.skipTest("missing fixture: JDK for the Android sandbox probe")
        if sys.platform != "linux" or platform.machine() != "x86_64":
            self.skipTest("missing fixture: Linux x86_64 guest compiler")
        with tempfile.TemporaryDirectory() as temporary:
            directory = Path(temporary)
            source = directory / "guest.c"
            source.write_text('''
#include <assert.h>
#include <arpa/inet.h>
#include <ctype.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <netdb.h>
#include <pthread.h>
#include <stdio.h>
#include <sys/socket.h>
#include <unistd.h>
static void *worker(void *value) { *(int *)value = 42; return NULL; }
int main(int argc, char **argv) {
    assert(argc == 2 && isalpha((unsigned char)argv[1][1]));
    struct ifaddrs *addresses = NULL;
    assert(getifaddrs(&addresses) == 0);
    int loopback4 = 0, loopback6 = 0;
    for (struct ifaddrs *item = addresses; item; item = item->ifa_next) {
        if (!item->ifa_addr || !(item->ifa_flags & IFF_LOOPBACK)) continue;
        assert(item->ifa_name && item->ifa_netmask && (item->ifa_flags & IFF_UP));
        loopback4 |= item->ifa_addr->sa_family == AF_INET;
        loopback6 |= item->ifa_addr->sa_family == AF_INET6;
    }
    int routed = 0;
    for (struct ifaddrs *item = addresses; item; item = item->ifa_next) {
        struct sockaddr_in *address = (struct sockaddr_in *)item->ifa_addr;
        if (!address || address->sin_family != AF_INET || (item->ifa_flags & IFF_LOOPBACK)) continue;
        routed |= address->sin_addr.s_addr == htonl(INADDR_LOOPBACK)
            && (item->ifa_flags & (IFF_UP | IFF_RUNNING)) == (IFF_UP | IFF_RUNNING);
    }
    freeifaddrs(addresses);
    assert(loopback4 && loopback6 && routed);
    struct addrinfo hints = {.ai_family = AF_UNSPEC, .ai_socktype = SOCK_STREAM};
    struct addrinfo *resolved = NULL;
    assert(getaddrinfo("localhost", "80", &hints, &resolved) == 0 && resolved);
    freeaddrinfo(resolved);
    resolved = NULL;
    assert(getaddrinfo("localhost", "80", NULL, &resolved) == 0 && resolved);
    freeaddrinfo(resolved);
    pthread_t thread;
    int value = 0;
    assert(pthread_create(&thread, NULL, worker, &value) == 0);
    assert(pthread_join(thread, NULL) == 0 && value == 42);
    FILE *file = fopen(argv[1], "w+");
    assert(file && fputs("saved", file) >= 0);
    rewind(file);
    assert(fgetc(file) == 's' && fclose(file) == 0);
    int socket_fd = socket(AF_INET, SOCK_DGRAM, 0);
    struct sockaddr_in address = {.sin_family=AF_INET, .sin_addr.s_addr=htonl(INADDR_LOOPBACK)};
    assert(socket_fd >= 0 && bind(socket_fd, (void *)&address, sizeof(address)) == 0);
    assert(close(socket_fd) == 0);
    puts("original Linux guest started");
}
''')
            guest = directory / "guest"
            subprocess.run([compiler, str(source), "-pthread", "-o", str(guest)], check=True)
            result = run_app_probe(directory, guest, libraries, serial, RUNTIME)
            self.assertIn("guest_exit=0", result.stdout, result.stdout + result.stderr)
            self.assertIn("original Linux guest started", result.stdout)



if __name__ == "__main__":
    unittest.main()
