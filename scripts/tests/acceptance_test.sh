#!/usr/bin/env bash
set -euo pipefail

project_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)"
script="$project_root/scripts/acceptance.sh"
temp_root="$(mktemp -d "${TMPDIR:-/tmp}/rust-mcbe-acceptance.XXXXXX")"
trap 'rm -rf "$temp_root"' EXIT

bds_dir="$temp_root/bds source"
metrics_out="$temp_root/metrics output/metrics.json"
mkdir -p "$bds_dir"
printf fixture >"$bds_dir/bedrock_server"
chmod +x "$bds_dir/bedrock_server"
[[ ! -e "$project_root/.local/acceptance/dry-run" ]] || {
    echo 'pre-existing dry-run artifact prevents the immutability assertion' >&2
    exit 1
}

output="$(bash "$script" --dry-run --duration 900 --bds-dir "$bds_dir" --metrics-out "$metrics_out")"
commands=()
while IFS= read -r line; do
    case "$line" in
        BDS_COMMAND=*|CORE_COMMAND=*|APP_COMMAND=*) commands[${#commands[@]}]=$line ;;
    esac
done <<<"$output"
[[ ${#commands[@]} -eq 3 ]]
[[ ${commands[0]} == BDS_COMMAND=* ]]
[[ ${commands[1]} == CORE_COMMAND=* ]]
[[ ${commands[2]} == APP_COMMAND=* ]]
for flag in --socket-dir '--acceptance-seconds 900' --metrics-out --auto-fly; do
    [[ ${commands[2]} == *"$flag"* ]]
done
[[ ${commands[2]} != *--no-vsync* ]]
[[ $output == *'BUILD_PROFILE=release'* ]]
[[ $output == *'REQUESTED_PRESENT_MODE=Fifo'* ]]
[[ $output == *'EFFECTIVE_PRESENT_MODE=UNPROVEN'* ]]
[[ ${commands[0]} == *"'"* ]]
[[ ! -e "$project_root/.local/acceptance/dry-run" ]]
[[ ! -e "$metrics_out" ]]

no_vsync_output="$(bash "$script" --dry-run --duration 900 --bds-dir "$bds_dir" --metrics-out "$metrics_out" --no-vsync)"
[[ $no_vsync_output == *--no-vsync* ]]
[[ $no_vsync_output == *'REQUESTED_PRESENT_MODE=Immediate'* ]]
[[ $no_vsync_output == *'EFFECTIVE_PRESENT_MODE=UNPROVEN'* ]]
! grep -q 'PresentMode Immediate requested but not available. Falling back to Fifo' "$script"
grep -q 'present_mode_proven' "$script"

if bash "$script" --dry-run --duration 59 --bds-dir "$bds_dir" --metrics-out "$metrics_out" >/dev/null 2>&1; then
    echo 'duration below 60 seconds was accepted' >&2
    exit 1
fi
if bash "$script" --dry-run --duration 900 --bds-dir "$temp_root/missing" --metrics-out "$metrics_out" >/dev/null 2>&1; then
    echo 'missing BDS directory was accepted' >&2
    exit 1
fi
if bash "$script" --dry-run --duration 900 --bds-dir "$bds_dir" --upstream 127.0.0.1:19132 --metrics-out "$metrics_out" >/dev/null 2>&1; then
    echo 'conflicting BDS inputs were accepted' >&2
    exit 1
fi
for invalid_upstream in 127.0.0.1:0 127.0.0.1:65536; do
    if bash "$script" --dry-run --duration 900 --upstream "$invalid_upstream" --metrics-out "$metrics_out" >/dev/null 2>&1; then
        echo "invalid upstream was accepted: $invalid_upstream" >&2
        exit 1
    fi
done

case $(uname -s) in
    MINGW*|MSYS*) ;;
    *)
        chmod -x "$bds_dir/bedrock_server"
        if bash "$script" --dry-run --duration 900 --bds-dir "$bds_dir" --metrics-out "$metrics_out" >/dev/null 2>&1; then
            echo 'non-executable BDS was accepted' >&2
            exit 1
        fi
        chmod +x "$bds_dir/bedrock_server"
        ;;
esac

upstream="$(bash "$script" --dry-run --duration 900 --upstream 127.0.0.1:19132 --metrics-out "$metrics_out")"
[[ $(printf '%s\n' "$upstream" | grep -Ec '^(BDS|CORE|APP)_COMMAND=') -eq 3 ]]
if bash "$script" --duration 60 --upstream 127.0.0.1:19132 --metrics-out "$metrics_out" >/dev/null 2>&1; then
    echo 'live external upstream without a mutation command was accepted' >&2
    exit 1
fi

grep -Fq 'fcntl.flock' "$script"
grep -Fq '6>&- 9>&-' "$script"
grep -Fq '6>&- 8>&- 9>&-' "$script"
grep -Fq '6>&- 7>&-' "$script"
grep -Fq 'External Bedrock upstream ready' "$script"
grep -Fq -- "--mutation-command is required for a live --upstream run" "$script"
grep -Fq 'metrics["publisher_radius_chunks"] == 16' "$script"
grep -Fq 'metrics["frame_count"] > 0' "$script"
grep -Fq 'math.isfinite(metrics["p99_frame_ms"])' "$script"
grep -Fq "pinned_gophertunnel_commit='9948b1729395d2e819fce28e079d4a7bfc67716c'" "$script"
grep -Fq "pinned_valentine_commit='6f6806e821a579c183c44d786f76d9b358a2b825'" "$script"
metadata_line=$(grep -n '^write_metadata preparing$' "$script" | cut -d: -f1)
build_line=$(grep -n 'cargo build --release' "$script" | tail -1 | cut -d: -f1)
[[ -n $metadata_line && -n $build_line && $metadata_line -lt $build_line ]]

case $(uname -s) in
    MINGW*|MSYS*) ;;
    *)
        helper_root="$temp_root/helper-parent-death"
        mkdir -p "$helper_root"
        (
            export RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY=1
            # shellcheck source=/dev/null
            source "$script"
            start_runtime_lease_helper \
                "$helper_root/runtime.lock" \
                "$helper_root/lease.control" \
                "$helper_root/lease.out" \
                "$helper_root/lease.err"
            start_udp_port_helper \
                "$helper_root/port.control" \
                "$helper_root/port.out" \
                "$helper_root/port.err"
            printf '%s %s\n' "$lease_pid" "$port_helper_pid" >"$helper_root/pids"
            wait
        ) &
        helper_parent=$!
        helper_deadline=$(( $(date +%s) + 10 ))
        while [[ ! -s $helper_root/pids ]]; do
            kill -0 "$helper_parent" 2>/dev/null || {
                echo 'helper parent exited before publishing child PIDs' >&2
                exit 1
            }
            (( $(date +%s) < helper_deadline )) || {
                kill -KILL "$helper_parent" 2>/dev/null || true
                echo 'timed out starting helper parent-death test' >&2
                exit 1
            }
            sleep 0.05
        done
        read -r lease_helper port_helper <"$helper_root/pids"
        kill -KILL "$helper_parent"
        wait "$helper_parent" 2>/dev/null || true
        for helper_pid in "$lease_helper" "$port_helper"; do
            helper_deadline=$(( $(date +%s) + 5 ))
            while kill -0 "$helper_pid" 2>/dev/null; do
                if (( $(date +%s) >= helper_deadline )); then
                    kill -KILL "$helper_pid" 2>/dev/null || true
                    echo "orphaned acceptance helper retained its control FIFO: $helper_pid" >&2
                    exit 1
                fi
                sleep 0.05
            done
        done
        python3 -c 'import fcntl, sys
lock = open(sys.argv[1], "a+b")
fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
' "$helper_root/runtime.lock"
        ;;
esac

printf '%s\n' 'acceptance.sh dry-run tests: PASS'
