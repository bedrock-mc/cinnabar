"""Check the standalone site's build and GitHub download contract."""

from html.parser import HTMLParser
import importlib.util
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from urllib.parse import urlsplit


SITE = Path(__file__).resolve().parents[1]
SOURCE = SITE.parent
SPEC = importlib.util.spec_from_file_location("site_render", SITE / "render.py")
RENDER = importlib.util.module_from_spec(SPEC)
SPEC.loader.exec_module(RENDER)


class Page(HTMLParser):
    def __init__(self):
        super().__init__()
        self.scripts = []
        self.downloads = []
        self.repository = None

    def handle_starttag(self, tag, attributes):
        attrs = dict(attributes)
        if tag == "script":
            self.scripts.append(attrs)
        if tag == "a" and "data-release-asset" in attrs:
            self.downloads.append(attrs["data-release-asset"])
        if tag == "a" and attrs.get("class") == "github":
            self.repository = attrs["href"]


class RenderTests(unittest.TestCase):
    def setUp(self):
        temporary = tempfile.TemporaryDirectory()
        self.addCleanup(temporary.cleanup)
        self.root = Path(temporary.name)
        self.repository = self.root / "source"
        self.output = self.root / "candidate"
        self.config = json.loads((SOURCE / "packaging/release-assets.json").read_text())
        self.config["repository"] = "fixture/download-contract"
        self.config["website"] = "https://unrelated-site.invalid"
        self.config["install_script"] = "fixture-" + self.config["install_script"]
        self.config["assets"] = {
            platform: {arch: "fixture-" + name for arch, name in arches.items()}
            for platform, arches in self.config["assets"].items()
        }
        for path in ("packaging/icons/cinnabar.svg", "assets/branding/title.png"):
            target = self.repository / path
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copyfile(SOURCE / path, target)
        (self.repository / "packaging/release-assets.json").write_text(json.dumps(self.config))

    def config_from_output(self):
        script = (self.output / "downloads.js").read_text()
        return json.loads(script.removeprefix("window.CINNABAR_DOWNLOADS = ").removesuffix(";\n"))

    def test_build_requires_only_artwork_and_manifest_and_redirects_to_release_assets(self):
        RENDER.build(self.repository, self.output)
        config = self.config_from_output()
        self.assertEqual(config["assets"], self.config["assets"])
        release = urlsplit(config["release_url"])
        self.assertEqual(release.scheme, "https")
        self.assertEqual(release.path, f'/{self.config["repository"]}/releases/latest')
        self.assertEqual(config["download_base"], config["release_url"] + "/download/")
        self.assertEqual(config["install_command"],
                         f'curl -fsSL {config["download_base"]}{self.config["install_script"]} | sh')
        self.assertNotIn(self.config["website"], config["install_command"])
        self.assertEqual(config["nightly_base"],
                         f'https://github.com/{self.config["repository"]}/releases/download/nightly/')
        self.assertEqual(config["nightly_install_command"], config["install_command"] + " -s -- --channel nightly")
        self.assertEqual({path.name for path in self.output.iterdir()}, {
            "index.html", "downloads.js", "app.js", "title.png", "texture.svg",
        })
        self.assertFalse((self.output / self.config["install_script"]).exists())

    def test_rendered_page_assets_and_mac_first_open_guidance_are_complete(self):
        RENDER.build(self.repository, self.output)
        html = (self.output / "index.html").read_text()
        self.assertNotIn("@@", html)
        page = Page()
        page.feed(html)
        config = self.config_from_output()
        self.assertEqual(page.repository + "/releases/latest", config["release_url"])
        self.assertEqual([script["src"] for script in page.scripts], ["/downloads.js", "/app.js"])
        for script in page.scripts:
            self.assertIn("defer", script)
            self.assertTrue((self.output / script["src"].lstrip("/")).is_file())
        for selection in page.downloads:
            platform, architecture = selection.split("/")
            self.assertIn(architecture, config["assets"][platform])
        self.assertIn('aria-haspopup="menu"', html)
        self.assertEqual(html.count('role="menuitemradio"'), 2)
        self.assertIn("Nightly (Unstable)", html)
        self.assertIn("Built daily from dev and may break.", html)
        self.assertIn("If macOS blocks the first launch", html)
        self.assertIn('class="first-open"', html)
        self.assertEqual((self.output / "title.png").read_bytes(),
                         (self.repository / "assets/branding/title.png").read_bytes())


if __name__ == "__main__":
    unittest.main()
