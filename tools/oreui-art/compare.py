#!/usr/bin/env python3
"""Compare matching offline screen captures without reading an installed game."""

import argparse
import base64
import html
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

COLUMNS = (("game", "Game art"), ("none", "No game (old fallback)"), ("new", "New Cinnabar art"))


def destination(path):
    """Require evidence to remain outside every Git checkout."""
    path = path.resolve()
    if any((parent / ".git").exists() for parent in (path, *path.parents)):
        raise ValueError("Comparison output contains game art and must stay outside Git checkouts.")
    return path


def captures(root):
    """Require the same nonempty set of screenshot names in all three columns."""
    sets = [{path.name for path in (root / directory).glob("*.png")} for directory, _ in COLUMNS]
    if not sets[0] or any(names != sets[0] for names in sets[1:]):
        raise ValueError("The game, none and new directories must have matching PNG filenames.")
    return sorted(sets[0])


def write_html(root, names):
    """Embed full-resolution screenshots in a standalone three-column comparison."""
    rows = []
    for name in names:
        images = []
        for directory, label in COLUMNS:
            encoded = base64.b64encode((root / directory / name).read_bytes()).decode("ascii")
            images.append(f'<figure><figcaption>{html.escape(label)}</figcaption>'
                          f'<a href="data:image/png;base64,{encoded}" target="_blank">'
                          f'<img alt="{html.escape(name)}: {html.escape(label)}" '
                          f'src="data:image/png;base64,{encoded}"></a></figure>')
        rows.append(f'<section><h2>{html.escape(Path(name).stem)}</h2><div>{"".join(images)}</div></section>')
    page = '''<!doctype html><html lang="en"><meta charset="utf-8">
<meta name="viewport" content="width=device-width"><title>OreUI art comparison</title>
<style>
body{margin:24px;background:#18202a;color:#eee;font:14px system-ui}h1{font-size:26px}
section{border-top:1px solid #607080;padding:16px 0}h2{font-size:18px}
section>div{display:grid;grid-template-columns:repeat(3,minmax(0,1fr));gap:12px}
figure{margin:0}figcaption{margin-bottom:8px}img{width:100%;image-rendering:pixelated}
</style><h1>OreUI art comparison</h1>
<p>Offline 2560×1440 captures of matching screen fixtures. Local review only: contains game art.</p>
''' + "".join(rows) + "</html>"
    (root / "compare.html").write_text(page)


def write_pages(root, names):
    """Build printable contact sheets with eight screen rows per PNG page."""
    width, cell_height, row_height = 640, 360, 395
    font = ImageFont.load_default(size=18)
    for page, start in enumerate(range(0, len(names), 8), 1):
        batch = names[start:start + 8]
        sheet = Image.new("RGB", (width * 3, 45 + row_height * len(batch)), "#18202a")
        draw = ImageDraw.Draw(sheet)
        for column, (_, label) in enumerate(COLUMNS):
            draw.text((column * width + 8, 12), label, fill="white", font=font)
        for row, name in enumerate(batch):
            y = 45 + row * row_height
            draw.text((8, y), Path(name).stem, fill="white", font=font)
            for column, (directory, _) in enumerate(COLUMNS):
                with Image.open(root / directory / name) as image:
                    image = image.convert("RGB")
                    image.thumbnail((width, cell_height), Image.Resampling.LANCZOS)
                    sheet.paste(image, (column * width, y + 28))
        sheet.save(root / f"compare-{page:02}.png")


def main():
    """Read only the three capture directories and write local comparison artifacts."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("evidence", type=Path)
    root = destination(parser.parse_args().evidence)
    names = captures(root)
    write_html(root, names)
    write_pages(root, names)
    print(f"Compared {len(names)} screens in {root / 'compare.html'}")


if __name__ == "__main__":
    main()
