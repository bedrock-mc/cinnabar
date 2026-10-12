#!/usr/bin/env bash
# Builds Cinnabar-<version>-<arch>.AppImage. Requires appimagetool (APPIMAGETOOL) and librsvg or ImageMagick.
# Usage: build-appimage.sh [out_dir]. Env: CLIENT, CORE, LOCAL_SERVER, ASSETC, ARCH (default uname -m),
# APPIMAGE_RUNTIME (pinned type2 runtime; appimagetool otherwise downloads the latest).
set -euo pipefail
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck source=../common/stage-payload.sh
source "$here/../common/stage-payload.sh"

out="${1:-$repo_root/.local/dist/linux-release}"
client="${CLIENT:-$repo_root/target/release/cinnabar}"
core="${CORE:-$repo_root/target/release/bedrock-core}"
local_server="${LOCAL_SERVER:-$repo_root/target/release/bedrock-local-server}"
assetc="${ASSETC:-$repo_root/target/release/assetc}"
arch="${ARCH:-$(uname -m)}"
for binary in "$client" "$core" "$local_server" "$assetc"; do require_file "$binary" 'build with make package-binaries'; done

appdir="$out/Cinnabar.AppDir"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/cinnabar"
install -m 0755 "$client" "$appdir/usr/bin/cinnabar"
install -m 0755 "$core" "$appdir/usr/bin/bedrock-core"
install -m 0755 "$local_server" "$appdir/usr/bin/bedrock-local-server"
stage_resources "$appdir/usr/share/cinnabar" "$assetc"
install -m 0755 "$here/AppRun" "$appdir/AppRun"
install -m 0644 "$here/cinnabar.desktop" "$appdir/cinnabar.desktop"
"$here/../icons/render.sh" "$out/icons" 256
install -m 0644 "$out/icons/icon-256.png" "$appdir/cinnabar.png"

image="$out/Cinnabar-$(version_of)-$arch.AppImage"
runtime_args=()
[[ -z "${APPIMAGE_RUNTIME:-}" ]] || runtime_args=(--runtime-file "$APPIMAGE_RUNTIME")
ARCH="$arch" "${APPIMAGETOOL:-appimagetool}" "${runtime_args[@]}" "$appdir" "$image"
echo "$image"
