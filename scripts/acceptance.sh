#!/usr/bin/env bash
set -euo pipefail

pinned_gophertunnel_commit='9948b1729395d2e819fce28e079d4a7bfc67716c'
pinned_valentine_commit='6f6806e821a579c183c44d786f76d9b358a2b825'

usage() {
    cat >&2 <<'EOF'
usage: scripts/acceptance.sh --duration SECONDS (--bds-dir PATH | --upstream HOST:PORT) --metrics-out PATH [--mutation-command PATH] [--no-vsync] [--dry-run]

When --upstream is used for a live run, --mutation-command is required. The executable
receives each complete BDS console command as its single argument and must relay it to
the upstream BDS console.
EOF
}

die() {
    printf 'acceptance: %s\n' "$*" >&2
    exit 1
}

shell_quote() {
    case "$1" in
        ''|*[!A-Za-z0-9_./:=+,-]*)
            printf "'%s'" "$(printf '%s' "$1" | sed "s/'/'\\\\''/g")"
            ;;
        *) printf '%s' "$1" ;;
    esac
}

format_command() {
    local separator=''
    local argument
    for argument in "$@"; do
        printf '%s' "$separator"
        shell_quote "$argument"
        separator=' '
    done
}

absolute_path() {
    local path=$1
    case "$path" in
        /*|[A-Za-z]:[\\/]*) printf '%s\n' "$path" ;;
        *) printf '%s/%s\n' "$(pwd -P)" "$path" ;;
    esac
}

sha256_file() {
    if command -v shasum >/dev/null 2>&1; then
        shasum -a 256 "$1" | awk '{print $1}'
    else
        sha256sum "$1" | awk '{print $1}'
    fi
}

wait_for_marker() {
    local log=$1 marker=$2 timeout=$3 pid=$4
    local deadline=$(( $(date +%s) + timeout ))
    while (( $(date +%s) < deadline )); do
        if [[ -f $log ]] && grep -Fq "$marker" "$log"; then
            return 0
        fi
        if ! kill -0 "$pid" 2>/dev/null; then
            die "process $pid exited before marker '$marker' (log: $log)"
        fi
        sleep 0.1
    done
    die "timed out waiting for marker '$marker' (log: $log)"
}

wait_for_exit() {
    local pid=$1 timeout=$2
    local deadline=$(( $(date +%s) + timeout ))
    while kill -0 "$pid" 2>/dev/null; do
        if (( $(date +%s) >= deadline )); then
            return 1
        fi
        sleep 0.1
    done
    return 0
}

wait_for_external_bds() {
    local address=$1 stdout_log=$2 stderr_log=$3
    python3 -u -c 'import random, socket, struct, sys, time
host, port_text = sys.argv[1].rsplit(":", 1)
port = int(port_text)
magic = bytes.fromhex("00ffff00fefefefefdfdfdfd12345678")
deadline = time.monotonic() + 120
last_error = "no response"
while time.monotonic() < deadline:
    try:
        targets = socket.getaddrinfo(host, port, type=socket.SOCK_DGRAM)
    except OSError as error:
        last_error = str(error)
        time.sleep(0.2)
        continue
    for family, kind, protocol, _, target in targets:
        probe = socket.socket(family, kind, protocol)
        probe.settimeout(1.0)
        try:
            sent_at = int(time.time() * 1000)
            guid = random.getrandbits(63)
            probe.sendto(b"\x01" + struct.pack(">q", sent_at) + magic + struct.pack(">q", guid), target)
            response, _ = probe.recvfrom(65535)
            if response[:1] == b"\x1c" and magic in response:
                print(f"External Bedrock upstream ready: {host}:{port}", flush=True)
                raise SystemExit(0)
            last_error = "unexpected RakNet response"
        except OSError as error:
            last_error = str(error)
        finally:
            probe.close()
    time.sleep(0.2)
print(f"timed out waiting for external Bedrock upstream: {last_error}", file=sys.stderr)
raise SystemExit(1)
' "$address" >"$stdout_log" 2>"$stderr_log"
}

start_runtime_lease_helper() {
    local lock_path=$1 control_path=$2 output_path=$3 error_path=$4
    local lease_deadline
    mkfifo -- "$control_path"
    exec 6<>"$control_path"
    lease_fd_open=true
    python3 -u -c 'import fcntl, sys
lock = open(sys.argv[1], "a+b")
try:
    fcntl.flock(lock.fileno(), fcntl.LOCK_EX | fcntl.LOCK_NB)
except BlockingIOError:
    print("stable BDS runtime is already leased", file=sys.stderr, flush=True)
    raise SystemExit(73)
print("LEASED", flush=True)
sys.stdin.readline()
' "$lock_path" <"$control_path" >"$output_path" 2>"$error_path" 6>&- &
    lease_pid=$!
    lease_deadline=$(( $(date +%s) + 10 ))
    while ! grep -Fq 'LEASED' "$output_path" 2>/dev/null; do
        if ! kill -0 "$lease_pid" 2>/dev/null; then
            die "failed to acquire stable BDS runtime lease (log: $error_path)"
        fi
        (( $(date +%s) < lease_deadline )) || die 'timed out acquiring stable BDS runtime lease'
        sleep 0.05
    done
}

start_udp_port_helper() {
    local control_path=$1 output_path=$2 error_path=$3
    local port_deadline
    mkfifo -- "$control_path"
    exec 7<>"$control_path"
    port_fd_open=true
    python3 -u -c 'import socket, sys
sockets = [socket.socket(socket.AF_INET, socket.SOCK_DGRAM) for _ in range(2)]
for item in sockets: item.bind(("127.0.0.1", 0))
print(*(item.getsockname()[1] for item in sockets), flush=True)
sys.stdin.readline()
' <"$control_path" >"$output_path" 2>"$error_path" 6>&- 7>&- &
    port_helper_pid=$!
    port_deadline=$(( $(date +%s) + 10 ))
    while [[ ! -s $output_path ]]; do
        kill -0 "$port_helper_pid" 2>/dev/null || die "UDP port reservation helper exited early (log: $error_path)"
        (( $(date +%s) < port_deadline )) || die 'timed out reserving UDP ports'
        sleep 0.05
    done
    read -r port port_v6 <"$output_path"
}

configure_server_properties() {
    local path=$1 port=$2 port_v6=$3 temporary="$1.tmp.$$"
    awk -v port="$port" -v port_v6="$port_v6" '
        BEGIN {
            want["server-port"] = port
            want["server-portv6"] = port_v6
            want["online-mode"] = "false"
            want["allow-list"] = "false"
            want["enable-lan-visibility"] = "false"
        }
        {
            line = $0
            sub(/\r$/, "", line)
            split(line, fields, "=")
            key = fields[1]
            if (key in want) {
                seen[key]++
                line = key "=" want[key]
            }
            print line
        }
        END {
            for (key in want) {
                if (seen[key] != 1) {
                    print "server.properties must contain exactly one " key > "/dev/stderr"
                    failed = 1
                }
            }
            exit failed
        }
    ' "$path" >"$temporary" || { rm -f -- "$temporary"; return 1; }
    mv -f -- "$temporary" "$path"
}

prepare_stable_runtime() {
    local source=$1 runtime=$2 executable=$3
    local source_full runtime_parent runtime_full source_prefix runtime_prefix marker owner entry temporary
    source_full=$(cd "$source" && pwd -P)
    runtime_parent=$(cd "$(dirname "$runtime")" && pwd -P)
    runtime_full="$runtime_parent/$(basename "$runtime")"
    source_prefix="$source_full/"
    runtime_prefix="$runtime_full/"
    case "$source_prefix" in "$runtime_prefix"*) die "BDS source is inside stable runtime" ;; esac
    case "$runtime_prefix" in "$source_prefix"*) die "stable runtime is inside BDS source" ;; esac

    mkdir -p -- "$runtime_full"
    [[ ! -L $runtime_full ]] || die "stable runtime must not be a symlink: $runtime_full"
    marker="$runtime_full/.rust-mcbe-runtime-owner"
    owner=$(printf 'rust-mcbe-bds-runtime-v1\nsource=%s\n' "$source_full")
    if [[ -e $marker ]]; then
        [[ ! -L $marker && -f $marker ]] || die "invalid stable runtime owner marker: $marker"
        [[ $(cat "$marker") == "$owner" ]] || die "stable runtime belongs to a different BDS source: $marker"
    else
        if find "$runtime_full" -mindepth 1 -maxdepth 1 -print -quit | grep -q .; then
            die "refusing unmarked non-empty stable runtime: $runtime_full"
        fi
        printf '%s\n' "$owner" >"$marker"
    fi

    if [[ ! -f $runtime_full/$executable ]] || ! cmp -s -- "$source_full/$executable" "$runtime_full/$executable"; then
        temporary="$runtime_full/bedrock-server-exe-$$.tmp"
        cp -p -- "$source_full/$executable" "$temporary"
        mv -f -- "$temporary" "$runtime_full/$executable"
    fi

    shopt -s nullglob dotglob
    for entry in "$runtime_full"/*; do
        [[ $(basename "$entry") == "$executable" || $(basename "$entry") == '.rust-mcbe-runtime-owner' ]] && continue
        case "$entry" in "$runtime_prefix"*) ;; *) die "refusing to reset path outside stable runtime: $entry" ;; esac
        rm -rf -- "$entry"
    done
    for entry in "$source_full"/*; do
        [[ $(basename "$entry") == "$executable" ]] && continue
        cp -a -- "$entry" "$runtime_full/"
    done
    shopt -u dotglob nullglob
    printf '%s/%s\n' "$runtime_full" "$executable"
}

if [[ ${RUST_MCBE_ACCEPTANCE_TEST_LIBRARY_ONLY:-} == 1 ]]; then
    return 0 2>/dev/null || exit 0
fi

duration=''
bds_dir=''
upstream=''
metrics_out=''
mutation_command=''
dry_run=false
no_vsync=false
while (( $# )); do
    case "$1" in
        --duration) [[ $# -ge 2 ]] || { usage; exit 2; }; duration=$2; shift 2 ;;
        --bds-dir) [[ $# -ge 2 ]] || { usage; exit 2; }; bds_dir=$2; shift 2 ;;
        --upstream) [[ $# -ge 2 ]] || { usage; exit 2; }; upstream=$2; shift 2 ;;
        --metrics-out) [[ $# -ge 2 ]] || { usage; exit 2; }; metrics_out=$2; shift 2 ;;
        --mutation-command) [[ $# -ge 2 ]] || { usage; exit 2; }; mutation_command=$2; shift 2 ;;
        --no-vsync) no_vsync=true; shift ;;
        --dry-run) dry_run=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *) usage; die "unknown argument: $1" ;;
    esac
done

[[ $duration =~ ^[0-9]+$ ]] || die '--duration must be an integer'
(( duration >= 60 )) || die '--duration must be at least 60 seconds'
[[ -n $metrics_out ]] || die '--metrics-out is required'
if [[ -n $bds_dir && -n $upstream ]] || [[ -z $bds_dir && -z $upstream ]]; then
    die 'provide exactly one of --bds-dir or --upstream'
fi
if [[ -n $upstream && ! $upstream =~ ^[^:[:space:]]+:[0-9]+$ ]]; then
    die "invalid upstream address: $upstream"
fi
if [[ -n $upstream ]]; then
    upstream_port=${upstream##*:}
    upstream_port_value=$((10#$upstream_port))
    (( upstream_port_value >= 1 && upstream_port_value <= 65535 )) || die "upstream port is out of range: $upstream_port"
fi

project_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd -P)
metrics_out=$(absolute_path "$metrics_out")
exe_suffix=''
bds_executable_name="bedrock_server$exe_suffix"
if [[ -n $bds_dir ]]; then
    [[ -d $bds_dir ]] || die "BDS directory does not exist: $bds_dir"
    bds_dir=$(cd "$bds_dir" && pwd -P)
    [[ -f $bds_dir/$bds_executable_name ]] || die "BDS executable does not exist: $bds_dir/$bds_executable_name"
    case $(uname -s) in
        MINGW*|MSYS*) ;;
        *) [[ -x $bds_dir/$bds_executable_name ]] || die "BDS executable is not executable: $bds_dir/$bds_executable_name" ;;
    esac
fi

run_name='dry-run'
if [[ $dry_run == false ]]; then
    run_name="$(date -u +%Y%m%dT%H%M%SZ)-$$"
fi
run_dir="$project_root/.local/acceptance/$run_name"
socket_dir="$run_dir/socket"
canonical_metrics="$run_dir/app-metrics.json"
core_executable="$project_root/target/release/bedrock-core$exe_suffix"
app_executable="$project_root/target/release/bedrock-client$exe_suffix"
runtime_dir=''
if [[ -n $bds_dir ]]; then
    runtime_dir="$project_root/.local/bds-runtime/$(basename "$bds_dir")"
    bds_executable="$runtime_dir/$bds_executable_name"
    initial_upstream='127.0.0.1:19132'
    bds_command=("$bds_executable")
else
    bds_executable=''
    initial_upstream=$upstream
    bds_command=(external-upstream "$upstream")
fi
core_command=("$core_executable" -socket-dir "$socket_dir" -upstream "$initial_upstream")
app_command=("$app_executable" --socket-dir "$socket_dir" --acceptance-seconds "$duration" --metrics-out "$canonical_metrics" --auto-fly)
if [[ $no_vsync == true ]]; then
    app_command+=(--no-vsync)
fi

if [[ $dry_run == true ]]; then
    printf 'BDS_COMMAND=%s\n' "$(format_command "${bds_command[@]}")"
    printf 'CORE_COMMAND=%s\n' "$(format_command "${core_command[@]}")"
    printf 'APP_COMMAND=%s\n' "$(format_command "${app_command[@]}")"
    printf 'BUILD_PROFILE=release\n'
    if [[ $no_vsync == true ]]; then
        printf 'REQUESTED_PRESENT_MODE=Immediate\n'
        printf 'EFFECTIVE_PRESENT_MODE=Immediate\n'
    else
        printf 'REQUESTED_PRESENT_MODE=Fifo\n'
        printf 'EFFECTIVE_PRESENT_MODE=Fifo\n'
    fi
    exit 0
fi

if [[ -n $upstream ]]; then
    [[ -n $mutation_command ]] || die '--mutation-command is required for a live --upstream run'
    mutation_command=$(command -v -- "$mutation_command") || die "mutation command is not executable: $mutation_command"
    [[ -x $mutation_command ]] || die "mutation command is not executable: $mutation_command"
fi

lease_pid=''
lease_fd_open=false
bds_pid=''
core_pid=''
app_pid=''
mutation_pid=''
bds_fd_open=false
core_fd_open=false
port_fd_open=false
port_helper_pid=''

cleanup() {
    local status=$?
    local cleanup_failed=0
    trap - EXIT INT TERM
    set +e
    if [[ -n $mutation_pid ]]; then
        if kill -0 "$mutation_pid" 2>/dev/null; then
            kill -TERM "$mutation_pid" 2>/dev/null
            if ! wait_for_exit "$mutation_pid" 2; then
                kill -KILL "$mutation_pid" 2>/dev/null
                wait_for_exit "$mutation_pid" 5 || cleanup_failed=1
            fi
        fi
        if kill -0 "$mutation_pid" 2>/dev/null; then
            cleanup_failed=1
        else
            wait "$mutation_pid" 2>/dev/null
        fi
    fi
    if [[ -n $app_pid ]]; then
        if kill -0 "$app_pid" 2>/dev/null; then
            kill -TERM "$app_pid" 2>/dev/null
            if ! wait_for_exit "$app_pid" 10; then
                printf 'acceptance: app did not close in 10 seconds; forcing termination\n' >&2
                kill -KILL "$app_pid" 2>/dev/null
                if ! wait_for_exit "$app_pid" 5; then
                    printf 'acceptance: app remained alive after forced termination\n' >&2
                    cleanup_failed=1
                fi
            fi
        fi
        if kill -0 "$app_pid" 2>/dev/null; then
            cleanup_failed=1
        else
            wait "$app_pid" 2>/dev/null
        fi
    fi
    if [[ $core_fd_open == true ]]; then
        exec 8>&-
        core_fd_open=false
    fi
    if [[ -n $core_pid ]]; then
        if kill -0 "$core_pid" 2>/dev/null && ! wait_for_exit "$core_pid" 10; then
            printf 'acceptance: core did not stop on stdin EOF; forcing termination\n' >&2
            kill -KILL "$core_pid" 2>/dev/null
            if ! wait_for_exit "$core_pid" 5; then
                printf 'acceptance: core remained alive after forced termination\n' >&2
                cleanup_failed=1
            fi
        fi
        if kill -0 "$core_pid" 2>/dev/null; then
            cleanup_failed=1
        else
            wait "$core_pid" 2>/dev/null
        fi
    fi
    if [[ $bds_fd_open == true ]]; then
        printf 'stop\n' >&9
        exec 9>&-
        bds_fd_open=false
    fi
    if [[ -n $bds_pid ]]; then
        if kill -0 "$bds_pid" 2>/dev/null && ! wait_for_exit "$bds_pid" 20; then
            printf 'acceptance: BDS did not stop in 20 seconds; forcing termination\n' >&2
            kill -KILL "$bds_pid" 2>/dev/null
            if ! wait_for_exit "$bds_pid" 5; then
                printf 'acceptance: BDS remained alive after forced termination\n' >&2
                cleanup_failed=1
            fi
        fi
        if kill -0 "$bds_pid" 2>/dev/null; then
            cleanup_failed=1
        else
            wait "$bds_pid" 2>/dev/null
        fi
    fi
    if [[ $port_fd_open == true ]]; then
        printf '\n' >&7
        exec 7>&-
    fi
    if [[ -n $port_helper_pid ]] && kill -0 "$port_helper_pid" 2>/dev/null; then
        if ! wait_for_exit "$port_helper_pid" 5; then
            kill -KILL "$port_helper_pid" 2>/dev/null
            wait_for_exit "$port_helper_pid" 5 || cleanup_failed=1
        fi
    fi
    [[ -n $port_helper_pid ]] && ! kill -0 "$port_helper_pid" 2>/dev/null && wait "$port_helper_pid" 2>/dev/null
    if [[ -f $canonical_metrics ]]; then
        if ! mkdir -p -- "$(dirname "$metrics_out")" || ! cp -f -- "$canonical_metrics" "$metrics_out"; then
            printf 'acceptance: failed to copy requested metrics\n' >&2
            cleanup_failed=1
        fi
    fi
    if [[ $lease_fd_open == true ]]; then
        printf '\n' >&6
        exec 6>&-
        lease_fd_open=false
    fi
    if [[ -n $lease_pid ]] && kill -0 "$lease_pid" 2>/dev/null; then
        if ! wait_for_exit "$lease_pid" 5; then
            kill -KILL "$lease_pid" 2>/dev/null
            wait_for_exit "$lease_pid" 5 || cleanup_failed=1
        fi
    fi
    [[ -n $lease_pid ]] && ! kill -0 "$lease_pid" 2>/dev/null && wait "$lease_pid" 2>/dev/null
    if (( cleanup_failed != 0 && status == 0 )); then
        status=1
    fi
    if (( status != 0 )) && type write_metadata >/dev/null 2>&1; then
        write_metadata failed "$status" || {
            printf 'acceptance: failed to update failure metadata\n' >&2
            status=1
        }
    fi
    exit "$status"
}
trap cleanup EXIT
trap 'exit 130' INT
trap 'exit 143' TERM

mkdir -p -- "$run_dir" "$project_root/.local/bds-runtime" "$(dirname "$metrics_out")"

repo_commit=$(git -C "$project_root" rev-parse HEAD)
bds_hash='external-upstream'
[[ -n $bds_dir ]] && bds_hash=$(sha256_file "$bds_dir/$bds_executable_name")
machine=$(hostname 2>/dev/null || printf unknown)
os_description=$(uname -a 2>/dev/null || printf unknown)
cpu_description=$(sysctl -n machdep.cpu.brand_string 2>/dev/null || awk -F: '/model name/{sub(/^ /,"",$2); print $2; exit}' /proc/cpuinfo 2>/dev/null || printf unknown)
gpu_description=$(system_profiler SPDisplaysDataType 2>/dev/null || printf unavailable)
run_started_utc=$(date -u +%Y-%m-%dT%H:%M:%SZ)
runtime_metadata_json="$run_dir/runtime-metadata.json"

write_command_manifest() {
    local temporary="$run_dir/commands.txt.tmp.$$"
    {
        printf 'REPO_COMMIT=%s\n' "$repo_commit"
        printf 'BDS_COMMAND=%s\n' "$(format_command "${bds_command[@]}")"
        printf 'CORE_COMMAND=%s\n' "$(format_command "${core_command[@]}")"
        printf 'APP_COMMAND=%s\n' "$(format_command "${app_command[@]}")"
        printf 'MUTATION_COMMAND=%s\n' "${mutation_command:-local-bds-stdin}"
    } >"$temporary"
    mv -f -- "$temporary" "$run_dir/commands.txt"
}

write_metadata() {
    local status=$1 exit_code=${2:-}
    export RUST_MCBE_META_STATUS="$status"
    export RUST_MCBE_META_EXIT_CODE="$exit_code"
    export RUST_MCBE_META_STARTED="$run_started_utc"
    export RUST_MCBE_META_REPO_COMMIT="$repo_commit"
    export RUST_MCBE_META_GOPHERTUNNEL="$pinned_gophertunnel_commit"
    export RUST_MCBE_META_VALENTINE="$pinned_valentine_commit"
    export RUST_MCBE_META_BDS_HASH="$bds_hash"
    export RUST_MCBE_META_BDS_COMMAND="$(format_command "${bds_command[@]}")"
    export RUST_MCBE_META_CORE_COMMAND="$(format_command "${core_command[@]}")"
    export RUST_MCBE_META_APP_COMMAND="$(format_command "${app_command[@]}")"
    export RUST_MCBE_META_MUTATION_COMMAND="${mutation_command:-local-bds-stdin}"
    export RUST_MCBE_META_MACHINE="$machine"
    export RUST_MCBE_META_OS="$os_description"
    export RUST_MCBE_META_CPU="$cpu_description"
    export RUST_MCBE_META_GPU="$gpu_description"
    export RUST_MCBE_META_DURATION="$duration"
    export RUST_MCBE_META_NO_VSYNC="$no_vsync"
    export RUST_MCBE_META_RUNTIME_PATH="$runtime_metadata_json"
    python3 - "$run_dir/metadata.json" <<'PY'
import datetime, json, os, sys
keys = {
    "status": "RUST_MCBE_META_STATUS",
    "started_utc": "RUST_MCBE_META_STARTED",
    "repo_commit": "RUST_MCBE_META_REPO_COMMIT",
    "pinned_gophertunnel_commit": "RUST_MCBE_META_GOPHERTUNNEL",
    "pinned_valentine_commit": "RUST_MCBE_META_VALENTINE",
    "bds_sha256": "RUST_MCBE_META_BDS_HASH",
    "bds_command": "RUST_MCBE_META_BDS_COMMAND",
    "core_command": "RUST_MCBE_META_CORE_COMMAND",
    "app_command": "RUST_MCBE_META_APP_COMMAND",
    "mutation_command": "RUST_MCBE_META_MUTATION_COMMAND",
    "machine": "RUST_MCBE_META_MACHINE",
    "operating_system": "RUST_MCBE_META_OS",
    "cpu": "RUST_MCBE_META_CPU",
    "gpu_display": "RUST_MCBE_META_GPU",
}
metadata = {name: os.environ[source] for name, source in keys.items()}
metadata.update({
    "duration_seconds": int(os.environ["RUST_MCBE_META_DURATION"]),
    "build_app_command": "cargo build --release -p bedrock-client --locked",
    "build_profile": "release",
    "build_core_command": "go build -trimpath -o target/release/bedrock-core ./core/cmd/bedrock-core",
    "use_vsync": os.environ["RUST_MCBE_META_NO_VSYNC"] != "true",
    "no_vsync_ab": os.environ["RUST_MCBE_META_NO_VSYNC"] == "true",
    "requested_present_mode": "Immediate" if os.environ["RUST_MCBE_META_NO_VSYNC"] == "true" else "Fifo",
    "effective_present_mode": "Immediate" if os.environ["RUST_MCBE_META_NO_VSYNC"] == "true" else "Fifo",
})
runtime_path = os.environ["RUST_MCBE_META_RUNTIME_PATH"]
if os.path.isfile(runtime_path):
    with open(runtime_path, encoding="utf-8") as source:
        metadata.update(json.load(source))
if os.environ["RUST_MCBE_META_EXIT_CODE"]:
    metadata["exit_code"] = int(os.environ["RUST_MCBE_META_EXIT_CODE"])
if metadata["status"] in {"passed", "failed"}:
    metadata["completed_utc"] = datetime.datetime.now(datetime.timezone.utc).isoformat()
temporary = sys.argv[1] + ".tmp"
with open(temporary, "w", encoding="utf-8") as output:
    json.dump(metadata, output, indent=2, sort_keys=True)
    output.write("\n")
os.replace(temporary, sys.argv[1])
PY
}

write_command_manifest
write_metadata preparing

if [[ -n $bds_dir ]]; then
    lease_control="$run_dir/bds-runtime-lease.control"
    lease_output="$run_dir/bds-runtime-lease.out"
    lease_error="$run_dir/bds-runtime-lease.stderr.log"
    start_runtime_lease_helper "$runtime_dir.lock" "$lease_control" "$lease_output" "$lease_error"
    bds_executable=$(prepare_stable_runtime "$bds_dir" "$runtime_dir" "$bds_executable_name")

    port_control="$run_dir/port-reservation.control"
    port_output="$run_dir/port-reservation.out"
    port_error="$run_dir/port-reservation.stderr.log"
    start_udp_port_helper "$port_control" "$port_output" "$port_error"
    upstream="127.0.0.1:$port"
    configure_server_properties "$runtime_dir/server.properties" "$port" "$port_v6"
    core_command=("$core_executable" -socket-dir "$socket_dir" -upstream "$upstream")
    bds_command=("$bds_executable")
fi

printf 'BDS_COMMAND=%s\n' "$(format_command "${bds_command[@]}")"
printf 'CORE_COMMAND=%s\n' "$(format_command "${core_command[@]}")"
printf 'APP_COMMAND=%s\n' "$(format_command "${app_command[@]}")"

write_command_manifest
write_metadata building

(cd "$project_root" && cargo build --release -p bedrock-client --locked) >"$run_dir/build-app.log" 2>&1 || die "release app build failed (log: $run_dir/build-app.log)"
(cd "$project_root" && go build -trimpath -o "$core_executable" ./core/cmd/bedrock-core) >"$run_dir/build-core.log" 2>&1 || die "release core build failed (log: $run_dir/build-core.log)"
write_metadata launching

if [[ -n $bds_dir ]]; then
    printf '\n' >&7
    exec 7>&-
    port_fd_open=false
    wait "$port_helper_pid"
    bds_stdin="$run_dir/bds.stdin"
    mkfifo -- "$bds_stdin"
    (cd "$runtime_dir" && exec "$bds_executable" <"$bds_stdin" >"$run_dir/bds.stdout.log" 2>"$run_dir/bds.stderr.log") &
    bds_pid=$!
    exec 9>"$bds_stdin"
    bds_fd_open=true
    wait_for_marker "$run_dir/bds.stdout.log" 'Server started.' 120 "$bds_pid"
else
    wait_for_external_bds "$upstream" "$run_dir/bds.stdout.log" "$run_dir/bds.stderr.log" || die "external BDS did not become ready (log: $run_dir/bds.stderr.log)"
fi

core_stdin="$run_dir/core.stdin"
mkfifo -- "$core_stdin"
(cd "$project_root" && exec "$core_executable" -socket-dir "$socket_dir" -upstream "$upstream" <"$core_stdin" >"$run_dir/core.stdout.log" 2>"$run_dir/core.stderr.log" 6>&- 9>&-) &
core_pid=$!
exec 8>"$core_stdin"
core_fd_open=true
endpoint="$socket_dir/game.sock"
endpoint_deadline=$(( $(date +%s) + 30 ))
while [[ ! -S $endpoint ]]; do
    kill -0 "$core_pid" 2>/dev/null || die "core exited before endpoint publication (log: $run_dir/core.stderr.log)"
    (( $(date +%s) < endpoint_deadline )) || die "timed out waiting for core endpoint: $endpoint"
    sleep 0.1
done

(cd "$project_root" && exec "${app_command[@]}" >"$run_dir/app.stdout.log" 2>"$run_dir/app.stderr.log" 6>&- 8>&- 9>&-) &
app_pid=$!
wait_for_marker "$run_dir/app.stdout.log" 'RUST_MCBE_MUTATION_COORDINATE=' 180 "$app_pid"
wait_for_marker "$run_dir/app.stdout.log" 'RUST_MCBE_WORLD_READY ' 180 "$app_pid"
coordinate=$(grep -m1 '^RUST_MCBE_MUTATION_COORDINATE=' "$run_dir/app.stdout.log" | sed 's/^RUST_MCBE_MUTATION_COORDINATE=//')
[[ $coordinate =~ ^-?[0-9]+,-?[0-9]+,-?[0-9]+$ ]] || die "invalid mutation coordinate: $coordinate"
IFS=, read -r mutation_x mutation_y mutation_z <<EOF
$coordinate
EOF

app_deadline=$(( $(date +%s) + duration + 90 ))
block_index=0
next_mutation=0
blocks=(minecraft:gold_block minecraft:diamond_block)
while kill -0 "$app_pid" 2>/dev/null; do
    now=$(date +%s)
    (( now < app_deadline )) || die "app exceeded acceptance deadline of $((duration + 90)) seconds"
    if (( now >= next_mutation )); then
        command="setblock $mutation_x $mutation_y $mutation_z ${blocks[$block_index]}"
        if [[ -n $bds_pid ]]; then
            printf '%s\n' "$command" >&9
        else
            "$mutation_command" "$command" >>"$run_dir/mutation-command.stdout.log" 2>>"$run_dir/mutation-command.stderr.log" 6>&- 8>&- 9>&- &
            mutation_pid=$!
            if ! wait_for_exit "$mutation_pid" 5; then
                kill -TERM "$mutation_pid" 2>/dev/null
                if ! wait_for_exit "$mutation_pid" 2; then
                    kill -KILL "$mutation_pid" 2>/dev/null
                    wait_for_exit "$mutation_pid" 5 || die "mutation command could not be terminated: $mutation_command"
                fi
                wait "$mutation_pid" 2>/dev/null
                mutation_pid=''
                die "mutation command timed out: $mutation_command"
            fi
            if ! wait "$mutation_pid"; then
                mutation_pid=''
                die "mutation command failed: $mutation_command (log: $run_dir/mutation-command.stderr.log)"
            fi
            mutation_pid=''
        fi
        printf '%s\n' "$command" >>"$run_dir/bds.console.log"
        block_index=$(( (block_index + 1) % 2 ))
        next_mutation=$(( now + 2 ))
    fi
    sleep 0.1
done
wait "$app_pid" || die "app exited unsuccessfully (logs: $run_dir/app.stdout.log, $run_dir/app.stderr.log)"
app_pid=''

python3 - "$run_dir/app.stdout.log" "$run_dir/app.stderr.log" "$runtime_metadata_json" "$no_vsync" <<'PY'
import json, os, sys
stdout_path, stderr_path, output_path, no_vsync = sys.argv[1:]
prefix = "RUST_MCBE_ACCEPTANCE_RUNTIME_METADATA="
with open(stdout_path, encoding="utf-8") as source:
    markers = [line.rstrip("\n")[len(prefix):] for line in source if line.startswith(prefix)]
if len(markers) != 1:
    raise SystemExit(f"expected exactly one acceptance runtime metadata marker, found {len(markers)}")
runtime = json.loads(markers[0])
required = {
    "build_profile", "requested_present_mode", "effective_present_mode",
    "backend", "adapter", "driver", "driver_info",
}
missing = sorted(name for name in required if not str(runtime.get(name, "")).strip())
if missing:
    raise SystemExit("acceptance runtime metadata is missing: " + ", ".join(missing))
expected_mode = "Immediate" if no_vsync == "true" else "Fifo"
fallback_marker = "PresentMode Immediate requested but not available. Falling back to Fifo"
with open(stdout_path, encoding="utf-8") as source:
    stdout_text = source.read()
with open(stderr_path, encoding="utf-8") as source:
    stderr_text = source.read()
effective_mode = runtime["effective_present_mode"]
if runtime["requested_present_mode"] == "Immediate" and (
    fallback_marker in stdout_text or fallback_marker in stderr_text
):
    effective_mode = "Fifo"
if runtime["build_profile"] != "release":
    raise SystemExit(f"acceptance requires a release client, observed {runtime['build_profile']}")
metadata = {
    "build_profile": runtime["build_profile"],
    "requested_present_mode": runtime["requested_present_mode"],
    "effective_present_mode": effective_mode,
    "graphics_backend": runtime["backend"],
    "graphics_adapter": runtime["adapter"],
    "graphics_driver": runtime["driver"],
    "graphics_driver_info": runtime["driver_info"],
}
temporary = output_path + ".tmp"
with open(temporary, "w", encoding="utf-8") as output:
    json.dump(metadata, output, indent=2, sort_keys=True)
    output.write("\n")
os.replace(temporary, output_path)
if runtime["requested_present_mode"] != expected_mode or effective_mode != expected_mode:
    raise SystemExit(
        "acceptance present mode mismatch: "
        f"expected={expected_mode} requested={runtime['requested_present_mode']} "
        f"effective={effective_mode}"
    )
PY

python3 - "$canonical_metrics" "$duration" <<'PY'
import json, math, platform, sys
path, duration = sys.argv[1], int(sys.argv[2])
with open(path, encoding="utf-8") as source:
    metrics = json.load(source)
required = {
    "session_seconds", "world_ready", "requested_radius_chunks", "received_radius_chunks",
    "publisher_radius_chunks", "mutation_coordinate", "visible_mutation_count", "frame_count",
    "p50_frame_ms", "p95_frame_ms", "p99_frame_ms", "max_frame_ms", "max_decode_ms",
    "max_mesh_ms", "max_remesh_ms", "max_mutation_to_visible_ms", "decode_error_count",
    "rendered_sub_chunks", "resident_sub_chunks", "visible_sub_chunks",
    "peak_admitted_world_events", "peak_admitted_heavy_events", "peak_queued_decode_jobs",
    "peak_in_flight_decode_jobs", "peak_completed_decode_results", "peak_pending_retry_requests",
    "peak_outbound_requests", "peak_pending_mesh_jobs", "peak_in_flight_mesh_jobs",
    "gpu_upload_bytes",
}
missing = sorted(required - metrics.keys())
if missing:
    raise SystemExit("missing metrics fields: " + ", ".join(missing))
checks = [
    (metrics["session_seconds"] >= duration, "session duration gate failed"),
    (metrics["world_ready"] is True, "world-ready gate failed"),
    (metrics["requested_radius_chunks"] == 16, "requested radius gate failed"),
    (metrics["received_radius_chunks"] == 16, "received radius gate failed"),
    (metrics["publisher_radius_chunks"] == 16, "publisher radius gate failed"),
    (metrics["frame_count"] > 0, "frame-count gate failed"),
    (isinstance(metrics["p99_frame_ms"], (int, float)) and math.isfinite(metrics["p99_frame_ms"]), "p99 evidence is not finite"),
    (metrics["decode_error_count"] == 0, "decode-error gate failed"),
    (metrics["rendered_sub_chunks"] > 0, "rendered sub-chunk gate failed"),
    (metrics["resident_sub_chunks"] > 0, "resident sub-chunk gate failed"),
    (metrics["visible_sub_chunks"] > 0, "visible sub-chunk gate failed"),
    (metrics["visible_mutation_count"] > 0, "visible mutation gate failed"),
    (metrics["max_mutation_to_visible_ms"] <= 100, "mutation latency gate failed"),
]
if platform.system() == "Darwin":
    checks.append((metrics["p99_frame_ms"] <= 8, "dev MacBook p99 gate failed"))
failures = [message for passed, message in checks if not passed]
if failures:
    raise SystemExit("; ".join(failures))
print(f"ACCEPTANCE_P99_FRAME_MS={metrics['p99_frame_ms']}")
PY
write_metadata passed
printf 'ACCEPTANCE_ARTIFACTS=%s\n' "$run_dir"
