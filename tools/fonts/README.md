# Cinnangles face generation

The four faces are original drawings in `letterforms_art.py`. `letterforms.py`
composes accented, Greek and Cyrillic letters from those bases and marks, and
writes the pixel cells, bearings and advances to `face_designs/`:

```sh
python3 tools/fonts/letterforms.py
```

Each face keeps its pixel pitch, stem weight and line metrics: Ten 70 units with
three-pixel stems, Seven 100, Five 140, and Five Bold's 178-unit strokes on Five's
skeletons. Every advance is the ink width plus the face's fixed spacing.

`design_faces.py` traces the cells into new TrueType outlines and derives pair
spacing from the drawn silhouettes. A same-script letter pair, digit pair or
letter/digit beside common punctuation tightens by whole pixels toward the face's
spacing at its narrowest rows, limited by half the mean excess and two pixels.
The same pairs go into the `kern` table and the source manifests. `assetc`
rasterizes the faces unhinted with FreeType into a 52 px distance field and exact
1-9 px coverage, keeping source bearings and advances in 1/64 texels.

`face_art.py`, `face_metrics.py` and the pinned Cinnangles Sans supply the existing
fallback drawings. Generation scales their coordinates to the new em without
changing the pixel pitch. `native_subset.py` then keeps only the Latin, Greek, Cyrillic and
symbol ranges each face draws; Cinnangles Sans draws every other character at runtime.
The generator reads only these committed sources.

Use Python 3.14 with Unicode 16.0.0. The reproducible tool versions are fontTools
4.66.1, numpy 2.5.3, and scipy 1.18.1. From the repository root:

```sh
python3 tools/fonts/build_faces.py --sans assets/fonts/CinnanglesSans.ttf \
  --out assets/fonts --manifests assets ten seven five five-bold
python3 -m unittest discover -s tools/fonts -v
```

Generation retains the pinned Sans timestamps instead of the current time and
sets the generated names and notices in source. To check complete reproduction,
generate into two empty directories, then compare their TTFs and source manifests
with the shipped files. `--reviewed assets/fonts` additionally checks every font
table against existing outputs; omit it while changing the drawings or metrics.

Font provenance and licensing review remains pending as described in
`THIRD_PARTY_NOTICES.md`. No license is asserted for the generated faces.
