"""Build pixel-grid faces and pair spacing with our own outline generator."""
import functools
import json
import unicodedata
from pathlib import Path

import numpy as np
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont, newTable
from fontTools.ttLib.scaleUpem import scale_upem
from fontTools.ttLib.tables._k_e_r_n import KernTable_format_0

UNITS_PER_EM = 1000
RASTER_EM = 20
SOURCE = Path(__file__).with_name("face_designs")


def read_design(face):
    """Read the face's pixel cells, dimensions and spacing in font units."""
    return json.loads((SOURCE / f"{face}.json").read_text())


def ink_boxes(record):
    """Return the record's ink cells as (x0, y0, x1, y1) boxes in font units."""
    xs = record["x"] + np.concatenate(([0], np.cumsum(record["columns"])))
    tops = record["y"] + sum(record["heights"]) - np.concatenate(([0], np.cumsum(record["heights"])))
    return [(xs[i], tops[j + 1], xs[i + 1], tops[j])
            for j, row in enumerate(record["rows"]) for i, cell in enumerate(row) if cell == "#"]


PUNCTUATION = set(".,:;!?'\"()-\u2013\u2014\u2018\u2019\u201a\u201c\u201d\u201e\u2026")


def kern_class(cp):
    """Letters by script, digits, common punctuation, or None for glyphs never kerned."""
    ch = chr(cp)
    if ch in PUNCTUATION:
        return "punctuation"
    kind = unicodedata.category(ch)[0]
    return unicodedata.name(ch).split()[0] if kind == "L" else "digit" if kind == "N" else None


def kernable(left, right):
    """Same-script letters, digits together, or either beside common punctuation."""
    pair = {kern_class(left), kern_class(right)}
    return None not in pair and (len(pair) == 1 and pair != {"punctuation"} or "punctuation" in pair)


@functools.lru_cache(maxsize=None)
def kerning(face, limit=2):
    """Return (left, right, value) pairs that close open pairs toward the face's spacing.

    Each pair is measured on quarter-pixel bands, with the left ink dilated by one pixel
    vertically. It tightens by the smaller of the narrowest excess gap and half the mean
    excess, in whole pixels, at most `limit` pixels.
    """
    design = read_design(face)
    pixel, spacing = design["pixel"], design["spacing"]
    cps = [int(cp) for cp, record in design["glyphs"].items() if record["rows"]]
    boxes = [ink_boxes(design["glyphs"][str(cp)]) for cp in cps]
    advance = np.array([design["glyphs"][str(cp)]["advance"] for cp in cps], float)
    low = min(b[1] for bs in boxes for b in bs)
    high = max(b[3] for bs in boxes for b in bs)
    bands = np.arange(low, high, pixel / 4) + pixel / 8
    right = np.full((len(cps), len(bands)), -np.inf)
    left = np.full((len(cps), len(bands)), np.inf)
    for g, glyph_boxes in enumerate(boxes):
        for x0, y0, x1, y1 in glyph_boxes:
            near = (bands > y0 - pixel) & (bands < y1 + pixel)
            right[g, near] = np.maximum(right[g, near], x1)
            inside = (bands > y0) & (bands < y1)
            left[g, inside] = np.minimum(left[g, inside], x0)
    gaps = advance[:, None, None] - right[:, None, :] + left[None, :, :] - spacing
    valid = np.isfinite(gaps)
    count = valid.sum(2)
    narrowest = np.where(valid, gaps, np.inf).min(2)
    mean = np.where(valid, gaps, 0).sum(2) / np.maximum(count, 1)
    steps = np.minimum(np.floor(narrowest / pixel), np.floor(mean / (2 * pixel)))
    steps = np.where(count > 0, np.clip(steps, 0, limit), 0)
    return [(cps[i], cps[j], -int(steps[i, j]) * pixel) for i, j in zip(*np.nonzero(steps))
            if kernable(cps[i], cps[j])]


def grid_glyph(record, trace):
    """Trace pixel cells into fresh outlines, retaining their measured widths and heights."""
    pen = TTGlyphPen(None)
    if record["rows"]:
        pixels = np.array([[cell == "#" for cell in row] for row in record["rows"]])
        xs = np.concatenate(([record["x"]], record["x"] + np.cumsum(record["columns"])))
        ys = np.concatenate(([record["y"]], record["y"] + np.cumsum(record["heights"][::-1])))
        for loop in trace(pixels, step=1, bottom=0):
            points = [(int(xs[x]), int(ys[y])) for x, y in loop]
            pen.moveTo(points[0])
            for point in points[1:]:
                pen.lineTo(point)
            pen.closePath()
    glyph = pen.glyph()
    glyph.recalcBounds(None)
    return glyph


def apply_design(face, path, trace):
    """Replace designed glyphs while preserving every remaining fallback drawing."""
    design = read_design(face)
    font = TTFont(path, recalcTimestamp=False)
    scale_upem(font, UNITS_PER_EM)
    cmap = dict(font.getBestCmap())
    order = list(font.getGlyphOrder())
    for cp, record in design["glyphs"].items():
        cp = int(cp)
        if cp not in cmap:
            continue
        name = f"pixel{cp:06X}"
        glyph = grid_glyph(record, trace)
        font["glyf"][name] = glyph
        font["hmtx"][name] = (record["advance"], record["x"])
        order.append(name)
        cmap[cp] = name
    font.setGlyphOrder(order)
    font["glyf"].glyphOrder = order
    font["maxp"].numGlyphs = len(order)
    for table in font["cmap"].tables:
        if table.isUnicode():
            table.cmap = {cp: name for cp, name in cmap.items() if cp <= 0xFFFF or table.format >= 12}
    kern = newTable("kern")
    kern.version = 0
    subtable = KernTable_format_0()
    subtable.version, subtable.coverage = 0, 1
    subtable.kernTable = {(cmap[left], cmap[right]): value for left, right, value in kerning(face)
                          if left in cmap and right in cmap}
    kern.kernTables = [subtable]
    font["kern"] = kern
    font["hhea"].ascent, font["hhea"].descent = design["ascent"], design["descent"]
    font["hhea"].lineGap = 0
    font["OS/2"].sxHeight = design["x_height"]
    font["OS/2"].sCapHeight = 700
    font["OS/2"].recalcAvgCharWidth(font)
    font.save(path)
