#!/usr/bin/env bash
# Sourced by build-apk.sh with its NDK, staging paths and runtime manifest lookup.

# Build the Linux translator and install its Android-adapted native runtime in executable APK storage.
build_box64() {
    local helper
    helper="$(python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))["bds_translation"]["helper_library"])' "$here/runtime.json")"
    local build="$out/bds-runtime"
    local compiler="${CINNABAR_ANDROID_BDS_CC:-aarch64-linux-gnu-gcc}"
    cmake -S "$here/box64" -B "$build" -G Ninja \
        -DCMAKE_SYSTEM_NAME=Linux -DCMAKE_SYSTEM_PROCESSOR=aarch64 \
        -DCMAKE_C_COMPILER="$compiler" -DCMAKE_BUILD_TYPE=Release \
        -DCINNABAR_BDS_CXX="${CINNABAR_ANDROID_BDS_CXX:-${compiler%gcc}g++}" \
        -DCMAKE_EXE_LINKER_FLAGS="-pie $page_flags" \
        -DCINNABAR_BDS_NATIVE_OVERRIDE="${CINNABAR_ANDROID_BDS_NATIVE_DIR:-}" \
        -DCINNABAR_BUILD_JOBS="${CARGO_BUILD_JOBS:-4}"
    cmake --build "$build" --target box64 --parallel "${CARGO_BUILD_JOBS:-4}"
    "$toolchain/bin/llvm-strip" --strip-unneeded -o "$libs/$helper" "$build/build/box64"
    chmod 0755 "$libs/$helper"
    local source packaged
    while IFS=$'\t' read -r source packaged; do
        install -m 0755 "$source" "$libs/$packaged"
    done < <(python3 - "$here/runtime.json" "${CINNABAR_ANDROID_BDS_NATIVE_DIR:-}" "$build" <<'PY'
import json, pathlib, sys
runtime = json.load(open(sys.argv[1]))["bds_translation"]["native_runtime"]
root = pathlib.Path(sys.argv[2]) if sys.argv[2] else pathlib.Path(sys.argv[3]) / "glibc" / runtime["prefix"]
for library in runtime["libraries"]:
    print(root / library["source"], library["packaged"], sep="\t")
PY
)
    cmake -DMANIFEST="$here/runtime.json" -DDIRECTORY="$libs" \
        -DREADELF="$toolchain/bin/llvm-readelf" -P "$here/box64/verify.cmake"
}
