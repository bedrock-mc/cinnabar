#!/usr/bin/env python3
"""Render the standalone site using Cinnabar's original artwork and release filenames."""

import argparse
import json
from pathlib import Path
import random
import shutil
from urllib.parse import quote
import xml.etree.ElementTree as ET


HERE = Path(__file__).resolve().parent
REPOSITORY = HERE.parent


def build(repository, output):
    config = json.loads((repository / "packaging/release-assets.json").read_text())
    repository_url = "https://github.com/" + config["repository"]
    config["release_url"] = repository_url + "/releases/latest"
    config["download_base"] = config["release_url"] + "/download/"
    config["install_command"] = (
        f'curl -fsSL {config["download_base"]}{config["install_script"]} | sh'
    )
    config["nightly_base"] = repository_url + "/releases/download/nightly/"
    config["nightly_install_command"] = config["install_command"] + " -s -- --channel nightly"
    icon = (repository / "packaging/icons/cinnabar.svg").read_text()
    red = ET.fromstring(icon).find("{http://www.w3.org/2000/svg}rect").attrib["fill"]
    page = (HERE / "index.html.in").read_text()
    page = page.replace("@@RED@@", red)
    page = page.replace("@@FAVICON@@", "data:image/svg+xml," + quote(icon, safe=""))
    page = page.replace("@@REPOSITORY_URL@@", repository_url)
    output.mkdir(parents=True, exist_ok=True)
    (output / "index.html").write_text(page)
    (output / "downloads.js").write_text("window.CINNABAR_DOWNLOADS = " + json.dumps(config) + ";\n")
    shutil.copyfile(HERE / "app.js", output / "app.js")
    shutil.copyfile(repository / "assets/branding/title.png", output / "title.png")
    rng = random.Random(27)
    colors = ["#1c221d", "#202620", "#242a23", "#262d25", "#1f251f"]
    pixels = "".join(
        f'<rect x="{x}" y="{y}" width="16" height="16" fill="{rng.choice(colors)}"/>'
        for y in range(0, 256, 16) for x in range(0, 256, 16)
    )
    (output / "texture.svg").write_text(
        '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 256 256">' + pixels + "</svg>"
    )


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=HERE / "dist")
    args = parser.parse_args()
    build(REPOSITORY, args.output)
