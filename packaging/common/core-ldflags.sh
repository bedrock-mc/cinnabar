#!/usr/bin/env bash
# Prints the -ldflags -X arguments that stamp bedrock-core with the workspace release version and
# the build commit (CINNABAR_BUILD_COMMIT, else HEAD), the identity the Rust client reports.
# Usage: go build -ldflags "$(bash packaging/common/core-ldflags.sh)" ./core/cmd/bedrock-core
set -euo pipefail

# shellcheck source=stage-payload.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/stage-payload.sh"

commit="${CINNABAR_BUILD_COMMIT:-}"
if [[ -z "$commit" ]]; then
    commit="$(cd "$repo_root" && git rev-parse HEAD)"
fi
printf -- '-X main.version=%s -X main.commit=%s\n' "$(version_of)" "$commit"
