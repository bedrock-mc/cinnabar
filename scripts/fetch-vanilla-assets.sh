#!/usr/bin/env bash
set -euo pipefail

accept_eula=false
dry_run=false
for argument in "$@"; do
    case "$argument" in
        --accept-eula) accept_eula=true ;;
        --dry-run) dry_run=true ;;
        *)
            printf 'unknown argument: %s\n' "$argument" >&2
            exit 2
            ;;
    esac
done

if [[ "$accept_eula" != true ]]; then
    printf 'Refusing to fetch Mojang assets without the explicit --accept-eula flag.\n' >&2
    exit 2
fi

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_root="$(CDPATH= cd -- "$script_dir/.." && pwd)"
manifest_path="$repo_root/assets/vanilla-source.json"

manifest_string() {
    local key="$1"
    local value
    value="$(sed -n 's/^[[:space:]]*"'"$key"'"[[:space:]]*:[[:space:]]*"\([^"]*\)".*$/\1/p' "$manifest_path" | head -n 1)"
    if [[ -z "$value" ]]; then
        printf "vanilla source manifest is missing '%s'\n" "$key" >&2
        exit 1
    fi
    printf '%s' "$value"
}

if [[ ! -f "$manifest_path" ]]; then
    printf 'vanilla source manifest is missing: %s\n' "$manifest_path" >&2
    exit 1
fi

archive="$(manifest_string archive)"
url="$(manifest_string url)"
expected_sha256="$(manifest_string sha256 | tr '[:upper:]' '[:lower:]')"
artifact_policy="$(manifest_string artifact_policy)"
cache_relative="$(manifest_string cache_dir)"
if [[ "$artifact_policy" != local-only ]]; then
    printf "vanilla source manifest must declare artifact_policy 'local-only'\n" >&2
    exit 1
fi
case "$cache_relative" in
    .local/assets/*) ;;
    *)
        printf 'cache_dir must stay below .local/assets: %s\n' "$cache_relative" >&2
        exit 1
        ;;
esac
cache_suffix="${cache_relative#.local/assets/}"
case "/$cache_suffix/" in
    *'/../'*|*'/./'*|*'//'*)
        printf 'cache_dir must not contain empty or traversal components: %s\n' "$cache_relative" >&2
        exit 1
        ;;
esac
case "$cache_suffix" in
    *\\*)
        printf 'cache_dir must use forward-slash path components: %s\n' "$cache_relative" >&2
        exit 1
        ;;
esac

asset_root="$repo_root/.local/assets"
download_directory="$asset_root/downloads"
archive_path="$download_directory/$archive"
partial_path="$archive_path.partial"
cache_path="$repo_root/$cache_relative"
cache_parent="$(dirname -- "$cache_path")"
temporary_extract="$cache_path.extracting.$$"
normalized_source="$cache_path/resource_pack/blocks.json"

printf 'Manifest: %s\n' "$manifest_path"
printf 'Source URL: %s\n' "$url"
printf 'Expected SHA-256: %s\n' "$expected_sha256"
printf 'Partial download: %s\n' "$partial_path"
printf 'Verified archive: %s\n' "$archive_path"
printf 'Temporary extraction: %s\n' "$temporary_extract"
printf 'Cache directory: %s -> %s\n' "$cache_relative" "$cache_path"
printf 'Normalized source: %s\n' "$normalized_source"

if [[ "$dry_run" == true ]]; then
    printf 'DRY-RUN: download, verify, extract, normalize, and atomically publish only to the paths above.\n'
    exit 0
fi

if [[ -f "$normalized_source" ]]; then
    printf 'Vanilla source is already available: %s\n' "$normalized_source"
    exit 0
fi
if [[ -e "$cache_path" ]]; then
    printf 'cache directory exists without resource_pack/blocks.json: %s\n' "$cache_path" >&2
    exit 1
fi

for command_name in curl unzip; do
    if ! command -v "$command_name" >/dev/null 2>&1; then
        printf 'required command is unavailable: %s\n' "$command_name" >&2
        exit 1
    fi
done

hash_file() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | awk '{ print tolower($1) }'
    elif command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{ print tolower($1) }'
    else
        printf 'required SHA-256 command is unavailable (sha256sum or shasum)\n' >&2
        exit 1
    fi
}

cleanup_extract() {
    if [[ -n "${temporary_extract:-}" && -d "$temporary_extract" ]]; then
        case "$temporary_extract" in
            "$cache_parent"/*.extracting.*) rm -rf -- "$temporary_extract" ;;
            *) printf 'refusing to clean unexpected extraction path: %s\n' "$temporary_extract" >&2 ;;
        esac
    fi
}
trap cleanup_extract EXIT HUP INT TERM

mkdir -p -- "$download_directory" "$cache_parent"
archive_verified=false
if [[ -f "$archive_path" ]]; then
    actual_sha256="$(hash_file "$archive_path")"
    if [[ "$actual_sha256" == "$expected_sha256" ]]; then
        archive_verified=true
        printf 'Using verified archive: %s\n' "$archive_path"
    else
        rm -f -- "$archive_path"
    fi
fi

if [[ "$archive_verified" != true ]]; then
    rm -f -- "$partial_path"
    printf 'Downloading %s\n' "$url"
    curl --fail --location --output "$partial_path" "$url"
    actual_sha256="$(hash_file "$partial_path")"
    if [[ "$actual_sha256" != "$expected_sha256" ]]; then
        rm -f -- "$partial_path"
        printf 'SHA-256 mismatch: expected %s, got %s\n' "$expected_sha256" "$actual_sha256" >&2
        exit 1
    fi
    mv -- "$partial_path" "$archive_path"
    printf 'Verified archive SHA-256: %s\n' "$actual_sha256"
fi

mkdir -- "$temporary_extract"
unzip -q "$archive_path" -d "$temporary_extract"

if [[ -f "$temporary_extract/resource_pack/blocks.json" ]]; then
    normalized_root="$temporary_extract"
else
    normalized_root=''
    top_level_count=0
    for candidate in "$temporary_extract"/*; do
        [[ -e "$candidate" ]] || continue
        normalized_root="$candidate"
        top_level_count=$((top_level_count + 1))
    done
    if [[ "$top_level_count" -ne 1 || ! -d "$normalized_root" ]]; then
        printf 'archive must contain exactly one top-level directory\n' >&2
        exit 1
    fi
    if [[ ! -f "$normalized_root/resource_pack/blocks.json" ]]; then
        printf 'archive is missing resource_pack/blocks.json\n' >&2
        exit 1
    fi
fi

mv -- "$normalized_root" "$cache_path"
if [[ "$normalized_root" != "$temporary_extract" ]]; then
    rmdir -- "$temporary_extract"
fi
temporary_extract=''

if [[ ! -f "$normalized_source" ]]; then
    printf 'normalized source was not published: %s\n' "$normalized_source" >&2
    exit 1
fi
printf 'Vanilla source ready: %s\n' "$normalized_source"
