"""Discord changelog embeds list every merged pull request and respect Discord's size cap."""

import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location(
    "discord_changelog", Path(__file__).resolve().parents[1] / "discord-changelog.py")
changelog = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(changelog)

BODY = """## Installing

- Linux: run it.

## What's Changed
* Fix 17 gameplay bugs by @RestartFU in https://github.com/bedrock-mc/cinnabar/pull/245
* Restore absorption hearts by @HashimTheArab in https://github.com/bedrock-mc/cinnabar/pull/285

**Full Changelog**: https://github.com/bedrock-mc/cinnabar/compare/v0.1.11...v0.1.12
"""
RELEASE = {
    "tagName": "v0.1.12",
    "url": "https://github.com/bedrock-mc/cinnabar/releases/tag/v0.1.12",
    "body": BODY,
    "publishedAt": "2026-10-07T13:09:53Z",
}


class DiscordChangelogTest(unittest.TestCase):
    def test_lists_each_pull_request_with_a_link(self):
        embed = changelog.payload(RELEASE)["embeds"][0]
        self.assertEqual(embed["title"], "v0.1.12 released")
        self.assertEqual(embed["url"], RELEASE["url"])
        self.assertEqual(embed["description"].splitlines(), [
            "• Fix 17 gameplay bugs ([#245](https://github.com/bedrock-mc/cinnabar/pull/245))",
            "• Restore absorption hearts ([#285](https://github.com/bedrock-mc/cinnabar/pull/285))",
        ])
        self.assertIn("compare/v0.1.11...v0.1.12", embed["fields"][0]["value"])

    def test_long_changelogs_stay_within_the_embed_cap(self):
        lines = [f"• Change {n} with a fairly long descriptive title ([#{n}](https://x/pull/{n}))"
                 for n in range(200)]
        text = changelog.description(lines, RELEASE["url"])
        self.assertLessEqual(len(text), changelog.EMBED_LIMIT)
        self.assertIn("more in the [full release notes]", text)

    def test_never_pings_anyone(self):
        self.assertEqual(changelog.payload(RELEASE)["allowed_mentions"], {"parse": []})


if __name__ == "__main__":
    unittest.main()
