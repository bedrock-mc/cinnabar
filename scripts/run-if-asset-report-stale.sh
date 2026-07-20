#!/usr/bin/env bash
set -euo pipefail

report=$1
carrier=$2
shift 2

if [[ -f "$report" && ! "$report" -ot "$carrier" ]]; then
    exit 0
fi

if (( $# == 0 )); then
    echo "asset report recovery command is empty" >&2
    exit 1
fi

exec "$@"
