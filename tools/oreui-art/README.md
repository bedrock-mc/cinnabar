# Original OreUI art

The source in this folder draws the replacements listed in
[`assets/oreui/manifest.json`](../../assets/oreui/manifest.json). It uses text grids,
integer shapes, a hand-chosen palette, and deterministic scene details. It does not
read installed game images, fonts, textures, screenshots, or other drawing inputs.
The artwork and generator use the repository's [license](../../LICENSE).

The manifest maps each stable output filename to its replacement key, full pixel
size, stored frame count, frame size, drawing recipe, and animation timing.
All outputs are PNG except the loading GIF. JPEG replacement keys intentionally
map to PNG output files so the original pixel edges remain sharp.

```sh
python3 -m pip install -r tools/oreui-art/requirements.txt
python3 tools/oreui-art/render.py
python3 tools/oreui-art/render.py --check
```

The check regenerates all images in temporary storage, compares their bytes, and
checks dimensions, GIF frame count, and GIF timing. No Rust build or installed
game is needed. `--output DIRECTORY` can write a separate set of original renders.

## Animation cells

`frames` counts stored cells, including terminal cells. Sprite sheets run from
left to right. Their timing lives in the manifest because PNG files do not carry
playback timing:

- Pending ping has seven 16×20 cells. Six steps span 700 ms; the last cell repeats
  the first and marks the loop endpoint.
- Icon highlight has nine 24×24 cells. Eight steps span 500 ms; the last cell is
  transparent and stays displayed after the transition.
- Loading has ten 28×28 GIF frames, each displayed for 100 ms, repeating forever.

An integration must use the declared frame dimensions and playback steps
separately. Dividing sheet width by playback steps does not give the cell width.
The artwork is prepared for review; this change does not wire it into the client.

## Local comparison

Only `compare.py` reads reference images. It uses them to validate dimensions and
GIF timing and to build a side-by-side review, with every animation cell shown in
order. It never supplies reference pixels to the drawing code. Both columns use
the same nearest-neighbour display scale. The HTML file embeds all images and
lets the reviewer select fit, 1×, 2×, or 3× views and search by image name or key.

```sh
python3 tools/oreui-art/compare.py \
  --bundle /path/to/installed/hbui \
  --output /path/outside/all/checkouts/oreui-review
```

This writes `sheet.html` and numbered PNG pages. Comparison files contain game
images and must never be committed or redistributed. The tool rejects output
inside any Git checkout or the installed bundle. Source-location notes, the full
coverage audit, and review findings belong in the private task report.
