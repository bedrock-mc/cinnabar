#!/bin/sh
# Keep one Go child on the atomic release selected by the SSH receiver.
set -eu
child=''
stop() {
    if [ -n "$child" ]; then
        kill -TERM "$child" 2>/dev/null || true
        wait "$child" || true
        child=''
    fi
}
trap 'stop; exit 0' INT TERM
while :; do
    release="$(readlink /srv/site/current)"
    export CINNABAR_RELEASE="${release##*/}"
    export CINNABAR_ROOT="/srv/site/$release/public"
    export CINNABAR_LISTEN=':80'
    "/srv/site/$release/server" &
    child="$!"
    while [ "$(readlink /srv/site/current)" = "$release" ]; do
        if ! kill -0 "$child" 2>/dev/null; then
            wait "$child"
            exit 1
        fi
        sleep 1
    done
    stop
done
