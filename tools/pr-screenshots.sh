#!/bin/bash
# pr-screenshots.sh <pr-number> <image>...: posts images on a PR as one comment.
# Images are uploaded as assets of the `pr-screenshots` release (never committed); captions come from file names.
set -euo pipefail
[ $# -ge 2 ] || { echo "usage: $0 <pr-number> <image>..." >&2; exit 2; }
pr=$1; shift
repo=$(gh repo view --json nameWithOwner --jq .nameWithOwner)
tag=pr-screenshots
if ! gh release view "$tag" --repo "$repo" >/dev/null 2>&1; then
  gh release create "$tag" --repo "$repo" --prerelease --title "PR screenshots" \
    --notes "Screenshot assets attached to pull requests; not part of any release." >/dev/null
fi
staging=$(mktemp -d)
trap 'rm -rf "$staging"' EXIT
body="Screenshots"$'\n'
stamp=$(date +%Y%m%d%H%M%S)
for image in "$@"; do
  [ -f "$image" ] || { echo "missing $image" >&2; exit 1; }
  base=$(basename "$image")
  name="pr${pr}-${stamp}-${base// /-}"
  cp "$image" "$staging/$name"
  gh release upload "$tag" "$staging/$name" --repo "$repo" --clobber >/dev/null
  caption=${base%.*}
  body+=$'\n'"**${caption//[-_]/ }**"$'\n\n'"![${caption}](https://github.com/${repo}/releases/download/${tag}/${name})"$'\n'
done
printf '%s' "$body" > "$staging/comment.md"
gh pr comment "$pr" --repo "$repo" --body-file "$staging/comment.md"
