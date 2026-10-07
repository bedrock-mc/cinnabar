#!/bin/bash
# Blocks pattern-based process kills (pkill/killall) run as commands; they mass-killed agents and builds. Kill by PID instead.
cmd=$(python3 -c 'import json,sys; print(json.load(sys.stdin).get("tool_input",{}).get("command",""))' 2>/dev/null)
if printf '%s\n' "$cmd" | grep -Eq '(^|[;&|`(]|\$\()[[:space:]]*((sudo|xargs|exec|command|nohup|env|nice( -n -?[0-9]+)?)[[:space:]]+)*(/usr/bin/)?(pkill|killall)([[:space:]]|$)'; then
  echo "Blocked: pkill/killall match other agents' and builds' command lines. Stop processes by PID (kill <pid>) that you started." >&2
  exit 2
fi
exit 0
