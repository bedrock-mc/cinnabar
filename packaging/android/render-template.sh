#!/usr/bin/env bash
# Sourced by the APK packager after its pinned values and product identity are loaded.

# render <source> <destination>: fill packaging values and reject unresolved template tokens.
render() {
    sed -e "s|@APPLICATION_ID@|$(pin application_id)|g" -e "s|@CLIENT_LIBRARY@|$client_library|g" \
        -e "s|@PRODUCT_NAME@|$product|g" -e "s|@VERSION_CODE@|$version_code|g" \
        -e "s|@VERSION_NAME@|$version|g" "$1" >"$2"
    if grep -q '@[A-Z_]*@' "$2"; then
        echo "unfilled template token in $2" >&2
        exit 1
    fi
}
