#!/usr/bin/env bash
# Fails when the built site holds anything but the editor's own files, so no
# pack content (Mojang or otherwise) can ship. Usage: check-site.sh SITE_DIR
set -euo pipefail
site="$1"
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
expected="$(
    printf '%s\n' .nojekyll index.html style.css app.js pkg/jsonui_editor.js pkg/jsonui_editor_bg.wasm \
        cinnangles-sans.mcbefont examples/files.json
    (cd "$here/examples" && find . -type f | sed 's|^\./|examples/|')
)"
status=0
while IFS= read -r file; do
    if ! grep -qxF "$file" <<<"$expected"; then
        printf 'unexpected file in site: %s\n' "$file" >&2
        status=1
    fi
done < <(cd "$site" && find . -type f | sed 's|^\./||' | sort)
exit $status
