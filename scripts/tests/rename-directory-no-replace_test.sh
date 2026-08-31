#!/usr/bin/env bash
set -euo pipefail

# Direct contract test for the native atomic publisher. The executable is
# compiled only inside a temporary directory and is always removed by the trap.
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
source_path="${1:-$script_dir/../rename-directory-no-replace.c}"
fetcher_path="${2:-$script_dir/../fetch-vanilla-assets.sh}"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/cinnabar-rename-test.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM

grep -Fq 'mktemp -d "${TMPDIR:-/tmp}/cinnabar-rename-no-replace.XXXXXX"' "$fetcher_path"
grep -Fq 'publisher_binary="$publisher_work/publisher"' "$fetcher_path"
if grep -Fq 'chmod 700 -- "$publisher_work"' "$fetcher_path"; then
    printf '%s\n' 'fetcher uses a GNU-only chmod option terminator' >&2
    exit 1
fi
if grep -Fq 'rm -f -- "$publisher_binary"' "$fetcher_path"; then
    printf '%s\n' 'fetcher removes and reuses the atomic publisher leaf path' >&2
    exit 1
fi

cc -std=c11 -O2 -Wall -Wextra -Werror "$source_path" -o "$scratch/publisher"

mkdir "$scratch/source-success"
printf '%s\n' staged > "$scratch/source-success/staged.txt"
"$scratch/publisher" "$scratch/source-success" "$scratch/destination-success"
[[ -f "$scratch/destination-success/staged.txt" ]]
[[ ! -e "$scratch/source-success" ]]

mkdir "$scratch/source-race" "$scratch/destination-race"
printf '%s\n' staged > "$scratch/source-race/staged.txt"
printf '%s\n' winner > "$scratch/destination-race/winner.txt"
set +e
race_output="$("$scratch/publisher" "$scratch/source-race" "$scratch/destination-race" 2>&1)"
race_status=$?
set -e
[[ "$race_status" -eq 3 ]]
[[ "$race_output" == *'cache directory appeared during extraction'* ]]
[[ -f "$scratch/source-race/staged.txt" ]]
[[ -f "$scratch/destination-race/winner.txt" ]]
[[ ! -e "$scratch/destination-race/source-race" ]]

mkdir "$scratch/source-file-race"
printf '%s\n' staged > "$scratch/source-file-race/staged.txt"
printf '%s\n' winner > "$scratch/destination-file-race"
set +e
file_race_output="$("$scratch/publisher" "$scratch/source-file-race" "$scratch/destination-file-race" 2>&1)"
file_race_status=$?
set -e
[[ "$file_race_status" -eq 3 ]]
[[ "$file_race_output" == *'cache directory appeared during extraction'* ]]
[[ -f "$scratch/source-file-race/staged.txt" ]]
[[ "$(cat "$scratch/destination-file-race")" == winner ]]

printf '%s\n' 'rename-directory-no-replace tests passed'
