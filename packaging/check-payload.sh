#!/usr/bin/env bash
# Fails when an extracted installer holds any file outside the allowlist, so no Mojang asset (or
# anything else unreviewed) can ship. Missing expected files fail too, as does a prep kit its own
# compiler cannot plan a first run from.
# Usage: check-payload.sh <macos|linux|windows> <mounted DMG | squashfs-root | MSI admin image>
set -euo pipefail
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck source=common/stage-payload.sh
source "$here/common/stage-payload.sh"

platform="${1:?usage: check-payload.sh <macos|linux|windows> <dir>}"
root="$(CDPATH= cd -- "${2:?usage: check-payload.sh <macos|linux|windows> <dir>}" && pwd)"

require_file "$runtime_binary_list" 'restore the runtime binary table'

# Required and optional entries relative to $root; optional ones may be absent.
case "$platform" in
    macos)
        prefix=Cinnabar.app/Contents/
        resources="${prefix}Resources/"
        required=(Applications "${prefix}Info.plist" "${resources}Cinnabar.icns")
        while IFS= read -r name; do required+=("${prefix}MacOS/$name"); done < <(runtime_binaries)
        # CodeResources at the top of Contents is the stapled notarization ticket.
        optional=("${prefix}_CodeSignature/CodeResources" "${prefix}CodeResources")
        ;;
    linux)
        prefix=''
        resources=usr/share/cinnabar/
        required=(AppRun cinnabar.desktop cinnabar.png)
        while IFS= read -r name; do required+=("usr/bin/$name"); done < <(runtime_binaries)
        optional=(.DirIcon)
        ;;
    windows)
        # The admin image nests the install dir under a standard-folder name; find it by the client.
        client="$(cd "$root" && find . -type f -name cinnabar.exe | sed 's|^\./||')"
        [[ -n "$client" && "$client" != *$'\n'* ]] || { echo "expected exactly one cinnabar.exe under $root" >&2; exit 1; }
        prefix="$(dirname -- "$client")/"
        [[ "$prefix" != ./ ]] || prefix=''
        resources="${prefix}resources/"
        required=()
        while IFS= read -r name; do required+=("$prefix$name.exe"); done < <(runtime_binaries)
        optional=()
        # msiexec /a leaves a copy of the package beside the image.
        while IFS= read -r msi; do optional+=("$msi"); done < <(cd "$root" && find . -maxdepth 1 -type f -name '*.msi' | sed 's|^\./||')
        ;;
    *) echo "unknown platform: $platform" >&2; exit 2 ;;
esac
require_file "$prep_kit_list" "$prep_kit_hint"
while IFS= read -r file; do required+=("$resources$file"); done < <(resource_manifest "$platform")

allowed="$(printf '%s\n' "${required[@]}" "${optional[@]}" | sort -u)"
actual="$(cd "$root" && find . ! -type d | sed 's|^\./||' | sort)"
status=0
while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    if ! grep -qxF -- "$file" <<<"$allowed"; then
        printf 'unexpected file in %s artifact: %s\n' "$platform" "$file" >&2
        status=1
    fi
done <<<"$actual"
for file in "${required[@]}"; do
    if ! grep -qxF -- "$file" <<<"$actual"; then
        printf 'missing file in %s artifact: %s\n' "$platform" "$file" >&2
        status=1
    fi
done
[[ $status -ne 0 ]] || printf '%s artifact holds only allowlisted files (%s entries)\n' "$platform" "$(wc -l <<<"$actual" | tr -d ' ')"

# The shipped compiler plans a first run against the shipped kit, hashing every input it reads.
native() { if command -v cygpath >/dev/null; then cygpath -w "$1"; else printf '%s\n' "$1"; fi; }
kit="$root/${resources}prep-kit"
assetc="$kit/bin/assetc"
[[ "$platform" != windows ]] || assetc+=.exe
case "$(uname -s)" in
    Darwin) host=macos ;;
    Linux) host=linux ;;
    MINGW* | MSYS* | CYGWIN*) host=windows ;;
    *) host=unknown ;;
esac
scratch="$(mktemp -d)"
runs=0
[[ "$host" != "$platform" ]] || "$assetc" --help >/dev/null 2>&1 || runs=$?
# Another OS's artifact, or a DMG for another architecture (no Rosetta), cannot run here.
if [[ "$host" != "$platform" || ( $runs -eq 126 && "$platform" == macos ) ]]; then
    printf 'skipping the %s prep-kit plan: %s does not run on this host\n' "$platform" "$assetc"
elif plan="$("$assetc" prepare --kit "$(native "$kit")" --workspace "$(native "$scratch/workspace")" \
    --out "$(native "$scratch/out")" --check 2>&1)"; then
    printf '%s prep kit satisfies its compiler: %s\n' "$platform" "$plan"
else
    printf 'the shipped compiler cannot plan from the %s prep kit:\n%s\n' "$platform" "$plan" >&2
    status=1
fi
if [[ "$platform" == windows && "$host" == windows ]]; then
    while IFS= read -r name; do
        pwsh -NoProfile -File "$(native "$here/windows/execution-level.ps1")" -Executable "$(native "$root/$prefix$name.exe")" || status=1
    done < <(runtime_binaries)
    pwsh -NoProfile -File "$(native "$here/windows/execution-level.ps1")" -Executable "$(native "$assetc")" || status=1
fi
rm -rf "$scratch"
exit $status
