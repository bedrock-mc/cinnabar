"""PR titles map to changelog categories, and Discord posts group them within Discord's cap."""

import importlib.util
from pathlib import Path
import unittest

SPEC = importlib.util.spec_from_file_location("changelog", Path(__file__).resolve().parents[1] / "changelog.py")
changelog = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(changelog)


def entry(number, title):
    return {"number": str(number), "title": title, "url": f"https://github.com/o/r/pull/{number}"}


class ChangelogTest(unittest.TestCase):
    def test_titles_map_to_categories(self):
        cases = {
            "Add third-person and crosshair color settings": "new",
            "feat(ui): add crosshair colors": "new",
            "Polish OreUI menus, motion and dark mode": "new",
            "Fix limb swing boosts on consecutive damage": "fix",
            "Restore absorption hearts in the native HUD": "fix",
            "fix(authcache): never hold the account lock across network I/O": "fix",
            "Reduce foliage fragment tint work": "performance",
            "perf: share light and mesh dispatch snapshots": "performance",
            "chore(go): repin gophertunnel fork": "internal",
            "Revert \"Make local world choices independent\"": "internal",
            "Something unusual": changelog.DEFAULT_LABEL,
        }
        for title, label in cases.items():
            self.assertEqual(changelog.classify(title), label, title)

    def test_post_groups_by_label_and_collapses_internal_changes(self):
        entries = [entry(1, "Add a thing"), entry(2, "fix(ui): broken thing"), entry(3, "Retune CI caches")]
        labels = {"1": "new", "2": "fix", "3": "internal"}
        embed = changelog.payload("v1.0.0", "https://r", "", entries, labels, "https://cmp")["embeds"][0]
        text = embed["description"]
        self.assertTrue(text.startswith("2 changes"))
        self.assertLess(text.index("✨ New"), text.index("🛠 Fixes"))
        self.assertIn("• Broken thing [#2](https://github.com/o/r/pull/2)", text)
        self.assertNotIn("Retune CI caches", text)
        self.assertIn("+1 internal change", text)
        self.assertTrue(text.endswith("[Full changelog →](https://cmp)"))

    def test_long_posts_stay_within_the_embed_cap(self):
        entries = [entry(n, f"Fix a long descriptive problem number {n} in the renderer") for n in range(300)]
        labels = {e["number"]: "fix" for e in entries}
        text = changelog.payload("v1", "https://r", "", entries, labels, "https://cmp")["embeds"][0]["description"]
        self.assertLessEqual(len(text), changelog.EMBED_LIMIT)
        self.assertTrue(text.endswith("[Full changelog →](https://cmp)"))

    def test_never_pings_anyone(self):
        message = changelog.payload("v1", "https://r", "", [], {}, "")
        self.assertEqual(message["allowed_mentions"], {"parse": []})


if __name__ == "__main__":
    unittest.main()
