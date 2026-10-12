#!/usr/bin/env bash
# Runs the browser build for testing from other devices on this network: a local Dragonfly server
# and the web relay serving the site over self-signed HTTPS, with this checkout's compiled carriers.
# Builds whatever is missing first. Ctrl-C stops both.
# Usage: dev.sh [LISTEN] (default 0.0.0.0:8787); CINNABAR_ASSETS names another compiled carrier
# directory, such as the main checkout's when running from a worktree.
set -euo pipefail

here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo="$(CDPATH= cd -- "$here/.." && pwd)"
listen="${1:-0.0.0.0:8787}"
make="${MAKE:-make}"
cd "$repo"

assets="${CINNABAR_ASSETS:-.local/assets/compiled}"
[[ -f "$assets/vanilla-v2193.mcbea" ]] || "$make" assets physics-assets
[[ -f target/web-site/pkg/web_client_bg.wasm ]] || "$make" web
[[ -x target/release/bedrock-web-relay ]] || "$make" web-relay
[[ -x target/release/bedrock-local-server ]] || "$make" local-server

world=.local/web-dev-world
mkdir -p "$world"

# One certificate for this machine's names and addresses, kept so browsers accept it only once.
tls=.local/web-dev
if [[ ! -f "$tls/cert.pem" ]]; then
    mkdir -p "$tls"
    names="DNS:localhost,DNS:$(hostname),IP:127.0.0.1"
    for address in $(hostname -I); do names+=",IP:$address"; done
    openssl req -x509 -newkey ec -pkeyopt ec_paramgen_curve:P-256 -nodes -days 825 \
        -subj "/CN=Cinnabar development relay" -addext "subjectAltName=$names" \
        -keyout "$tls/key.pem" -out "$tls/cert.pem" 2>/dev/null
fi
pids=()
trap 'kill "${pids[@]}" 2>/dev/null || true' EXIT INT TERM

# The server stops when its stdin closes, so it reads from a pipe this script holds open.
exec 3> >(target/release/bedrock-local-server -dir "$world" -addr 127.0.0.1:19132 -game-mode creative)
pids+=($!)
target/release/bedrock-web-relay -listen "$listen" -tls-cert "$tls/cert.pem" -tls-key "$tls/key.pem" \
    -dev-allow-private-upstream \
    -allowed-origin '*' -dev-assets "$assets" -site target/web-site &
pids+=($!)

host="${listen%:*}"
[[ "$host" == 0.0.0.0 ]] && host="$(hostname -I | awk '{ print $1 }')"
printf '\nOpen https://%s:%s/ and join 127.0.0.1:19132.\n\n' "$host" "${listen##*:}"
wait -n "${pids[@]}"
