"""Original Cinnangles letterforms, composed into the committed face designs.

Rows run top to bottom with '#' for ink; an optional leading integer is the bottom
row's offset from the baseline in design pixels. Five Bold draws the Five skeletons
with heavier, overlapping strokes. `python3 letterforms.py` rewrites face_designs/.
"""
import json
import math
import unicodedata
from dataclasses import dataclass
from pathlib import Path

import numpy as np

from letterforms_art import ALIASES, FIVE, FIVE_MARKS, SEVEN, SEVEN_MARKS, TEN, TEN_MARKS

DESIGNS = Path(__file__).with_name("face_designs")


@dataclass(frozen=True)
class Grid:
    """A face's pixel pitch, stroke, spacing and vertical layout in font units."""
    pitch: float
    stroke: int
    spacing: int  # pen gap after every glyph's ink
    space: int
    cap: int  # cap height in design pixels
    ascent: int
    descent: int
    x_height: int
    cap_gap: int  # empty rows between a cap-height base and its mark
    low_gap: int
    kern_pixel: int
    caps_only: bool


GRIDS = {
    "ten": Grid(70, 70, 105, 245, 10, 990, -250, 700, 1, 1, 70, True),
    "seven": Grid(100, 100, 100, 300, 7, 800, -100, 500, 0, 1, 100, False),
    "five": Grid(140, 140, 140, 420, 5, 1120, -280, 700, 1, 1, 140, True),
    "five-bold": Grid(130.5, 178, 140, 420, 5, 1138, -306, 700, 1, 1, 130, True),
}
ART = {"ten": TEN, "seven": SEVEN, "five": FIVE, "five-bold": FIVE}
MARKS = {"ten": TEN_MARKS, "seven": SEVEN_MARKS, "five": FIVE_MARKS, "five-bold": FIVE_MARKS}
COMBINING = {
    0x300: "grave", 0x301: "acute", 0x302: "circumflex", 0x303: "tilde", 0x304: "macron",
    0x306: "breve", 0x307: "dot", 0x308: "diaeresis", 0x30A: "ring", 0x30B: "dacute",
    0x30C: "caron", 0x326: "comma", 0x327: "cedilla", 0x328: "ogonek",
}
BELOW = {"comma", "cedilla", "ogonek"}
SPACING_MARKS = {"¨": ["diaeresis"], "¯": ["macron"], "´": ["acute"],
                 "¸": ["cedilla"], "΄": ["acute"], "΅": ["diaeresis", "acute"]}
# Cedilla under these letters is the Latvian comma.
COMMA_CEDILLA = set("KkLlNnRr")


class Bitmap:
    """Ink cells with the bottom row's offset from the baseline."""

    def __init__(self, cells, bottom=0):
        self.cells = np.asarray(cells, bool).reshape(len(cells), -1) if len(cells) else np.zeros((0, 0), bool)
        self.bottom = bottom

    @classmethod
    def parse(cls, art):
        bottom, text = art if isinstance(art, tuple) else (0, art)
        rows = text.split("/")
        if len({len(row) for row in rows}) != 1:
            raise ValueError(f"ragged rows: {text}")
        return cls([[c == "#" for c in row] for row in rows], bottom)

    @property
    def width(self):
        return self.cells.shape[1]

    @property
    def top(self):
        """Row above the highest ink row."""
        return self.bottom + self.cells.shape[0]

    def union(self, other, dx, dy):
        """Overlay `other` shifted by (dx, dy) pixels, growing the canvas as needed."""
        left = min(0, dx)
        right = max(self.width, dx + other.width)
        bottom = min(self.bottom, other.bottom + dy)
        top = max(self.top, other.top + dy)
        out = np.zeros((top - bottom, right - left), bool)
        for bm, ox, oy in ((self, 0, 0), (other, dx, dy)):
            r0 = top - (bm.top + oy)
            out[r0:r0 + bm.cells.shape[0], ox - left:ox - left + bm.width] |= bm.cells
        return Bitmap(out, bottom)

    def trimmed(self):
        """Drop empty outer columns and rows."""
        cols = np.flatnonzero(self.cells.any(0))
        rows = np.flatnonzero(self.cells.any(1))
        if not len(cols):
            return Bitmap([], 0)
        cells = self.cells[rows[0]:rows[-1] + 1, cols[0]:cols[-1] + 1]
        return Bitmap(cells, self.bottom + self.cells.shape[0] - 1 - rows[-1])


def drawing(face, ch):
    """Return the face's own drawing for `ch`, following aliases and caps folding."""
    art = ART[face]
    if ch in art:
        return Bitmap.parse(art[ch])
    if GRIDS[face].caps_only:
        folded = ALIASES["caps"].get(ch) or (ch.upper() if len(ch.upper()) == 1 else None)
        if folded and folded != ch:
            return glyph(face, folded)
    if ch in ALIASES["shape"]:
        return glyph(face, ALIASES["shape"][ch])
    return None


def mark(face, name):
    return Bitmap.parse(MARKS[face][name])


def attach(face, base, name, base_ch):
    """Place one mark on `base` following the face's gap and centring rules."""
    grid = GRIDS[face]
    m = mark(face, name)
    if name == "caron" and base_ch in "dltL":
        tick = mark(face, "apostrophe")
        return base.union(tick, base.width + 1, base.top - tick.top)
    if name == "acute" and "\u0370" <= base_ch <= "\u03ff" and base_ch.isupper():
        return base.union(m, -m.width - 1, base.top - m.top).trimmed()
    if name in BELOW:
        dy = min(0, base.bottom) - m.top
        dx = base.width - m.width if name == "ogonek" else (base.width - m.width) // 2
        return base.union(m, dx, dy)
    gap = grid.cap_gap if base.top >= grid.cap else grid.low_gap
    if name == "ring":
        gap = max(0, gap - 1)
    dx = (base.width - m.width + (name in ("acute", "dacute"))) // 2
    return base.union(m, dx, base.top + gap - m.bottom)


def glyph(face, ch):
    """Return the bitmap for one character, composing marks from our own bases."""
    own = drawing(face, ch)
    if own is not None:
        return own
    grid = GRIDS[face]
    if ch in SPACING_MARKS:
        out = None
        for name in SPACING_MARKS[ch]:
            m = mark(face, name)
            if out is None:
                out = Bitmap(m.cells, -m.top if name in BELOW else grid.cap - m.top)
            else:
                out = attach(face, out, name, ch)
        return out
    decomposition = unicodedata.decomposition(ch).split()
    if decomposition and not decomposition[0].startswith("<"):
        base_ch, *marks = (chr(int(part, 16)) for part in decomposition)
        base_ch = {"i": "ı", "j": "ȷ", "і": "ı"}.get(base_ch, base_ch)
        base = glyph(face, base_ch)
        for m in marks:
            name = COMBINING[ord(m)]
            if name == "cedilla" and base_ch in COMMA_CEDILLA:
                name = "comma"
            if base_ch == "g" and name == "cedilla":
                name = "acute"
            base = attach(face, base, name, base_ch)
        return base
    if decomposition and decomposition[0] == "<compat>" and len(decomposition) == 3:
        left, right = (glyph(face, chr(int(part, 16))) for part in decomposition[1:])
        return left.union(right, left.width + 1, 0)
    raise KeyError(f"{face}: no drawing for U+{ord(ch):04X} {ch}")


def boundaries(count, grid):
    """Stroke edges for `count` skeleton cells, merged where strokes overlap."""
    edges = set()
    for i in range(count):
        edges.add(math.floor(i * grid.pitch + 0.5))
        edges.add(math.floor(i * grid.pitch + 0.5) + grid.stroke)
    return sorted(edges)


def record(face, bm):
    """Encode a bitmap as the design's cell widths, heights and rows in font units."""
    grid = GRIDS[face]
    if not bm.cells.any():
        return {"x": 0, "y": 0, "advance": grid.space, "columns": [], "heights": [], "rows": []}
    bm = bm.trimmed()
    rows, cols = bm.cells.shape
    lsb = grid.spacing // 2
    if grid.pitch == grid.stroke:
        widths = [grid.pitch] * cols
        heights = [grid.pitch] * rows
        cells = ["".join("#" if c else "." for c in row) for row in bm.cells]
        width = cols * grid.pitch
        y = bm.bottom * grid.pitch
    else:
        xs = boundaries(cols, grid)
        ys = boundaries(rows, grid)
        starts = [math.floor(i * grid.pitch + 0.5) for i in range(max(rows, cols))]

        def covered(edges, count):
            return [[lo < starts[i] + grid.stroke and starts[i] < hi for i in range(count)]
                    for lo, hi in zip(edges, edges[1:])]
        cx, cy = covered(xs, cols), covered(ys, rows)
        sky = bm.cells[::-1]  # bottom-up skeleton rows
        cells = []
        for band in reversed(cy):
            cells.append("".join(
                "#" if any(sky[r, c] for r in range(rows) if band[r] for c in range(cols) if span[c]) else "."
                for span in cx))
        widths = [b - a for a, b in zip(xs, xs[1:])]
        heights = [b - a for a, b in zip(ys, ys[1:])][::-1]
        width = xs[-1]
        y = math.floor(bm.bottom * grid.pitch + 0.5)
    return {"x": lsb, "y": int(y), "advance": int(width + grid.spacing),
            "columns": [int(w) for w in widths], "heights": [int(h) for h in heights], "rows": cells}


def design(face, codepoints):
    """Build the face's design document for the given scalars."""
    grid = GRIDS[face]
    glyphs = {}
    for cp in sorted(codepoints):
        ch = chr(cp)
        bm = Bitmap([], 0) if ch in "  " else glyph(face, "-" if ch == "­" else ch)
        glyphs[str(cp)] = record(face, bm)
    return {"ascent": grid.ascent, "descent": grid.descent, "x_height": grid.x_height,
            "pixel": grid.kern_pixel, "spacing": grid.spacing, "glyphs": glyphs}


def write(face):
    path = DESIGNS / f"{face}.json"
    codepoints = [int(cp) for cp in json.loads(path.read_text())["glyphs"]]
    doc = design(face, codepoints)
    glyphs = doc.pop("glyphs")
    lines = [f'  "{cp}": {json.dumps(rec)}' for cp, rec in glyphs.items()]
    head = json.dumps(doc, indent=2)[:-2]
    path.write_text(head + ',\n  "glyphs": {\n' + ",\n".join("  " + line for line in lines) + "\n  }\n}\n")
    doc["glyphs"] = glyphs
    return doc


if __name__ == "__main__":
    for face in GRIDS:
        doc = write(face)
        print(face, len(doc["glyphs"]))
