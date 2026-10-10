#!/usr/bin/env bash
# Sourced by the platform packagers. Stages the distributable payload without any Mojang-derived
# carrier: those are built per user on first run from the bundled prep kit.
set -euo pipefail

repo_root="$(CDPATH= cd -- "${CINNABAR_SOURCE_ROOT:-$(dirname -- "${BASH_SOURCE[0]}")/../..}" && pwd)"

# Shared with check-payload.sh; Windows copies the same set in build-installer.ps1.
kit_registry_stems=(block-registry block-light-registry biome-registry)

# stage_prep_kit <kit_dir> <assetc_binary>
stage_prep_kit() {
    local kit="$1" assetc="$2" name
    rm -rf "$kit"
    mkdir -p "$kit/bin" "$kit/assets" "$kit/data"
    install -m 0755 "$assetc" "$kit/bin/$(basename "$assetc")"
    cp "$repo_root"/assets/*.json "$kit/assets/"
    cp -R "$repo_root/assets/fonts" "$kit/assets/"
    for name in "${kit_registry_stems[@]}"; do
        cp "$repo_root/crates/assets/data/$name"-v2193.* "$kit/data/"
    done
}

# stage_resources <resource_root> <assetc_binary>: physics registry, notices, licenses, UI font, prep kit.
stage_resources() {
    local resources="$1" assetc="$2"
    mkdir -p "$resources/assets"
    install -m 0644 "$repo_root/crates/assets/data/block-physics-v2193.bin" "$resources/assets/block-physics-v2193.bin"
    install -m 0644 "$repo_root/THIRD_PARTY_NOTICES.md" "$resources/assets/THIRD_PARTY_NOTICES.md"
    mkdir -p "$resources/licenses"
    install -m 0644 "$repo_root/LICENSE" "$resources/licenses/Cinnabar-LICENSE.md"
    cp "$repo_root"/assets/licenses/* "$resources/licenses/"
    local font
    font="$(ui_font_path)"
    require_file "$font" 'restore the bundled Cinnangles Sans source'
    install -d "$resources/fonts"
    install -m 0644 "$font" "$resources/fonts/${font##*/}"
    stage_prep_kit "$resources/prep-kit" "$assetc"
    # Optional endpoints, injected by CI; absent means the feature is off.
    [[ -z "${CINNABAR_UPDATE_URL:-}" ]] || printf '%s\n' "$CINNABAR_UPDATE_URL" > "$resources/update-url"
}

# ui_font_path: the bundled Cinnangles Sans source; first-run draws with it.
ui_font_path() {
    local manifest="$repo_root/assets/cinnangles-sans-source.json" file
    file="$(sed -n 's/^[[:space:]]*"font_file"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$manifest" | head -n 1)"
    printf '%s\n' "$repo_root/assets/fonts/$file"
}

# resource_manifest <macos|linux|windows>: every file stage_resources (or build-installer.ps1)
# writes under the resource root, one relative path per line. Optional endpoint files excluded.
resource_manifest() {
    local platform="$1" exe='' file name
    [[ "$platform" != windows ]] || exe=.exe
    file="$(ui_font_path)"
    printf '%s\n' licenses/Cinnabar-LICENSE.md assets/block-physics-v2193.bin assets/THIRD_PARTY_NOTICES.md "fonts/${file##*/}" "prep-kit/bin/assetc$exe"
    for file in "$repo_root"/assets/licenses/*; do printf 'licenses/%s\n' "${file##*/}"; done
    for file in "$repo_root"/assets/*.json; do printf 'prep-kit/assets/%s\n' "${file##*/}"; done
    for file in "$repo_root"/assets/fonts/*; do printf 'prep-kit/assets/fonts/%s\n' "${file##*/}"; done
    for name in "${kit_registry_stems[@]}"; do
        for file in "$repo_root/crates/assets/data/$name"-v2193.*; do printf 'prep-kit/data/%s\n' "${file##*/}"; done
    done
}

# require_file <path> <hint>
require_file() {
    [[ -f "$1" ]] || { printf 'missing %s (%s)\n' "$1" "$2" >&2; exit 1; }
}

# version_of: workspace package version, the single release version source.
version_of() {
    sed -n '/^\[workspace.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p;}' "$repo_root/Cargo.toml" | head -n 1
}
