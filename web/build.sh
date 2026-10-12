#!/usr/bin/env bash
# Builds the browser client site into OUT (default target/web-site); serve it with the web relay
# (`bedrock-web-relay -site OUT`) so its WebSockets share the page's origin.
# Usage: build.sh [OUT]; WEB_PROFILE picks the Cargo profile (default release; play builds faster).
# Needs the wasm32-unknown-unknown target, the wasm-bindgen CLI whose version Cargo.lock pins and,
# optionally, binaryen's wasm-opt.
set -euo pipefail

here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo="$(CDPATH= cd -- "$here/.." && pwd)"
out="${1:-$repo/target/web-site}"
cargo="${CARGO:-cargo}"
profile="${WEB_PROFILE:-release}"
case "$repo/" in
    "$out"/*) printf 'refusing to replace %s, which holds the repository\n' "$out" >&2; exit 1 ;;
esac

pinned="$(awk '/^name = "wasm-bindgen"$/ { getline; gsub(/version = |"/, ""); print; exit }' "$repo/Cargo.lock")"
installed="$(wasm-bindgen --version 2>/dev/null | awk '{ print $2 }' || true)"
if [[ "$installed" != "$pinned" ]]; then
    printf 'wasm-bindgen CLI %s is required (found %s): cargo install wasm-bindgen-cli --version %s --locked\n' \
        "$pinned" "${installed:-none}" "$pinned" >&2
    exit 1
fi

"$cargo" build --locked --profile "$profile" -p web-client --lib --target wasm32-unknown-unknown \
    --manifest-path "$repo/Cargo.toml"
target_dir="${CARGO_TARGET_DIR:-$repo/target}"

rm -rf "$out"
mkdir -p "$out/pkg"
wasm-bindgen --target web --no-typescript --out-dir "$out/pkg" \
    "$target_dir/wasm32-unknown-unknown/$profile/web_client.wasm"
# Other profiles keep function names, so panics in the browser console show where they began.
if [[ "$profile" == release ]] && command -v wasm-opt >/dev/null; then
    wasm-opt -O2 --enable-simd --enable-bulk-memory --enable-nontrapping-float-to-int --enable-sign-ext \
        --enable-mutable-globals --enable-reference-types \
        -o "$out/pkg/web_client_bg.wasm" "$out/pkg/web_client_bg.wasm"
fi
# Release builds ship precompressed copies the relay sends as they are (`StaticFiles`): zstd at the
# 8 MiB window HTTP allows, and gzip for browsers without zstd.
if [[ "$profile" == release ]]; then
    for file in "$out/pkg/web_client_bg.wasm" "$out/pkg/web_client.js"; do
        if command -v zstd >/dev/null; then zstd -19 -T0 -q -f "$file" -o "$file.zst"; fi
        if command -v gzip >/dev/null; then gzip -9 -k -f "$file"; fi
    done
fi
cp "$here/site/index.html" "$here/site/style.css" "$here/site/boot.js" "$here/site/packs.js" "$out/"
# Cinnabar's own physics registry; Mojang-derived carriers come from the player.
cp "$repo/crates/assets/data/block-physics-v2193.bin" "$out/"
