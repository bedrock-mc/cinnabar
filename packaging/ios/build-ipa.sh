#!/usr/bin/env bash
# Builds an unsigned arm64 iPhone/iPad IPA. iOS cannot spawn processes, so the Go core links into
# the app as a static archive. The app ships the shared resources only: no prep kit (iOS runs no
# helper executables) and no game assets; add-assets.py injects a user's own compiled carriers.
#
# Needs macOS with the Xcode major runtime.json pins (an /Applications/Xcode_<major>*.app is
# selected when present), the aarch64-apple-ios Rust target, Go, zip and librsvg or ImageMagick.
# CINNABAR_IOS_PROFILE picks the Cargo profile (release). The IPA is signed ad hoc only:
# sideloading tools sign it with the installer's identity.
set -euo pipefail

[[ "$(uname -s)" == Darwin ]] || { echo 'build-ipa.sh needs macOS with Xcode' >&2; exit 1; }

here="$(CDPATH='' cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../common/stage-payload.sh
source "$here/../common/stage-payload.sh"
cd "$repo_root"

pin() {
    python3 -c 'import json, sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$here/runtime.json" "$1"
}

triple=aarch64-apple-ios
profile="${CINNABAR_IOS_PROFILE:-release}"
xcode="$(pin xcode)"
executable="$(pin executable)"
export IPHONEOS_DEPLOYMENT_TARGET
IPHONEOS_DEPLOYMENT_TARGET="$(pin deployment_target)"
product="$(sed -n '/macro_rules! product_name/,/^}/s/^[[:space:]]*"\([^"]*\)"$/\1/p' crates/launcher/src/lib.rs | head -n 1)"
version="$(version_of)"
[[ -n "$product" ]] || { echo 'cannot read the launcher product name' >&2; exit 1; }

# The newest side-by-side install of the pinned Xcode major, as CI images name them.
shopt -s nullglob
installed=(/Applications/Xcode_"$xcode".app /Applications/Xcode_"$xcode".*.app)
shopt -u nullglob
if ((${#installed[@]})); then
    DEVELOPER_DIR="$(printf '%s\n' "${installed[@]}" | sort -V | tail -n 1)/Contents/Developer"
    export DEVELOPER_DIR
fi
sdk_version="$(xcrun --sdk iphoneos --show-sdk-version)"
[[ "${sdk_version%%.*}" == "$xcode" ]] || {
    echo "the iPhoneOS SDK is $sdk_version; install Xcode $xcode or point DEVELOPER_DIR at it" >&2
    exit 1
}
sdk_path="$(xcrun --sdk iphoneos --show-sdk-path)"

out="$repo_root/target/ios"
stage="$out/stage"
app="$stage/Payload/$executable.app"
ipa="$out/$product-$version-unsigned.ipa"
rm -rf "$stage" "$ipa"
mkdir -p "$app"

# One commit identifies both the app and the core, chosen as the Makefile does.
export CINNABAR_BUILD_COMMIT="${CINNABAR_BUILD_COMMIT:-$(git rev-parse HEAD)}"

target_flags="-isysroot $sdk_path -arch arm64 -miphoneos-version-min=$IPHONEOS_DEPLOYMENT_TARGET"
CGO_ENABLED=1 GOOS=ios GOARCH=arm64 CC="$(xcrun --sdk iphoneos --find clang)" \
    CGO_CFLAGS="$target_flags" CGO_LDFLAGS="$target_flags" \
    go build -buildmode=c-archive -trimpath \
    -ldflags "-s -w $(bash "$here/../common/core-ldflags.sh")" \
    -o "$out/libbedrock_core.a" ./core/cmd/bedrock-core

# The host crate's package and its single binary, as Cargo resolves them.
IFS=$'\t' read -r target_dir client_package client_binary < <(
    cargo metadata --format-version 1 --no-deps --locked | python3 -c '
import json, os, sys
meta = json.load(sys.stdin)
manifest = os.path.realpath(sys.argv[1])
package = next(p for p in meta["packages"] if os.path.realpath(p["manifest_path"]) == manifest)
binary = next(t["name"] for t in package["targets"] if "bin" in t["kind"])
print(meta["target_directory"], package["name"], binary, sep="\t")
' "$here/client/Cargo.toml")
CINNABAR_IOS_CORE_ARCHIVE="$out/libbedrock_core.a" \
    cargo build --target "$triple" --profile "$profile" --locked -p "$client_package"
# Cargo writes the `dev` profile's artifacts under `debug`.
profile_dir="$profile"
[[ "$profile" != dev ]] || profile_dir=debug
install -m 0755 "$target_dir/$triple/$profile_dir/$client_binary" "$app/$executable"

# A thin arm64 Mach-O for the iOS device platform at the pinned minimum version.
[[ "$(lipo -archs "$app/$executable")" == arm64 ]] || { echo "$executable is not thin arm64" >&2; exit 1; }
lipo "$app/$executable" -verify_arch arm64
build="$(xcrun vtool -show-build "$app/$executable" | awk '$1 == "platform" || $1 == "minos" {printf "%s ", $2}')"
[[ "$build" == "IOS $IPHONEOS_DEPLOYMENT_TARGET " ]] || {
    echo "$executable targets '$build', expected IOS $IPHONEOS_DEPLOYMENT_TARGET" >&2
    exit 1
}

# The resource root mirrors .local/; see runtime.json's resource_dir for the names iOS forbids.
stage_shared_resources "$app/$(pin resource_dir)"

sed -e "s|@EXECUTABLE@|$executable|g" -e "s|@BUNDLE_ID@|$(pin bundle_id)|g" \
    -e "s|@PRODUCT_NAME@|$product|g" -e "s|@VERSION@|$version|g" \
    -e "s|@DEPLOYMENT_TARGET@|$IPHONEOS_DEPLOYMENT_TARGET|g" -e "s|@SDK_VERSION@|$sdk_version|g" \
    "$here/Info.plist.in" >"$app/Info.plist"
if grep -q '@[A-Z_]*@' "$app/Info.plist"; then
    echo "unfilled template token in $app/Info.plist" >&2
    exit 1
fi
plutil -lint "$app/Info.plist"

# Pixel sizes for the base names Info.plist lists: iPhone 60pt @2x/@3x, iPad 76pt and 83.5pt @2x.
"$here/../icons/render.sh" "$out/icons" 120 180 152 167
cp "$out/icons/icon-120.png" "$app/AppIcon60x60@2x.png"
cp "$out/icons/icon-180.png" "$app/AppIcon60x60@3x.png"
cp "$out/icons/icon-152.png" "$app/AppIcon76x76@2x~ipad.png"
cp "$out/icons/icon-167.png" "$app/AppIcon83.5x83.5@2x~ipad.png"

# An ad-hoc signature fails here on a bundle layout iOS would refuse to install, such as a
# top-level Resources directory; the sideloading tool replaces it with the installer's identity.
codesign --force --sign - --timestamp=none "$app"
codesign --verify --strict "$app"
(cd "$stage" && zip -q -r "$ipa" Payload)
echo "$ipa"
