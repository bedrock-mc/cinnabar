#!/usr/bin/env python3
"""Changelog categories for pull requests, and the Discord release post built from them.

Usage:
  changelog.py label <pr-number>  Adds the category label to a PR that has none.
  changelog.py discord <tag>      Posts the release's changelog to DISCORD_CHANGELOG_WEBHOOK (skips when unset).
"""

import json
import os
import re
import subprocess
import sys
import urllib.request

# Ordered: the first matching rule wins, and the label names are the repo's category labels.
CATEGORIES = [
    ("internal", "🔧 Internal", r"^(chore|ci|docs?|test|style|refactor|build|revert|merge|bump|repin)\b"),
    ("performance", "⚡ Performance", r"^(perf\b|reduce|speed|cache|pool|prewarm|bound|optimi[sz]e|skip|share)"),
    ("fix", "🛠 Fixes", r"^(fix|restore|correct|repair|prevent|keep|match|resolve|release|stop|avoid|handle|never)"),
    ("new", "✨ New", r"^(feat\b|add|implement|wire|enable|support|introduce|allow|show|expose|polish|improve)"),
]
LABELS = [label for label, _, _ in CATEGORIES]
HEADINGS = {label: heading for label, heading, _ in CATEGORIES}
DISPLAY_ORDER = ["new", "fix", "performance"]
DEFAULT_LABEL = "new"
EMBED_LIMIT = 4096  # Discord's embed description cap
COLOR = 0x4E5058  # neutral slate
ENTRY = re.compile(r"^\* (?P<title>.+?) by @\S+ in (?P<url>https://\S+/pull/(?P<number>\d+))\s*$")
PREFIX = re.compile(r"^\w+(\([^)]*\))?!?:\s*")


def plain_title(title: str) -> str:
    """Drops a conventional-commit prefix and capitalises the summary."""
    text = PREFIX.sub("", title).strip()
    return text[:1].upper() + text[1:]


# Work players can't see, wherever it appears in a title.
INTERNAL_TERMS = r"\b(ci|clippy|lint|verify gate|test fakes?|fixtures?|refs map|merge markers)\b"


def classify(title: str) -> str:
    """Returns the category label a PR title implies."""
    if re.search(INTERNAL_TERMS, title, re.I):
        return "internal"
    for label, _, pattern in CATEGORIES:
        if re.match(pattern, title, re.I) or re.match(pattern, plain_title(title), re.I):
            return label
    return DEFAULT_LABEL


def gh(*args: str) -> str:
    return subprocess.check_output(["gh", *args], text=True)


def label_pr(number: str) -> None:
    pr = json.loads(gh("pr", "view", number, "--json", "title,labels"))
    if any(label["name"] in LABELS for label in pr["labels"]):
        return
    label = classify(pr["title"])
    gh("pr", "edit", number, "--add-label", label)
    print(f"Labelled #{number} {label}")


def release_entries(tag: str) -> list[dict]:
    """Returns the PRs merged since the previous stable release, from GitHub's generated notes."""
    releases = json.loads(gh("release", "list", "--limit", "100", "--json", "tagName,isPrerelease,isDraft,publishedAt"))
    stable = sorted(
        (r for r in releases if r["tagName"].startswith("v") and not r["isPrerelease"] and not r["isDraft"]),
        key=lambda r: r["publishedAt"] or "",
    )
    tags = [r["tagName"] for r in stable]
    args = ["api", "repos/{owner}/{repo}/releases/generate-notes", "-f", f"tag_name={tag}"]
    if tag in tags and tags.index(tag) > 0:
        args += ["-f", f"previous_tag_name={tags[tags.index(tag) - 1]}"]
    body = json.loads(gh(*args))["body"]
    entries = []
    for line in body.splitlines():
        match = ENTRY.match(line.strip())
        if match:
            entries.append({"title": match["title"], "url": match["url"], "number": match["number"]})
    return entries


def pr_label(number: str, title: str) -> str:
    labels = json.loads(gh("pr", "view", number, "--json", "labels"))["labels"]
    named = [label["name"] for label in labels if label["name"] in LABELS]
    return named[0] if named else classify(title)


def description(sections: dict, internal: int, compare_url: str, budget: int = EMBED_LIMIT) -> str:
    """Formats grouped entries within the embed cap, linking the full changelog last."""
    footer = f"[Full changelog →]({compare_url})" if compare_url else ""
    lines: list[str] = []
    for label in DISPLAY_ORDER:
        if not sections.get(label):
            continue
        lines += ([""] if lines else []) + [f"**{HEADINGS[label]}**"]
        lines += [f"• {plain_title(e['title'])} [#{e['number']}]({e['url']})" for e in sections[label]]
    if internal:
        lines += ["", f"-# +{internal} internal change{'s' if internal != 1 else ''}"]
    if not lines:
        lines = ["No player-facing changes."]
    tail = f"\n\n{footer}" if footer else ""
    text = "\n".join(lines)
    while len(text) + len(tail) > budget:
        lines.pop()
        text = "\n".join(lines + ["…"])
    return text + tail


def payload(tag: str, url: str, published: str, entries: list[dict], labels: dict, compare_url: str) -> dict:
    """Builds the webhook message; the webhook's own name and avatar identify the product."""
    sections: dict = {}
    for entry in entries:
        sections.setdefault(labels[entry["number"]], []).append(entry)
    internal = len(sections.pop("internal", []))
    shown = len(entries) - internal
    header = f"{shown} change{'s' if shown != 1 else ''}\n\n"
    embed = {
        "title": tag,
        "url": url,
        "description": header + description(sections, internal, compare_url, EMBED_LIMIT - len(header)),
        "color": COLOR,
    }
    if published:
        embed["timestamp"] = published
    return {"embeds": [embed], "allowed_mentions": {"parse": []}}


def post_discord(tag: str) -> None:
    webhook = os.environ.get("DISCORD_CHANGELOG_WEBHOOK", "")
    if not webhook:
        print("::warning::DISCORD_CHANGELOG_WEBHOOK is unset; skipping the Discord changelog")
        return
    release = json.loads(gh("release", "view", tag, "--json", "tagName,url,body,publishedAt"))
    entries = release_entries(tag)
    labels = {e["number"]: pr_label(e["number"], e["title"]) for e in entries}
    compare = re.search(r"\*\*Full Changelog\*\*: (\S+)", release.get("body") or "")
    message = payload(tag, release["url"], release.get("publishedAt") or "", entries, labels,
                      compare.group(1) if compare else "")
    request = urllib.request.Request(webhook, data=json.dumps(message).encode(),
                                     headers={"Content-Type": "application/json", "User-Agent": "cinnabar-release"})
    with urllib.request.urlopen(request, timeout=30) as response:
        print(f"Posted the {tag} changelog to Discord ({response.status})")


def main() -> int:
    if len(sys.argv) != 3 or sys.argv[1] not in ("label", "discord"):
        print(__doc__, file=sys.stderr)
        return 2
    (label_pr if sys.argv[1] == "label" else post_discord)(sys.argv[2])
    return 0


if __name__ == "__main__":
    sys.exit(main())
