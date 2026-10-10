# Cinnangles face generation

The four derived faces combine original pixel drawings in `face_art.py` with the
pinned Cinnangles Sans fallback. `face_metrics.py` declares printable ASCII
advances in thousandths of an em. Advances round to the nearest font unit;
outlines stay on the 64-unit grid. This preserves solid atlas coverage at the
20-pixel raster em. The carrier applies the existing Ten and Seven bearings.

Ten uses individually drawn fourteen-row display letters. Five uses an original
five-row alphabet expanded symmetrically to fourteen texels. Five Bold grows
short strokes inward; its narrow I and 1 have wider ink boxes. Seven uses original
lowercase and digit drawings on its two-texel design grid. Related Greek and
Cyrillic capitals share the adjusted Latin proportions, including accented bases.
Other characters retain the existing Sans-derived drawing rules and coverage.

Reference fonts supply scalar measurements and private visual comparisons only.
The generator never loads them. Do not import reference outlines, bitmaps, or
traced glyphs into the artwork.

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
