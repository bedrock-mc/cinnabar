#!/usr/bin/env bash
# Builds the prepared source archive without network access inside flatpak-builder.
set -euo pipefail
source packaging/common/stage-payload.sh

export CARGO_BUILD_JOBS="${FLATPAK_BUILDER_N_JOBS:-4}"
export CINNABAR_BUILD_COMMIT="$(cat packaging/flatpak/source-commit)"
mapfile -t rust_bins < <(awk '$2 != "-" {print "-p"; print $2; print "--bin"; print $1}' "$runtime_binary_list")
cargo build --offline --locked --release --no-default-features "${rust_bins[@]}" -p asset-compiler --bin assetc
stamp="$(bash packaging/common/core-ldflags.sh)"
root="$PWD"
(cd core && go build -trimpath -ldflags "-s -w $stamp" -o "$root/target/release/bedrock-core" ./cmd/bedrock-core)
(cd tools/localserver && go build -trimpath -ldflags '-s -w' -o "$root/target/release/bedrock-local-server" .)

export BINARIES="$root/target/release"
stage_runtime_binaries "$FLATPAK_DEST/bin"
stage_resources "$FLATPAK_DEST/share/cinnabar" "$BINARIES/assetc"
app_id="$FLATPAK_ID"
install -Dm0644 packaging/linux/cinnabar.desktop "$FLATPAK_DEST/share/applications/$app_id.desktop"
sed -i "s/^Icon=.*/Icon=$app_id/" "$FLATPAK_DEST/share/applications/$app_id.desktop"
install -Dm0644 packaging/icons/cinnabar.svg "$FLATPAK_DEST/share/icons/hicolor/scalable/apps/$app_id.svg"
install -Dm0644 "packaging/flatpak/$app_id.metainfo.xml" "$FLATPAK_DEST/share/metainfo/$app_id.metainfo.xml"
install -Dm0644 LICENSE "$FLATPAK_DEST/share/licenses/$app_id/LICENSE"
