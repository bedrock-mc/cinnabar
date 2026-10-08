#!/usr/bin/env python3
"""Posts a published release's changelog to a Discord channel webhook as an embed.

Usage: discord-changelog.py <tag>. Reads DISCORD_CHANGELOG_WEBHOOK; exits quietly when it is unset.
"""

import json
import os
import re
import subprocess
import sys
import urllib.request

EMBED_LIMIT = 4096  # Discord's embed description cap
COLOR = 0xE0453A
ENTRY = re.compile(r"^\* (?P<title>.+?) by @(?P<author>\S+) in (?P<url>https://\S+/pull/(?P<number>\d+))\s*$")


def entries(body: str) -> list[str]:
    """Returns one bullet per merged pull request listed under "What's Changed"."""
    lines = []
    for line in body.splitlines():
        match = ENTRY.match(line.strip())
        if match:
            lines.append(f"• {match['title']} ([#{match['number']}]({match['url']}))")
    return lines


def description(lines: list[str], release_url: str) -> str:
    """Joins bullets within the embed cap, ending with a link when some are cut."""
    if not lines:
        return f"No pull requests listed. [View the release]({release_url})"
    kept: list[str] = []
    for index, line in enumerate(lines):
        more = len(lines) - index
        tail = f"\n…and {more} more in the [full release notes]({release_url})"
        if len("\n".join(kept + [line])) + len(tail) > EMBED_LIMIT:
            return "\n".join(kept) + tail
        kept.append(line)
    return "\n".join(kept)


def payload(release: dict) -> dict:
    """Builds the webhook message for one release."""
    body = release.get("body") or ""
    compare = re.search(r"\*\*Full Changelog\*\*: (\S+)", body)
    embed = {
        "title": f"{release['tagName']} released",
        "url": release["url"],
        "description": description(entries(body), release["url"]),
        "color": COLOR,
    }
    if release.get("publishedAt"):
        embed["timestamp"] = release["publishedAt"]
    if compare:
        embed["fields"] = [{"name": "Full changelog", "value": compare.group(1), "inline": False}]
    # The webhook's own name and avatar identify the product.
    return {"embeds": [embed], "allowed_mentions": {"parse": []}}


def main() -> int:
    tag = sys.argv[1]
    webhook = os.environ.get("DISCORD_CHANGELOG_WEBHOOK", "")
    if not webhook:
        print("::warning::DISCORD_CHANGELOG_WEBHOOK is unset; skipping the Discord changelog")
        return 0
    release = json.loads(subprocess.check_output(
        ["gh", "release", "view", tag, "--json", "tagName,url,body,publishedAt"], text=True))
    request = urllib.request.Request(
        webhook,
        data=json.dumps(payload(release)).encode(),
        headers={"Content-Type": "application/json", "User-Agent": "cinnabar-release"},
    )
    with urllib.request.urlopen(request, timeout=30) as response:
        print(f"Posted the {tag} changelog to Discord ({response.status})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
