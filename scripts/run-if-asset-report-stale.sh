#!/usr/bin/env bash
set -euo pipefail

report=$1
carrier=$2

if [[ -f "$report" && ! "$report" -ot "$carrier" ]]; then
    exit 0
fi
exit 1
