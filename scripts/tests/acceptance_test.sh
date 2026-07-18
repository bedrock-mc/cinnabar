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

export RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY=1
# shellcheck source=/dev/null
source "$script"
unset RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY

protocol_fixture="$temp_root/protocol-provenance"
mkdir -p "$protocol_fixture"
cp "$project_root/Cargo.toml" "$protocol_fixture/Cargo.toml"
cp "$project_root/Cargo.lock" "$protocol_fixture/Cargo.lock"
cp -R "$project_root/app" "$project_root/crates" "$project_root/tools" "$protocol_fixture/"
assert_protocol_dependency_provenance "$protocol_fixture"

cp "$protocol_fixture/crates/protocol/Cargo.toml" "$protocol_fixture/protocol.Cargo.toml.clean"
cp -R "$protocol_fixture/crates/protocol/vendor/valentine" "$protocol_fixture/crates/protocol/vendor/valentine-decoy"
cp -R "$protocol_fixture/crates/protocol/vendor/jolyne" "$protocol_fixture/crates/protocol/vendor/jolyne-decoy"
python3 - "$protocol_fixture/crates/protocol/Cargo.toml" "$protocol_fixture/crates/protocol/vendor/jolyne-decoy/Cargo.toml" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
jolyne_decoy = pathlib.Path(sys.argv[2])
jolyne_text = jolyne_decoy.read_text(encoding="utf-8")
jolyne_text = jolyne_text.replace('path = "../valentine"', 'path = "../valentine-decoy"', 1)
jolyne_decoy.write_text(jolyne_text, encoding="utf-8")
text = path.read_text(encoding="utf-8")
decoys = '''description = """
[dependencies]
valentine = { path = "vendor/valentine", default-features = false, features = ["bedrock_1_26_30"] }
jolyne = { path = "vendor/jolyne", default-features = false, features = ["client"] }
"""'''
text = text.replace("publish = false", "publish = false\n" + decoys, 1)
text = text.replace(
    'valentine = { path = "vendor/valentine", default-features = false, features = ["bedrock_1_26_30"] }',
    '"valentine" = { path = "vendor/valentine-decoy", default-features = false, features = ["bedrock_1_26_30"] }',
    1,
)
text = text.replace(
    'jolyne = { path = "vendor/jolyne", default-features = false, features = ["client"] }',
    '"jolyne" = { path = "vendor/jolyne-decoy", default-features = false, features = ["client"] }',
    1,
)
path.write_text(text, encoding="utf-8")
PY
if assert_protocol_dependency_provenance "$protocol_fixture" >/dev/null 2>&1; then
    echo 'Bash protocol provenance accepted multiline canonical decoys with quoted wrong-path dependencies' >&2
    exit 1
fi
cp "$protocol_fixture/protocol.Cargo.toml.clean" "$protocol_fixture/crates/protocol/Cargo.toml"

cat >>"$protocol_fixture/crates/protocol/Cargo.toml" <<'EOF'

[target.'cfg(unix)'.dependencies]
valentine = { path = "vendor/valentine", default-features = false, features = ["bedrock_1_26_30"] }
EOF
if assert_protocol_dependency_provenance "$protocol_fixture" >/dev/null 2>&1; then
    echo 'Bash protocol provenance accepted an additional target-table declaration' >&2
    exit 1
fi
cp "$protocol_fixture/protocol.Cargo.toml.clean" "$protocol_fixture/crates/protocol/Cargo.toml"

cp "$protocol_fixture/Cargo.lock" "$protocol_fixture/Cargo.lock.clean"
python3 - "$protocol_fixture/Cargo.lock" <<'PY'
import pathlib, sys
path = pathlib.Path(sys.argv[1])
text = path.read_text(encoding="utf-8")
needle = 'name = "jolyne"\nversion = "0.1.0"'
replacement = needle + '\n   checksum = "' + ('2' * 64) + '"'
if needle not in text:
    raise SystemExit('Bash checksum fixture did not find Jolyne lock package')
path.write_text(text.replace(needle, replacement, 1), encoding="utf-8")
PY
if assert_protocol_dependency_provenance "$protocol_fixture" >/dev/null 2>&1; then
    echo 'Bash protocol provenance accepted a whitespace-prefixed local checksum' >&2
    exit 1
fi
cp "$protocol_fixture/Cargo.lock.clean" "$protocol_fixture/Cargo.lock"

publication_log="$temp_root/publication-snapshots.log"
publication_output="$temp_root/publication-snapshot.json"
publication_row='{"accepted_light_jobs":18446744073709551615,"noop_light_jobs":2,"value_changed_light_jobs":3,"provenance_only_light_jobs":5,"light_mesh_invalidations":7,"stale_light_jobs":11,"stale_mesh_jobs":13,"queued_decode_jobs":17,"in_flight_decode_jobs":19,"pending_light_jobs":23,"in_flight_light_jobs":29,"pending_mesh_jobs":31,"in_flight_mesh_jobs":37,"max_decode_queue_wait_ms":41.0,"max_light_queue_wait_ms":43.0,"max_mesh_queue_wait_ms":47.0,"max_decode_worker_ms":53.0,"max_light_worker_ms":59.0,"max_mesh_worker_ms":61.0,"upload_queue_items":67,"upload_queue_bytes":71,"gpu_upload_bytes":73,"frame_generation":79,"pose_generation":83,"view_generation":89,"draw_mode":"Direct","build_profile":"release","requested_present_mode":"Fifo","effective_present_mode":"Fifo","present_mode_proven":true,"backend":"Dx12","adapter":"Test Adapter","driver":"test-driver","driver_info":"1.2.3"}'
printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "$publication_row" >"$publication_log"
read_world_publication_snapshots "$publication_log" release Fifo "$publication_output"
python3 -c 'import json, sys
with open(sys.argv[1], encoding="utf-8") as source: row = json.load(source)
assert row["draw_mode"] == "Direct"
assert row["accepted_light_jobs"] == 2**64 - 1
assert row["max_decode_queue_wait_ms"] == 41.0
assert row["max_decode_worker_ms"] == 53.0
' "$publication_output"

if printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "${publication_row/\"queued_decode_jobs\":17/\"queued_decode_jobs\":\"17\"}" >"$publication_log" && read_world_publication_snapshots "$publication_log" release Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'publication snapshot accepted a JSON string for an integer field' >&2
    exit 1
fi
if printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "${publication_row/\"max_decode_queue_wait_ms\":41.0/\"max_decode_queue_wait_ms\":\"41.0\"}" >"$publication_log" && read_world_publication_snapshots "$publication_log" release Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'publication snapshot accepted a JSON string for a duration field' >&2
    exit 1
fi
if printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "${publication_row/\"present_mode_proven\":true/\"present_mode_proven\":\"true\"}" >"$publication_log" && read_world_publication_snapshots "$publication_log" release Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'publication snapshot accepted a JSON string for a boolean field' >&2
    exit 1
fi

if printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "${publication_row/,\"max_mesh_queue_wait_ms\":47.0/}" >"$publication_log" && read_world_publication_snapshots "$publication_log" release Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'publication snapshot accepted a missing stage field' >&2
    exit 1
fi
if printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "${publication_row/\"draw_mode\":\"Direct\"/\"draw_mode\":\"Direct\",\"draw_mode\":\"MultiDrawIndirect\"}" >"$publication_log" && read_world_publication_snapshots "$publication_log" release Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'publication snapshot accepted a duplicate draw identity' >&2
    exit 1
fi
if printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT={\n' >"$publication_log" && read_world_publication_snapshots "$publication_log" release Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'publication snapshot accepted malformed JSON' >&2
    exit 1
fi
printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "$publication_row" >"$publication_log"
if read_world_publication_snapshots "$publication_log" debug Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'release and debug publication rows were conflated' >&2
    exit 1
fi
if read_world_publication_snapshots "$publication_log" release Immediate "$publication_output" >/dev/null 2>&1; then
    echo 'FIFO and Immediate publication rows were conflated' >&2
    exit 1
fi
if printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "${publication_row/\"draw_mode\":\"Direct\"/\"draw_mode\":\"Direct|MultiDrawIndirect\"}" >"$publication_log" && read_world_publication_snapshots "$publication_log" release Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'Direct and MDI were conflated in one publication row' >&2
    exit 1
fi
printf 'RUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\nRUST_MCBE_WORLD_PUBLICATION_SNAPSHOT=%s\n' "$publication_row" "${publication_row/\"draw_mode\":\"Direct\"/\"draw_mode\":\"MultiDrawIndirect\"}" >"$publication_log"
if read_world_publication_snapshots "$publication_log" release Fifo "$publication_output" >/dev/null 2>&1; then
    echo 'periodic publication rows silently changed draw mode' >&2
    exit 1
fi

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
grep -Fq "pinned_valentine_fork_commit='6cd8087fc3f0b500e41708a8afc94a0fa3291525'" "$script"
grep -Fq "pinned_valentine_upstream_commit='6f6806e821a579c183c44d786f76d9b358a2b825'" "$script"
grep -Fq "pinned_valentine_license_sha256='62c75fcb256604584191434b605dc3fe661d938a94b2c35836ef55011bf24184'" "$script"
grep -Fq '"protocol_dependency_resolution": "vendored-path"' "$script"
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
