#!/usr/bin/env bash
# Signs and notarizes a .app or .dmg. Usage: sign-notarize.sh <path/to/Cinnabar.app|Cinnabar.dmg>
# Env (never committed): CODESIGN_IDENTITY, and either NOTARY_PROFILE (notarytool keychain profile)
# or APPLE_ID + APPLE_TEAM_ID + APPLE_APP_PASSWORD. Without CODESIGN_IDENTITY the app is ad-hoc
# signed and nothing is notarized, so recipients must clear Gatekeeper themselves.
set -euo pipefail
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
# shellcheck source=../common/stage-payload.sh
source "$here/../common/stage-payload.sh"
target="${1:?usage: sign-notarize.sh <app-or-dmg>}"

# helper_binaries <app>: listed runtime executables except the bundle's own, which signing the
# bundle covers. Signing that executable first would reject the still-unsigned helpers beside it.
helper_binaries() {
    local main
    require_file "$runtime_binary_list" 'restore the runtime binary table'
    main="$(/usr/libexec/PlistBuddy -c 'Print :CFBundleExecutable' "$1/Contents/Info.plist")"
    runtime_binaries | grep -vxF -- "$main"
}

if [[ -z "${CODESIGN_IDENTITY:-}" ]]; then
    case "$target" in
        *.app)
            # Apple Silicon refuses unsigned Mach-O, and cross-built x86_64 executables carry no
            # linker signature, so every helper is signed before the bundle.
            while IFS= read -r -d '' file; do
                if [[ "$(file -b "$file")" == Mach-O* ]]; then codesign --force --sign - "$file"; fi
            done < <(find "$target/Contents/Resources" -type f -perm -u+x -print0)
            while IFS= read -r name; do
                codesign --force --sign - "$target/Contents/MacOS/$name"
            done < <(helper_binaries "$target")
            codesign --force --deep --sign - "$target"
            codesign --verify --deep --strict --verbose=2 "$target"
            echo "ad-hoc signed $target (not notarized)"
            ;;
        *.dmg) echo "skipping DMG signing: no CODESIGN_IDENTITY" ;;
        *) echo "unsupported target: $target" >&2; exit 2 ;;
    esac
    exit 0
fi

notary_args=()
if [[ -n "${NOTARY_PROFILE:-}" ]]; then
    notary_args=(--keychain-profile "$NOTARY_PROFILE")
else
    : "${APPLE_ID:?set NOTARY_PROFILE or APPLE_ID/APPLE_TEAM_ID/APPLE_APP_PASSWORD}"
    : "${APPLE_TEAM_ID:?}" "${APPLE_APP_PASSWORD:?}"
    notary_args=(--apple-id "$APPLE_ID" --team-id "$APPLE_TEAM_ID" --password "$APPLE_APP_PASSWORD")
fi

sign() { codesign --force --timestamp --options runtime --sign "$CODESIGN_IDENTITY" "$@"; }

case "$target" in
    *.app)
        # Nested code first (helpers under Resources, then the bundle's own executables), bundle last.
        while IFS= read -r -d '' file; do
            if [[ "$(file -b "$file")" == Mach-O* ]]; then sign "$file"; fi
        done < <(find "$target/Contents/Resources" -type f -perm -u+x -print0)
        while IFS= read -r name; do
            sign "$target/Contents/MacOS/$name"
        done < <(helper_binaries "$target")
        sign "$target"
        codesign --verify --deep --strict --verbose=2 "$target"
        archive="$(mktemp -d)/Cinnabar.zip"
        ditto -c -k --keepParent "$target" "$archive"
        xcrun notarytool submit "$archive" "${notary_args[@]}" --wait
        xcrun stapler staple "$target"
        spctl --assess --type execute --verbose "$target"
        ;;
    *.dmg)
        sign "$target"
        xcrun notarytool submit "$target" "${notary_args[@]}" --wait
        xcrun stapler staple "$target"
        spctl --assess --type open --context context:primary-signature --verbose "$target"
        ;;
    *) echo "unsupported target: $target" >&2; exit 2 ;;
esac
