#!/usr/bin/env bash
# Builds the Figma plugin into tools/figma-plugin/dist: the JSON-UI editor's
# WebAssembly through the editor's own build, then the TypeScript shell.
# Usage: build.sh [CINNANGLES_SANS_TTF]
# Needs what tools/jsonui-editor/build.sh needs, plus Node.js and npm.
set -euo pipefail

here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
repo="$(CDPATH= cd -- "$here/../.." && pwd)"
scratch="$(mktemp -d)"
trap 'rm -rf "$scratch"' EXIT

JSONUI_EDITOR_TYPESCRIPT=1 bash "$repo/tools/jsonui-editor/build.sh" "$scratch/site" "$@"
rm -rf "$here/pkg"
cp -R "$scratch/site/pkg" "$here/pkg"
if [[ -f "$scratch/site/cinnangles-sans.mcbefont" ]]; then
    cp "$scratch/site/cinnangles-sans.mcbefont" "$here/pkg/"
fi

cd "$here"
npm ci --no-audit --no-fund
npm run typecheck
npm run build
printf 'Built %s; import %s in Figma\n' "$here/dist" "$here/manifest.json"
