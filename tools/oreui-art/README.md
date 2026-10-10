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
The client build embeds the manifest's images. Native control drawing supplies art that
has no raster replacement; optional prepared panorama crops replace the scenery.

## Local comparison

The offline UI gallery writes rendered screens to `CINNABAR_FORM_SNAPSHOT_DIR`.
To compare three already captured sets, place matching PNG filenames in `game/`,
`none/` and `new/` below an evidence directory outside every Git checkout, then run:

```sh
python3 tools/oreui-art/compare.py /path/to/evidence
```

This writes a self-contained `compare.html` and numbered PNG pages. It reads only
those screenshots. Comparison files may contain game art and must never be committed
or redistributed. Review findings belong in the private task report.
