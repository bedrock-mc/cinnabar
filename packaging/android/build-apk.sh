#!/usr/bin/env bash
# Builds a signed arm64-v8a APK. The client library, the Go core and assetc ship as native
# libraries, the only files Android lets an app execute; the prep kit ships as APK assets. Game
# assets never enter the APK: first launch downloads and compiles them on the device.
#
# Needs ANDROID_HOME holding the SDK packages runtime.json pins, the aarch64-linux-android Rust
# target, cargo-ndk, Go, a JDK, zip and librsvg or ImageMagick for the launcher icon.
# CINNABAR_ANDROID_PROFILE picks the Cargo profile (release).
# PACKAGE_CARGO_FEATURES supplies the release channel's comma-separated Cargo features.
# Signing uses CINNABAR_ANDROID_KEYSTORE and CINNABAR_ANDROID_KEY_ALIAS with passwords in
# CINNABAR_ANDROID_STORE_PASSWORD and CINNABAR_ANDROID_KEY_PASSWORD, else a generated test key.
set -euo pipefail

here="$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../common/stage-payload.sh
source "$here/../common/stage-payload.sh"
cd "$repo_root"

pin() {
    python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$here/runtime.json" "$1"
}

abi=arm64-v8a
triple=aarch64-linux-android
profile="${CINNABAR_ANDROID_PROFILE:-release}"
sdk="${ANDROID_HOME:?set ANDROID_HOME to the Android SDK}"
ndk="${ANDROID_NDK_HOME:-$sdk/ndk/$(pin ndk)}"
build_tools="$sdk/build-tools/$(pin build_tools)"
android_jar="$sdk/platforms/android-$(pin target_sdk)/android.jar"
min_sdk="$(pin min_sdk)"
toolchain="$ndk/toolchains/llvm/prebuilt/$(uname -s | tr '[:upper:]' '[:lower:]')-x86_64"
client_package="$(python3 -c 'import sys, tomllib; print(tomllib.load(open(sys.argv[1], "rb"))["package"]["name"])' "$here/client/Cargo.toml")"
client_library="${client_package//-/_}"
product="$(sed -n '/macro_rules! product_name/,/^}/s/^[[:space:]]*"\([^"]*\)"$/\1/p' crates/launcher/src/lib.rs | head -n 1)"
version="$(version_of)"
IFS=. read -r major minor patch <<<"$version"
version_code=$((major * 1000000 + minor * 1000 + patch))
[[ -n "$product" ]] || { echo 'cannot read the launcher product name' >&2; exit 1; }

out="$repo_root/target/android"
stage="$out/stage"
libs="$stage/lib/$abi"
rm -rf "$stage" "$out/classes" "$out/dex"
mkdir -p "$libs" "$stage/assets" "$out/classes" "$out/dex"

# One commit identifies both the client library and the core, chosen as the Makefile does.
export CINNABAR_BUILD_COMMIT="${CINNABAR_BUILD_COMMIT:-$(git rev-parse HEAD)}"

# 16 KB pages: Android 15 devices may use them, and every LOAD segment must allow it.
page_flags='-Wl,-z,max-page-size=16384'
export RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=$page_flags"
# Bash 3.2 needs a guarded expansion for an empty array under nounset.
feature_args=()
if [[ -n "${PACKAGE_CARGO_FEATURES:-}" ]]; then
    feature_args=(--features "$PACKAGE_CARGO_FEATURES")
fi
cargo ndk -t "$abi" --platform "$min_sdk" build --profile "$profile" --locked -p "$client_package" ${feature_args[@]+"${feature_args[@]}"}
cargo ndk -t "$abi" --platform "$min_sdk" build --profile "$profile" --locked -p asset-compiler --bin assetc ${feature_args[@]+"${feature_args[@]}"}
target_dir="$(cargo metadata --format-version 1 --no-deps --locked | python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])')"
# Cargo writes the `dev` profile's artifacts under `debug`.
profile_dir="$profile"
[[ "$profile" != dev ]] || profile_dir=debug
rust_out="$target_dir/$triple/$profile_dir"
install -m 0755 "$rust_out/lib$client_library.so" "$libs/lib$client_library.so"
install -m 0755 "$rust_out/assetc" "$libs/$(pin compiler_library)"
install -m 0644 "$toolchain/sysroot/usr/lib/$triple/libc++_shared.so" "$libs/libc++_shared.so"

# Pion's Android interface lookup (anet) needs Go's private net linknames.
CGO_ENABLED=1 GOOS=android GOARCH=arm64 CC="$toolchain/bin/$triple$min_sdk-clang" \
    go build -buildmode=pie -trimpath \
    -ldflags "-s -w -checklinkname=0 -extldflags=$page_flags $(bash "$here/../common/core-ldflags.sh")" \
    -o "$libs/$(pin core_library)" ./core/cmd/bedrock-core

# The desktop resource root, minus the kit's compiler: setup links it from the native libraries.
stage_resources "$stage/assets/resources" "$libs/$(pin compiler_library)"
rm -rf "$stage/assets/resources/prep-kit/bin"

# shellcheck source=render-template.sh
source "$here/render-template.sh"
mkdir -p "$out/java"
render "$here/AndroidManifest.xml" "$out/AndroidManifest.xml"
for source in "$here"/*.java; do
    render "$source" "$out/java/$(basename "$source")"
done
javac --release 8 -nowarn -classpath "$android_jar" -d "$out/classes" "$out"/java/*.java
find "$out/classes" -name '*.class' -print0 |
    xargs -0 "$build_tools/d8" --release --min-api "$min_sdk" --lib "$android_jar" --output "$out/dex"
cp "$out/dex/classes.dex" "$stage/classes.dex"

# shellcheck source=resources.sh
source "$here/resources.sh"
link_apk_resources "$out/AndroidManifest.xml" "$stage/assets" "$out"
(cd "$stage" && zip -q -r "$out/unsigned.apk" classes.dex lib)
"$build_tools/zipalign" -P 16 -f 4 "$out/unsigned.apk" "$out/aligned.apk"

keystore="${CINNABAR_ANDROID_KEYSTORE:-$out/test.keystore}"
key_alias="${CINNABAR_ANDROID_KEY_ALIAS:-cinnabar-test}"
if [[ -z "${CINNABAR_ANDROID_KEYSTORE:-}" ]]; then
    export CINNABAR_ANDROID_STORE_PASSWORD=android CINNABAR_ANDROID_KEY_PASSWORD=android
    [[ -f "$keystore" ]] || keytool -genkeypair -keystore "$keystore" -storepass android \
        -keypass android -alias "$key_alias" -keyalg RSA -keysize 2048 -validity 10000 \
        -dname "CN=$product Android test"
fi
apk="$out/$product-$version-$abi.apk"
"$build_tools/apksigner" sign --ks "$keystore" --ks-key-alias "$key_alias" \
    --ks-pass env:CINNABAR_ANDROID_STORE_PASSWORD --key-pass env:CINNABAR_ANDROID_KEY_PASSWORD \
    --out "$apk" "$out/aligned.apk"
"$build_tools/apksigner" verify "$apk"
echo "$apk"
