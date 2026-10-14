#!/usr/bin/env bash
# Prints the -ldflags -X arguments that stamp bedrock-core with the workspace release version and
# CINNABAR_BUILD_COMMIT, "unknown" when unset or empty, as the Rust client reports it. The caller
# selects the commit (the Makefile's CINNABAR_BUILD_COMMIT); this script never reads Git.
# Usage: go build -ldflags "$(bash packaging/common/core-ldflags.sh)" ./core/cmd/bedrock-core
set -euo pipefail

# Always prints a stamp: an unreadable version records "unknown" rather than an empty -ldflags.
# shellcheck source=stage-payload.sh
version="$(source "$(dirname -- "${BASH_SOURCE[0]}")/stage-payload.sh" && version_of)" || version=""
printf -- '-X main.version=%s -X main.commit=%s\n' "${version:-unknown}" "${CINNABAR_BUILD_COMMIT:-unknown}"
