# Android (experimental)

The Android client is the desktop client built for arm64-v8a Android 8+ devices with Vulkan.
The APK holds the client library, the Go core, the Dragonfly local server and `assetc`, plus
the prep kit. It ships no game assets: first launch asks you to accept the Minecraft EULA,
then downloads Mojang's sample resource pack and compiles the carriers into the app's private
storage. Keep the app open until setup finishes.

## Release APKs

Tagged Beta and nightly releases include `Cinnabar-arm64-v8a.apk` and its checksum in
`SHA256SUMS.txt`. Download the APK from the release and open it on an arm64 Android 8+ device,
allowing installation from that download source when Android asks. Release APKs use a persistent
signing key so later APKs can update the installed app. The desktop updater does not install APKs.
Uninstall an earlier CI test build before installing a release APK; this clears its app data.

On Android 11+, reopening the app after an unexpected exit offers **Copy details** with the
device, exit reason and the end of the client log. Include those details when reporting a crash.
**Save report** writes a ZIP to Downloads with those details and Android's retained crash trace,
when available. For native crashes on Android 12+, the trace is the original tombstone protobuf;
attach the ZIP to the crash report. Nothing is uploaded automatically.

## Build and install

Install a JDK, Go, CMake, Ninja, a Linux ARM64 GCC/G++ cross-toolchain, librsvg or ImageMagick
for the launcher icon, the Rust toolchain from
`rust-toolchain.toml` with the `aarch64-linux-android` target, the `cargo-ndk` version in
`packaging/android/runtime.json`, and
the Android SDK platform, build-tools and NDK versions pinned there. Then run:

```sh
ANDROID_HOME=/path/to/sdk bash packaging/android/build-apk.sh
adb install target/android/Cinnabar-*-arm64-v8a.apk
```

Without `CINNABAR_ANDROID_KEYSTORE` the script signs with a test key in `target/android`. The
Android CI workflow builds a test APK for pull requests that change `packaging/android/` or the
release workflow, every six hours and on manual runs, and attaches it to the run. Each run uses its own key, so
uninstall an earlier build (which clears its data) before installing a newer one.

## Controls

The stick moves, dragging elsewhere turns, and separate fingers hold jump, sneak, sprint,
attack and use; use also opens a held book. The top-right buttons open pause and the inventory,
and tapping a hotbar slot selects it. Screens without touch handling of their own, such as the
inventory, containers, server forms, the bed, the sign editor and the credits, take one finger
as the pointer; the inventory's close button closes it. Drop items by releasing a held stack
outside the inventory. Swipe Settings content or drag its scrollbar to scroll. Tapping a setting
still activates it; a swipe does not. Settings > Touch adjusts look sensitivity and explains
the on-screen controls. A hardware keyboard and mouse also drive gameplay.

Launcher text fields, including server addresses and the People drawer, request the on-screen
keyboard. People search filters friends currently playing; it does not search Xbox users.

## Local worlds

Dragonfly is bundled for local worlds, with Normal or Flat terrain. The server runs from the
APK's native-library directory and saves worlds in private app storage. Android creation,
reopening, offline play and suspend/resume still need device acceptance.

BDS is the default for new worlds. Android APK builds include the Box64 runtime and require
CMake, Ninja and a Linux ARM64 GCC/G++ cross-toolchain. The tested native runtime package
is preserved in a project-owned dependency release, with the same content hash and matching
sources; its URLs are in `packaging/android/runtime.json`. Set `CINNABAR_ANDROID_BDS_CC`
and `CINNABAR_ANDROID_BDS_CXX` if the compilers are not named `aarch64-linux-gnu-gcc` and
`aarch64-linux-gnu-g++`.

The experimental runtime packages an Android-adapted GNU C Library, uses Android-compatible
interface enumeration and requests stronger x86 memory ordering from the translator. A
target-SDK-35 test app on an Android 15 emulator has started BDS,
created a world, saved a changed spawn position, shut down cleanly and reopened the same world
with SELinux enforcing. A production protocol client also completes login and spawn; the
emulator run needed a longer diagnostic deadline than the standard live test. This runtime test
does not establish acceptance of the complete client APK or physical phones.

Starting a BDS world after accepting its EULA acquires the official
Linux server and pinned guest runtime on first use. No server binaries or game assets ship in
the APK. Dragonfly remains available as an alternative. Login, offline play, lifecycle and performance acceptance
remain incomplete across supported Android versions, CPU vendors and 4/16 KB page devices.
When all external network interfaces are down, the client reports that BDS needs a network
connection. Earlier startup
aborts leave extended reliability testing open.
BDS worlds cannot pause simulation.

With an emulator running, the runtime probe can check the packaged helper under the app's
normal permissions, including IPv4/IPv6 enumeration, threads, storage and UDP:

```sh
ANDROID_HOME=/path/to/android-sdk CINNABAR_ANDROID_TEST_SERIAL=emulator-5554 \
  CINNABAR_ANDROID_BDS_LIBS=target/android/stage/lib/arm64-v8a \
  python3 -m unittest discover -s packaging/tests -p test_android_box64.py
```

Put the SDK platform-tools and JDK on `PATH`. The probe uses an original Linux test program
and does not require Minecraft assets or a server download.

## Not supported yet

Typing chat, commands or sign text still needs a hardware keyboard. Featured servers need only
sign-in, whose code you enter on another device.
These stay keyboard, mouse or controller only: dropping the held item from the hotbar, pick
block, the emote wheel, the player list, perspective, and mod or server Experience key
bindings. Also missing are the Back gesture, clipboard, opening links, Microsoft
sign-in while the app is in the background, touch scrolling in forms and menus outside Settings,
and the native touch layouts, remapping and control customization. Device performance, audio
and suspend/resume are untested. See
`plan.md`.
