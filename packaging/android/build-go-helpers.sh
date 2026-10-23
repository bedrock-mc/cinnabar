#!/usr/bin/env bash
# Sourced by build-apk.sh with the APK staging paths and Android toolchain configured.

# Build executable helpers into the APK's extracted native-library directory.
build_go_helpers() (
    export CGO_ENABLED=1 GOOS=android GOARCH=arm64
    export CC="$toolchain/bin/$triple$min_sdk-clang"
    # Pion's Android interface lookup (anet) needs Go's private net linknames.
    local ldflags="-s -w -checklinkname=0 -extldflags=$page_flags"
    go build -buildmode=pie -trimpath \
        -ldflags "$ldflags $(bash "$here/../common/core-ldflags.sh")" \
        -o "$libs/$(pin core_library)" ./core/cmd/bedrock-core

    # The local server has its own module and dependency pins, outside go.work.
    cd "$repo_root/tools/localserver"
    GOWORK=off go build -buildmode=pie -trimpath -ldflags "$ldflags" \
        -o "$libs/$(pin local_server_library)" .
)
