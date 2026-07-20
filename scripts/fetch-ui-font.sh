#!/usr/bin/env bash
set -euo pipefail

dry_run=false
for argument in "$@"; do
    case "$argument" in
        --dry-run) dry_run=true ;;
        *) printf 'unknown argument: %s\n' "$argument" >&2; exit 2 ;;
    esac
done

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_root="$(CDPATH= cd -- "$script_dir/.." && pwd)"
manifest="$repo_root/assets/ui-font-source.json"

manifest_string() {
    local key="$1" value
    value="$(sed -n 's/^[[:space:]]*"'"$key"'"[[:space:]]*:[[:space:]]*"\([^"]*\)".*$/\1/p' "$manifest" | head -n 1)"
    if [[ -z "$value" ]]; then printf "font manifest is missing '%s'\n" "$key" >&2; exit 1; fi
    printf '%s' "$value"
}

manifest_integer() {
    local key="$1" value
    value="$(sed -n 's/^[[:space:]]*"'"$key"'"[[:space:]]*:[[:space:]]*\([0-9][0-9]*\).*$/\1/p' "$manifest" | head -n 1)"
    if [[ ! "$value" =~ ^[0-9]+$ || "$value" -le 0 ]]; then
        printf "font manifest has invalid '%s'\n" "$key" >&2; exit 1
    fi
    printf '%s' "$value"
}

[[ -f "$manifest" ]] || { printf 'font source manifest is missing: %s\n' "$manifest" >&2; exit 1; }
commit="$(manifest_string commit)"
policy="$(manifest_string artifact_policy)"
font_file="$(manifest_string font_file)"
font_url="$(manifest_string font_url)"
font_size="$(manifest_integer font_size_bytes)"
font_sha="$(manifest_string font_sha256 | tr '[:upper:]' '[:lower:]')"
license_file="$(manifest_string license_file)"
license_url="$(manifest_string license_url)"
license_size="$(manifest_integer license_size_bytes)"
license_sha="$(manifest_string license_sha256 | tr '[:upper:]' '[:lower:]')"

[[ "$policy" == local-source-cache ]] || { printf 'font artifact policy must be local-source-cache\n' >&2; exit 1; }
[[ "$commit" =~ ^[0-9a-f]{40}$ ]] || { printf 'font commit is invalid\n' >&2; exit 1; }
for basename in "$font_file" "$license_file"; do
    case "$basename" in ''|.|..|*/*|*\\*) printf 'font install path must be one basename: %s\n' "$basename" >&2; exit 1 ;; esac
done
for url in "$font_url" "$license_url"; do
    case "$url" in https://raw.githubusercontent.com/*) ;; *) printf 'font source URL is not approved HTTPS: %s\n' "$url" >&2; exit 1 ;; esac
done

cache="$repo_root/.local/assets/ui-font/$commit"
font_path="$cache/$font_file"
license_path="$cache/$license_file"
printf 'Manifest: %s\nFont source: %s\nLicense source: %s\nCache: %s\n' "$manifest" "$font_url" "$license_url" "$cache"
if [[ "$dry_run" == true ]]; then exit 0; fi

command -v curl >/dev/null 2>&1 || { printf 'required command is unavailable: curl\n' >&2; exit 1; }
hash_file() {
    if command -v sha256sum >/dev/null 2>&1; then sha256sum "$1" | awk '{print tolower($1)}'
    elif command -v shasum >/dev/null 2>&1; then shasum -a 256 "$1" | awk '{print tolower($1)}'
    else printf 'required SHA-256 command is unavailable\n' >&2; exit 1; fi
}
verify_file() {
    local path="$1" size="$2" sha="$3"
    [[ -f "$path" ]] && [[ "$(wc -c < "$path" | tr -d ' ')" == "$size" ]] && [[ "$(hash_file "$path")" == "$sha" ]]
}
download_file() {
    local url="$1" path="$2" size="$3" sha="$4" partial="$2.partial.$$"
    if verify_file "$path" "$size" "$sha"; then return; fi
    rm -f -- "$partial"
    curl --fail --location --output "$partial" "$url"
    if ! verify_file "$partial" "$size" "$sha"; then rm -f -- "$partial"; printf 'font source size or SHA-256 mismatch: %s\n' "$url" >&2; exit 1; fi
    mv -f -- "$partial" "$path"
}

mkdir -p -- "$cache"
download_file "$font_url" "$font_path" "$font_size" "$font_sha"
download_file "$license_url" "$license_path" "$license_size" "$license_sha"
printf 'FONT_SOURCE_PATH=%s\nFONT_LICENSE_PATH=%s\n' "$font_path" "$license_path"
