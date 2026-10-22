#!/usr/bin/env bash
# Moves native line tables out of staged Android libraries before the APK is built and signed.
# Arguments: staged library directory, private symbol directory, NDK LLVM bin directory, filenames.
set -euo pipefail
libs="$1"
symbols="$2"
llvm="$3"
shift 3
mkdir -p "$symbols"
for name in "$@"; do
    "$llvm/llvm-objcopy" --only-keep-debug "$libs/$name" "$symbols/$name.debug"
    "$llvm/llvm-strip" --strip-debug "$libs/$name"
    cp "$libs/$name" "$symbols/$name"
done
