#!/usr/bin/env bash
set -euo pipefail

# Direct contract test for the native atomic publisher. The executable is
# compiled only inside a temporary directory and is always removed by the trap.
script_dir="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
source_path="${1:-$script_dir/../rename-directory-no-replace.c}"
scratch="$(mktemp -d "${TMPDIR:-/tmp}/cinnabar-rename-test.XXXXXX")"
trap 'rm -rf -- "$scratch"' EXIT HUP INT TERM

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

printf '%s\n' 'rename-directory-no-replace tests passed'
