# Android (experimental)

The Android client is the desktop client built for arm64-v8a Android 8+ devices with Vulkan.
The APK holds the client library, the Go core and `assetc`, plus the prep kit. It ships no game
assets: first launch asks you to accept the Minecraft EULA, then downloads Mojang's sample
resource pack and compiles the carriers into the app's private storage. Keep the app open
until setup finishes.

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

Install a JDK, Go, librsvg or ImageMagick for the launcher icon, the Rust toolchain from
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
outside the inventory. Settings > Touch adjusts look sensitivity and explains the on-screen controls. A hardware keyboard and mouse also drive gameplay.

## Not supported yet

An on-screen keyboard: typing chat, commands, sign text or a server address needs a hardware
keyboard, while featured servers need only sign-in, whose code you enter on another device.
These stay keyboard, mouse or controller only: dropping the held item from the hotbar, pick
block, the emote wheel, the player list, perspective, and mod or server Experience key
bindings. Also missing are the Back gesture, clipboard, opening links, local worlds, Microsoft
sign-in while the app is in the background, touch scrolling in menus and forms, and the native
touch layouts, remapping and control customization. Device performance, audio and suspend/resume are untested. See
`plan.md`.
