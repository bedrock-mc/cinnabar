#!/usr/bin/env bash
set -euo pipefail

# VPA-209 provisional extraction bounds.
#
# PROVISIONAL-UNLESS-MEASURED: these constants carry explicit headroom over
# the pinned pack inventory measured locally from the exact pinned
# bedrock-samples v1.26.30.32-preview-full artifact (payload stays outside
# git): 21493 entries (620 directory + 20873 file), 297074336 bytes total
# expanded, largest single file 3460610 bytes, largest legitimate per-entry
# compression ratio about 120.5 (a .tga texture), aggregate compression
# ratio 1.99. Raise a bound only after re-measuring a newer pinned
# inventory; never loosen them to admit an unmeasured archive. These values
# MUST stay identical to scripts/fetch-vanilla-assets.ps1.
readonly DEFAULT_MAX_ARCHIVE_ENTRIES=65536
readonly DEFAULT_MAX_EXPANDED_FILE_BYTES=67108864        # 64 MiB
readonly DEFAULT_MAX_TOTAL_EXPANDED_BYTES=1073741824     # 1 GiB
readonly MIN_RATIO_SAMPLE_COMPRESSED_BYTES=4096
readonly DEFAULT_MAX_PER_ENTRY_COMPRESSION_RATIO=500
readonly DEFAULT_MAX_AGGREGATE_COMPRESSION_RATIO=100

# Staging reclamation policy for runs interrupted by process death (SIGKILL,
# power loss). Provisional-unless-measured. Must stay identical to the
# PowerShell script's StaleStagingMaxAgeSeconds/StaleStagingMaxRemaining.
readonly STALE_STAGING_MAX_AGE_SECONDS=86400             # 24 hours
readonly STALE_STAGING_MAX_REMAINING=4

accept_eula=false
dry_run=false
# VPA-209 additive tightening-only test overrides (0/empty = built-in default).
max_archive_entries_override=0
max_expanded_file_bytes_override=0
max_total_expanded_bytes_override=0
max_per_entry_compression_ratio_override=""
max_aggregate_compression_ratio_override=""

require_nonnegative_integer() {
    local name="$1" value="$2"
    case "$value" in
        ''|*[!0-9]*)
            printf 'invalid %s value: %s\n' "$name" "$value" >&2
            exit 1
            ;;
    esac
}

require_nonnegative_number() {
    local name="$1" value="$2"
    if ! [[ "$value" =~ ^[0-9]+(\.[0-9]+)?$ ]]; then
        printf 'invalid %s value: %s\n' "$name" "$value" >&2
        exit 1
    fi
}

# Compares nonnegative decimal integer strings without shell arithmetic, whose
# signed machine-word conversion can wrap attacker-sized CLI values. Prints -1,
# 0, or 1 when the left operand is less than, equal to, or greater than right.
compare_decimal_integers() {
    local LC_ALL=C
    local left="$1" right="$2"
    while [[ "${left#0}" != "$left" ]]; do left="${left#0}"; done
    while [[ "${right#0}" != "$right" ]]; do right="${right#0}"; done
    [[ -n "$left" ]] || left=0
    [[ -n "$right" ]] || right=0
    if [[ "${#left}" -lt "${#right}" ]]; then
        printf '%s\n' -1
    elif [[ "${#left}" -gt "${#right}" ]]; then
        printf '%s\n' 1
    elif [[ "$left" == "$right" ]]; then
        printf '%s\n' 0
    elif [[ "$left" < "$right" ]]; then
        printf '%s\n' -1
    else
        printf '%s\n' 1
    fi
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        --accept-eula) accept_eula=true ;;
        --dry-run) dry_run=true ;;
        --max-archive-entries=*)
            max_archive_entries_override="${1#*=}"
            require_nonnegative_integer "max-archive-entries" "$max_archive_entries_override"
            ;;
        --max-expanded-file-bytes=*)
            max_expanded_file_bytes_override="${1#*=}"
            require_nonnegative_integer "max-expanded-file-bytes" "$max_expanded_file_bytes_override"
            ;;
        --max-total-expanded-bytes=*)
            max_total_expanded_bytes_override="${1#*=}"
            require_nonnegative_integer "max-total-expanded-bytes" "$max_total_expanded_bytes_override"
            ;;
        --max-per-entry-compression-ratio=*)
            max_per_entry_compression_ratio_override="${1#*=}"
            require_nonnegative_number "max-per-entry-compression-ratio" "$max_per_entry_compression_ratio_override"
            ;;
        --max-aggregate-compression-ratio=*)
            max_aggregate_compression_ratio_override="${1#*=}"
            require_nonnegative_number "max-aggregate-compression-ratio" "$max_aggregate_compression_ratio_override"
            ;;
        *)
            printf 'unknown argument: %s\n' "$1" >&2
            exit 2
            ;;
    esac
    shift
done

resolve_tightened_long() {
    # Echoes the effective bound. Overrides may only tighten.
    local name="$1" default_value="$2" override="$3"
    local zero_comparison default_comparison
    zero_comparison="$(compare_decimal_integers "$override" 0)"
    if [[ "$zero_comparison" == 0 ]]; then
        printf '%s\n' "$default_value"
        return 0
    fi
    default_comparison="$(compare_decimal_integers "$override" "$default_value")"
    if [[ "$default_comparison" == 1 ]]; then
        printf -- '--%s %s exceeds the built-in maximum %s; overrides may only tighten bounds\n' \
            "$name" "$override" "$default_value" >&2
        exit 1
    fi
    printf '%s\n' "$override"
}

resolve_tightened_number() {
    local name="$1" default_value="$2" override="$3"
    if [[ -z "$override" ]]; then
        printf '%s\n' "$default_value"
        return 0
    fi
    if LC_ALL=C awk -v o="$override" -v d="$default_value" 'BEGIN { exit !(o + 0 > d + 0) }'; then
        printf -- '--%s %s exceeds the built-in maximum %s; overrides may only tighten bounds\n' \
            "$name" "$override" "$default_value" >&2
        exit 1
    fi
    printf '%s\n' "$override"
}

effective_max_archive_entries="$(resolve_tightened_long max-archive-entries "$DEFAULT_MAX_ARCHIVE_ENTRIES" "$max_archive_entries_override")"
effective_max_expanded_file_bytes="$(resolve_tightened_long max-expanded-file-bytes "$DEFAULT_MAX_EXPANDED_FILE_BYTES" "$max_expanded_file_bytes_override")"
effective_max_total_expanded_bytes="$(resolve_tightened_long max-total-expanded-bytes "$DEFAULT_MAX_TOTAL_EXPANDED_BYTES" "$max_total_expanded_bytes_override")"
effective_max_per_entry_ratio="$(resolve_tightened_number max-per-entry-compression-ratio "$DEFAULT_MAX_PER_ENTRY_COMPRESSION_RATIO" "$max_per_entry_compression_ratio_override")"
effective_max_aggregate_ratio="$(resolve_tightened_number max-aggregate-compression-ratio "$DEFAULT_MAX_AGGREGATE_COMPRESSION_RATIO" "$max_aggregate_compression_ratio_override")"

if [[ "$accept_eula" != true ]]; then
    printf 'Refusing to fetch Mojang assets without the explicit --accept-eula flag.\n' >&2
    exit 2
fi

script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo_root="$(CDPATH= cd -- "$script_dir/.." && pwd)"
manifest_path="$repo_root/assets/vanilla-source.json"

manifest_string() {
    local key="$1"
    local allow_empty="${2:-false}"
    local value
    value="$(sed -n 's/^[[:space:]]*"'"$key"'"[[:space:]]*:[[:space:]]*"\([^"]*\)".*$/__manifest_value__\1/p' "$manifest_path" | head -n 1)"
    if [[ "$value" != __manifest_value__* ]]; then
        printf "vanilla source manifest is missing '%s'\n" "$key" >&2
        exit 1
    fi
    value="${value#__manifest_value__}"
    if [[ -z "$value" && "$allow_empty" != true ]]; then
        printf "vanilla source manifest is missing '%s'\n" "$key" >&2
        exit 1
    fi
    printf '%s' "$value"
}

if [[ ! -f "$manifest_path" ]]; then
    printf 'vanilla source manifest is missing: %s\n' "$manifest_path" >&2
    exit 1
fi

archive="$(manifest_string archive true)"
url="$(manifest_string url)"
expected_sha256="$(manifest_string sha256 | tr '[:upper:]' '[:lower:]')"
artifact_policy="$(manifest_string artifact_policy)"
cache_relative="$(manifest_string cache_dir)"
case "$archive" in
    ''|.|..|[A-Za-z]:*|*/*|*\\*)
        printf 'archive must be exactly one nonempty basename\n' >&2
        exit 1
        ;;
esac
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
listing_work=''
publisher_work=''
publisher_work_identity=''
publisher_binary=''

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

reclaim_stale_staging() {
    # VPA-209: reclaim staging directories abandoned by interrupted runs.
    # Only siblings of the cache path whose name continues this script's own
    # ".extracting" marker are considered; they are reclaimed when older
    # than STALE_STAGING_MAX_AGE_SECONDS, or when more than
    # STALE_STAGING_MAX_REMAINING fresher leftovers exist (oldest deleted
    # first). Entries whose age cannot be determined are treated as fresh
    # and only fall to the count bound.
    local prefix ages now mtime age reclaimed kept candidate
    prefix="${cache_path##*/}.extracting"
    [[ -d "$cache_parent" ]] || return 0
    ages="$(mktemp "${TMPDIR:-/tmp}/cinnabar-stale.XXXXXX")" || return 0
    now="$(date +%s)"
    while IFS= read -r candidate; do
        mtime="$(stat -c %Y -- "$candidate" 2>/dev/null || stat -f %m -- "$candidate" 2>/dev/null || true)"
        case "$mtime" in
            ''|*[!0-9]*) age=0 ;;
            *) age=$((now - mtime)); [[ "$age" -ge 0 ]] || age=0 ;;
        esac
        printf '%s\t%s\n' "$age" "$candidate"
    done < <(find "$cache_parent" -maxdepth 1 -type d -name "${prefix}*" 2>/dev/null) > "$ages"
    reclaimed=0
    kept=0
    while IFS=$'\t' read -r age candidate; do
        if [[ "$kept" -lt "$STALE_STAGING_MAX_REMAINING" && "$age" -lt "$STALE_STAGING_MAX_AGE_SECONDS" ]]; then
            kept=$((kept + 1))
            continue
        fi
        case "$candidate" in
            "$cache_parent/$prefix"*)
                rm -rf -- "$candidate" && reclaimed=$((reclaimed + 1))
                ;;
            *)
                printf 'refusing to clean unexpected staging path: %s\n' "$candidate" >&2
                ;;
        esac
    done < <(sort -n "$ages")
    # sort -n walks youngest-first so the kept set is the freshest
    # STALE_STAGING_MAX_REMAINING leftovers; older ones are reclaimed. This
    # mirrors the PowerShell script's descending LastWriteTimeUtc ordering.
    rm -f -- "$ages"
    if [[ "$reclaimed" -gt 0 ]]; then
        printf 'Reclaimed %s stale extraction staging director(y/ies)\n' "$reclaimed"
    fi
}

# Returns a stable device/inode identity for a directory on GNU or BSD/macOS.
# Cleanup uses it to avoid recursively removing a substituted path.
directory_identity() {
    local path="$1" identity
    identity="$(stat -c '%d:%i' -- "$path" 2>/dev/null || stat -f '%d:%i' -- "$path" 2>/dev/null)" ||
        return 1
    printf '%s\n' "$identity"
}

cleanup_extract() {
    if [[ -n "${temporary_extract:-}" && -d "$temporary_extract" ]]; then
        case "$temporary_extract" in
            "$cache_parent"/*.extracting.*) rm -rf -- "$temporary_extract" ;;
            *) printf 'refusing to clean unexpected extraction path: %s\n' "$temporary_extract" >&2 ;;
        esac
    fi
    if [[ -n "${listing_work:-}" && -d "$listing_work" ]]; then
        case "$listing_work" in
            */cinnabar-zipcheck.*) rm -rf -- "$listing_work" ;;
        esac
    fi
    if [[ -n "${publisher_work:-}" && -d "$publisher_work" && ! -L "$publisher_work" ]]; then
        local current_identity=''
        current_identity="$(directory_identity "$publisher_work" || true)"
        case "$publisher_work" in
            */cinnabar-rename-no-replace.*)
                if [[ -n "$publisher_work_identity" && "$current_identity" == "$publisher_work_identity" ]]; then
                    rm -rf -- "$publisher_work"
                else
                    printf 'refusing to clean substituted publisher workspace: %s\n' "$publisher_work" >&2
                fi
                ;;
        esac
    elif [[ -n "${publisher_work:-}" && ( -e "$publisher_work" || -L "$publisher_work" ) ]]; then
        printf 'refusing to clean non-directory publisher workspace: %s\n' "$publisher_work" >&2
    fi
}
trap cleanup_extract EXIT HUP INT TERM

reclaim_stale_staging

if [[ -f "$normalized_source" ]]; then
    printf 'Vanilla source is already available: %s\n' "$normalized_source"
    exit 0
fi
if [[ -e "$cache_path" ]]; then
    printf 'cache directory exists without resource_pack/blocks.json: %s\n' "$cache_path" >&2
    exit 1
fi

for command_name in curl unzip od awk find stat cc; do
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

fatal() {
    printf '%s\n' "$*" >&2
    exit 1
}

publisher_source="$script_dir/rename-directory-no-replace.c"
if [[ ! -f "$publisher_source" ]]; then
    fatal "atomic publication helper source is missing: $publisher_source"
fi
publisher_work="$(mktemp -d "${TMPDIR:-/tmp}/cinnabar-rename-no-replace.XXXXXX")" ||
    fatal 'temporary atomic publication helper workspace unavailable'
chmod 700 "$publisher_work" || fatal 'atomic publication helper workspace permissions failed'
publisher_work_identity="$(directory_identity "$publisher_work")" ||
    fatal 'atomic publication helper workspace identity unavailable'
publisher_binary="$publisher_work/publisher"
if ! cc -std=c11 -O2 -Wall -Wextra -Werror "$publisher_source" -o "$publisher_binary"; then
    fatal 'atomic no-replace directory publication helper could not be compiled'
fi

validate_entry_name() {
    # VPA-209: mirrors the PowerShell extractor's per-entry safety rules in
    # the same order so both platforms reject identical entries with
    # identical diagnostics. Arguments: raw entry name, declared expanded
    # length, staging root, kinds output file. Exits on the first violation.
    local raw="$1" declared_length="$2" staging_root="$3" kinds_file="$4"
    local normalized part parts base lower cumulative invalid_re ctrl_re
    local is_directory=false

    # 1. empty or whitespace-only (NUL was already rejected globally).
    if [[ -z "${raw//[[:space:]]/}" ]]; then
        fatal "unsafe ZIP entry '$raw': path is empty or contains a null character"
    fi
    # 2. absolute/UNC.
    case "$raw" in
        /*|\\*)
            fatal "unsafe ZIP entry '$raw': absolute and UNC paths are not allowed"
            ;;
    esac
    # 3. normalize separators; reject empty components introduced either way.
    normalized="${raw//\\//}"
    case "$normalized" in
        *//*)
            fatal "unsafe ZIP entry '$raw': empty path components are not allowed"
            ;;
    esac
    # 4. directory entries must be empty.
    case "$normalized" in
        */) is_directory=true ;;
    esac
    if [[ "$is_directory" == true && "$declared_length" -ne 0 ]]; then
        fatal "unsafe ZIP entry '$raw': directory entries must be empty"
    fi
    # 5. trim the single trailing separator; whitespace-only remains empty.
    normalized="${normalized%/}"
    if [[ -z "${normalized//[[:space:]]/}" ]]; then
        fatal "unsafe ZIP entry '$raw': path is empty"
    fi
    # 6. per-component rules.
    invalid_re='["<>|*?]'
    ctrl_re="$(printf '[\001-\037]')"
    IFS='/' read -r -a parts <<< "$normalized"
    cumulative=''
    relative=''
    for part in "${parts[@]}"; do
        if [[ -z "$part" ]]; then
            fatal "unsafe ZIP entry '$raw': empty path components are not allowed"
        fi
        if [[ "$part" == "." || "$part" == ".." ]]; then
            fatal "unsafe ZIP entry '$raw': traversal components are not allowed"
        fi
        case "$part" in
            *:*)
                fatal "unsafe ZIP entry '$raw': drive and alternate-stream paths are not allowed"
                ;;
        esac
        if [[ "$part" =~ $invalid_re || "$part" =~ $ctrl_re ||
              "$part" == *" " || "$part" == *. ]]; then
            fatal "invalid filename component '$part'"
        fi
        # e. reserved Windows device names (base name before any extension).
        # declare -l lowercases on assignment without a subprocess spawn;
        # this loop runs once per archive entry.
        local base lower
        base="${part%%.*}"
        declare -l lower="$base"
        if [[ "$lower" =~ ^(con|prn|aux|nul|com[1-9]|lpt[1-9])$ ]]; then
            fatal "unsafe ZIP entry '$raw': reserved filename component '$part'"
        fi
        if [[ -z "$cumulative" ]]; then
            cumulative="$part"
        else
            cumulative="$cumulative/$part"
        fi
        # Ancestors are directories; the leaf takes the entry's own shape.
        local kind="d"
        if [[ "$part" == "${parts[${#parts[@]}-1]}" && "$is_directory" == false ]]; then
            kind="f"
        elif [[ "$part" == "${parts[${#parts[@]}-1]}" ]]; then
            # Explicit directory entries are distinct archive members. Keep
            # that fact separate from repeated implicit ancestor discovery so
            # duplicate explicit directories match PowerShell's rejection.
            kind="x"
        fi
        printf '%s\t%s\n' "$kind" "$cumulative" >> "$kinds_file"
    done
    # Containment belt-and-braces: the sanitized relative path must resolve
    # inside the staging root.
    if ! [[ "$staging_root/$cumulative" == "$staging_root"/* ]]; then
        fatal "unsafe ZIP entry '$raw': path escapes the extraction root"
    fi
}

parse_and_check_listing() {
    # VPA-209: enforce declared byte bounds and compression-ratio bomb guards
    # against the central directory BEFORE anything is extracted. Emits
    # "length<TAB>compressed<TAB>name" rows plus one final
    # "TOTALS<TAB>expanded<TAB>compressed" summary line.
    local verbose_file="$1"
    LC_ALL=C awk \
        -v max_file="$effective_max_expanded_file_bytes" \
        -v max_total="$effective_max_total_expanded_bytes" \
        -v min_sample="$MIN_RATIO_SAMPLE_COMPRESSED_BYTES" \
        -v ratio_pe="$effective_max_per_entry_ratio" '
        function fail(msg) {
            printf "%s\n", msg > "/dev/stderr"
            bad = 1
            exit 3
        }
        NR <= 3 { next }
        # Separator lines are pure dashes and whitespace; Info-ZIP prints two
        # of them around the summary, and the lower one contains internal
        # runs of spaces between dash groups.
        /^-[[:space:]-]*$/ && /-/ { next }
        {
            if (match($0, /^[[:space:]]*[0-9]+[[:space:]]+[^[:space:]]+[[:space:]]+[0-9]+[[:space:]]+-?[0-9]+%[[:space:]]+([0-9]{4}-[0-9]{2}-[0-9]{2}|[0-9]{2}-[0-9]{2}-[0-9]{4})[[:space:]]+[0-9]{2}:[0-9]{2}(:[0-9]{2})?[[:space:]]+[0-9A-Fa-f]{8}[[:space:]]{2}/)) {
                # Save the prefix length immediately: later match() calls on
                # sub-strings overwrite RSTART/RLENGTH.
                prefix_length = RLENGTH
                line = $0
                match(line, /^[[:space:]]*[0-9]+/)
                len = substr(line, RSTART, RLENGTH) + 0
                rest = substr(line, RLENGTH + 1)
                sub(/^[[:space:]]+/, "", rest)
                match(rest, /^[^[:space:]]+/)
                rest = substr(rest, RLENGTH + 1)
                sub(/^[[:space:]]+/, "", rest)
                match(rest, /^[0-9]+/)
                comp = substr(rest, RSTART, RLENGTH) + 0
                # Everything after the two-space gap following CRC is the raw
                # entry name, spaces preserved.
                name = substr($0, prefix_length + 1)
                isdir = (name ~ /\/$/) ? 1 : 0
                if (!isdir) {
                    if (len > max_file + 0) {
                        fail("ZIP entry \047" name "\047 declared expanded size " len " exceeds the maximum " max_file " bytes")
                    }
                    if (comp >= min_sample + 0 && len + 0 > ratio_pe * comp) {
                        fail("ZIP entry \047" name "\047 compression ratio " len ":" comp " exceeds the per-entry maximum " ratio_pe)
                    }
                }
                total_len += len
                total_comp += comp
                if (total_len > max_total + 0) {
                    fail("archive total declared expanded size " total_len " exceeds the maximum " max_total " bytes")
                }
                printf "%d\t%d\t%s\n", len, comp, name
                next
            }
            if ($0 ~ /[[:space:]][0-9]+ files?[[:space:]]*$/) { next }
            fail("archive listing could not be parsed consistently")
        }
        END {
            if (!bad) {
                printf "TOTALS\t%d\t%d\n", total_len, total_comp
            }
        }
        ' "$verbose_file"
}

reject_link_entries() {
    # VPA-209: reject symlink entries from central-directory metadata BEFORE
    # anything is extracted. Info-ZIP unzip restores Unix symlinks on
    # Linux/macOS, so extracting first and auditing afterwards lets children
    # of a directory symlink be written through the link outside staging
    # while staging itself stays audit-clean. This mirrors the PowerShell
    # extractor's S_IFLNK check against each entry's external attributes so
    # both platforms reject identical entries before any byte is written;
    # the post-extraction special-node scan remains defense-in-depth.
    #
    # Detection keys on the zipinfo long-listing mode column: every
    # Unix-made entry begins with an ls-style type character, and only
    # symlinks render with a leading "l" (regular files "-", directories
    # "d"). Framing lines ("Archive:", "Zip file size:", the trailing
    # files/bytes summary) never begin with an l-token followed by a
    # made-by version, so they cannot false-positive.
    local archive="$1"
    unzip -Zl "$archive" > "$listing_work/zlist" 2>/dev/null ||
        fatal "archive listing unavailable: $archive"
    if [[ ! -s "$listing_work/zlist" ]]; then
        fatal 'archive listing could not be parsed consistently'
    fi
    LC_ALL=C awk '
        /^[ \t]*l[^ \t]*[ \t]+[0-9]+\.[0-9]+[ \t]/ {
            name = $0
            # Recover the raw entry name: everything after the date/time
            # columns. Seconds are optional across zipinfo builds.
            if (match($0, /[ \t][0-9][0-9]-[A-Za-z][A-Za-z][A-Za-z]-[0-9][0-9]([0-9][0-9])?[ \t]+[0-9][0-9]:[0-9][0-9](:[0-9][0-9])?[ \t]+/)) {
                name = substr($0, RSTART + RLENGTH)
            }
            sub(/^[ \t]+/, "", name)
            printf "unsafe ZIP entry \047%s\047: link entries are not allowed\n", name > "/dev/stderr"
            exit 3
        }
        ' "$listing_work/zlist" || fatal 'unsafe ZIP link entries rejected before extraction'
}

validate_archive_bounds() {
    # VPA-209: pre-extraction validation of the verified archive: entry count,
    # NUL-free names, strict central-directory bounds, per-entry name safety,
    # duplicate/collision rejection, and the aggregate bomb ratio.
    local archive="$1"
    listing_work="$(mktemp -d "${TMPDIR:-/tmp}/cinnabar-zipcheck.XXXXXX")" ||
        fatal 'temporary listing workspace unavailable'

    unzip -Z1 "$archive" > "$listing_work/names" 2>/dev/null ||
        fatal "archive listing unavailable: $archive"

    if LC_ALL=C od -An -c "$listing_work/names" | grep -qF '\0'; then
        fatal 'unsafe ZIP entry name contains a null character'
    fi

    local z1_count
    z1_count="$(wc -l < "$listing_work/names" | tr -d '[:space:]')"
    if [[ "$z1_count" -gt "$effective_max_archive_entries" ]]; then
        fatal "archive entry count $z1_count exceeds the maximum $effective_max_archive_entries"
    fi

    unzip -v "$archive" > "$listing_work/verbose" 2>/dev/null ||
        fatal "archive listing unavailable: $archive"

    # The parser prints its own specific diagnostic and exits nonzero when a
    # bound is violated or the listing shape is unexpected.
    local parse_status=0
    parse_and_check_listing "$listing_work/verbose" > "$listing_work/rows" || parse_status=$?
    if [[ "$parse_status" -ne 0 ]]; then
        exit 1
    fi

    grep -v '^TOTALS' "$listing_work/rows" > "$listing_work/entries" || true

    local rows_count totals_line total_declared_expanded total_declared_compressed
    rows_count="$(wc -l < "$listing_work/entries" | tr -d '[:space:]')"
    if [[ "$rows_count" != "$z1_count" ]]; then
        fatal 'archive listing could not be parsed consistently'
    fi
    cut -f3 "$listing_work/entries" | LC_ALL=C sort > "$listing_work/rownames"
    LC_ALL=C sort "$listing_work/names" > "$listing_work/names.sorted"
    if ! cmp -s "$listing_work/rownames" "$listing_work/names.sorted"; then
        fatal 'archive listing could not be parsed consistently'
    fi

    totals_line="$(grep '^TOTALS' "$listing_work/rows" | tail -n 1 || true)"
    if [[ -z "$totals_line" ]]; then
        fatal 'archive listing could not be parsed consistently'
    fi
    total_declared_expanded="$(printf '%s' "$totals_line" | cut -f2)"
    total_declared_compressed="$(printf '%s' "$totals_line" | cut -f3)"

    # Link entries are rejected before per-entry name validation so a
    # directory symlink followed by child members fails with the link
    # diagnostic rather than the derived path collision, matching the
    # PowerShell extractor's per-entry rejection order for this class.
    reject_link_entries "$archive"

    : > "$listing_work/kinds"
    local decl_len decl_comp raw_name
    while IFS=$'\t' read -r decl_len decl_comp raw_name; do
        validate_entry_name "$raw_name" "$decl_len" "$temporary_extract" "$listing_work/kinds"
    done < "$listing_work/entries"

    awk -F'\t' '
        {
            key = $2; kind = $1
            shape = (kind == "f" ? "f" : "d")
            if (key in seen) {
                if (seen[key] != shape) {
                    printf "ZIP entry path collision at \047%s\047\n", key > "/dev/stderr"
                    exit 3
                }
                if (kind == "f" || (kind == "x" && explicit[key])) {
                    printf "duplicate ZIP entry path \047%s\047\n", key > "/dev/stderr"
                    exit 3
                }
                if (kind == "x") { explicit[key] = 1 }
            } else {
                seen[key] = shape
                if (kind == "x") { explicit[key] = 1 }
            }
        }
        ' "$listing_work/kinds" || fatal 'unsafe ZIP entry names'

    # Aggregate bomb ratio. A weighted average can never exceed the largest
    # per-entry ratio, so this guard only fires for distributed bombs whose
    # individual entries each stay under the per-entry threshold.
    if [[ "$total_declared_compressed" -ge "$MIN_RATIO_SAMPLE_COMPRESSED_BYTES" ]]; then
        if LC_ALL=C awk -v l="$total_declared_expanded" -v c="$total_declared_compressed" \
            -v m="$effective_max_aggregate_ratio" 'BEGIN { exit !(l + 0 > m * c) }'; then
            fatal "archive aggregate compression ratio $total_declared_expanded:$total_declared_compressed exceeds the aggregate maximum $effective_max_aggregate_ratio"
        fi
    fi

    # Release the listing workspace before returning: failure paths above exit
    # through the trap while the variable is still set, but this success return
    # is the last point where clearing alone would strand the directory, so
    # removal happens here and no temporary tree outlives validation.
    case "$listing_work" in
        */cinnabar-zipcheck.*)
            rm -rf -- "$listing_work"
            listing_work=''
            ;;
        *)
            printf 'refusing to clean unexpected listing workspace: %s\n' "$listing_work" >&2
            ;;
    esac
}

audit_extracted_tree() {
    # VPA-209: defense-in-depth after extraction, before publication.
    # Symlink entries are already rejected from the central directory before
    # extraction (see reject_link_entries): children of a directory symlink
    # would be written through the link outside staging during extraction,
    # where no later audit could observe them. This scan still rejects any
    # OTHER special node the extractor may materialize on Unix hosts, then
    # re-audits ACTUAL file counts and byte totals against the same bounds
    # in case headers lied.
    local node rel sz
    while IFS= read -r -d '' node; do
        rel="${node#"$temporary_extract"/}"
        fatal "unsafe extracted node '$rel': link and special filesystem entries are not allowed"
    done < <(find "$temporary_extract" -mindepth 1 ! -type d ! -type f -print0)

    local actual_files actual_bytes audit_line audit_status
    if find --version >/dev/null 2>&1; then
        # GNU find: one spawn for the whole tree. The awk guard reproduces the
        # per-file and running-total bounds with identical diagnostics.
        actual_files=-1
        actual_bytes=-1
        audit_line=''
        audit_status=0
        audit_line="$(find "$temporary_extract" -type f -printf '%s\t%P\n' | LC_ALL=C awk \
            -v max_file="$effective_max_expanded_file_bytes" \
            -v max_total="$effective_max_total_expanded_bytes" '
            function fail(msg) { printf "%s\n", msg > "/dev/stderr"; bad = 1; exit 3 }
            {
                n++
                sum += $1 + 0
                if ($1 + 0 > max_file + 0) {
                    fail("ZIP entry \047" $2 "\047 expanded size exceeded the maximum " max_file " bytes during extraction")
                }
                if (sum > max_total + 0) {
                    fail("archive total expanded size exceeded the maximum " max_total " bytes during extraction")
                }
            }
            END {
                if (!bad) { printf "%d %d\n", n, sum }
            }')" || audit_status=$?
        if [[ "$audit_status" -ne 0 ]]; then
            exit 1
        fi
        actual_files="${audit_line%% *}"
        actual_bytes="${audit_line#* }"
        if [[ -z "$actual_files" || -z "$actual_bytes" ]]; then
            fatal 'extracted tree audit failed'
        fi
    else
        # Portable fallback (BSD/macOS): stat once per file. Slower but this
        # branch only runs on platforms without GNU find.
        actual_files=0
        actual_bytes=0
        while IFS= read -r -d '' node; do
            rel="${node#"$temporary_extract"/}"
            if ! sz="$(stat -c %s -- "$node" 2>/dev/null)"; then
                if ! sz="$(stat -f %z -- "$node" 2>/dev/null)"; then
                    fatal "extracted file size unavailable: $rel"
                fi
            fi
            actual_files=$((actual_files + 1))
            actual_bytes=$((actual_bytes + sz))
            if [[ "$sz" -gt "$effective_max_expanded_file_bytes" ]]; then
                fatal "ZIP entry '$rel' expanded size exceeded the maximum $effective_max_expanded_file_bytes bytes during extraction"
            fi
            if [[ "$actual_bytes" -gt "$effective_max_total_expanded_bytes" ]]; then
                fatal "archive total expanded size exceeded the maximum $effective_max_total_expanded_bytes bytes during extraction"
            fi
        done < <(find "$temporary_extract" -type f -print0)
    fi
    if [[ "$actual_files" -gt "$effective_max_archive_entries" ]]; then
        fatal "extracted tree file count $actual_files exceeds the maximum $effective_max_archive_entries"
    fi
}

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

validate_archive_bounds "$archive_path"

mkdir -- "$temporary_extract"
unzip -q "$archive_path" -d "$temporary_extract"
audit_extracted_tree

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

# VPA-209: the helper makes the same-volume rename itself conditional on the
# destination still being absent. There is no check/use gap, and an existing
# directory is never interpreted as a request to nest staging inside it.
if ! "$publisher_binary" "$normalized_root" "$cache_path"; then
    exit 1
fi
if [[ "$normalized_root" != "$temporary_extract" ]]; then
    rmdir -- "$temporary_extract"
fi
temporary_extract=''

if [[ ! -f "$normalized_source" ]]; then
    printf 'normalized source was not published: %s\n' "$normalized_source" >&2
    exit 1
fi
printf 'Vanilla source ready: %s\n' "$normalized_source"
