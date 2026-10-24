#!/usr/bin/env bash
# Builds Cinnabar-<version>-<arch>.AppImage. Requires appimagetool (APPIMAGETOOL) and librsvg or ImageMagick.
# Usage: build-appimage.sh [out_dir]. Env: CLIENT, LOCAL_SERVER, CXB, BINARIES, ASSETC, ARCH (default uname -m),
# APPIMAGE_RUNTIME (pinned type2 runtime; appimagetool otherwise downloads the latest).
set -euo pipefail
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck source=../common/stage-payload.sh
source "$here/../common/stage-payload.sh"

out="${1:-$repo_root/.local/dist/linux-release}"
assetc="${ASSETC:-$repo_root/target/release/assetc}"
arch="${ARCH:-$(uname -m)}"
require_file "$assetc" 'build with make package-binaries'

appdir="$out/Cinnabar.AppDir"
rm -rf "$appdir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/cinnabar"
stage_runtime_binaries "$appdir/usr/bin"
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
