#!/usr/bin/env python3
"""Derive Cinnangles Ten, Seven, Five and Five Bold from Cinnangles Sans on its pixel grid.

Sans is rectilinear on a 64-unit texel grid, and its Latin set uses 2-texel design pixels.
Caps faces embolden Sans bitmaps and draw lowercase with capitals. Seven only changes
the em and line metrics. The 1280 em makes cap height 0.7 em. Reference faces supply only vertical metrics and target stroke weight.
Needs Python 3.14 (Unicode 16.0.0), numpy, scipy and fontTools. Regenerate the shipped faces with:
python3 tools/fonts/build_faces.py --sans assets/fonts/CinnanglesSans.ttf \
    --out assets/fonts --manifests assets --reviewed assets/fonts ten seven five five-bold
The source manifests pin the Sans SHA-256.
"""
import argparse
import hashlib
import json
import unicodedata
from pathlib import Path

import numpy as np
from fontTools.pens.recordingPen import RecordingPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont
from scipy.ndimage import label

T = 64  # texel, in Sans font units
DESIGN = 2  # texels per Latin design pixel
Y_BOTTOM = -256  # raster window bottom, font units
ROWS = 24  # raster window covers Y_BOTTOM .. Y_BOTTOM + ROWS * T
CAP = 896  # Sans cap height
UPEM = 1280  # CAP / UPEM == 0.7, the reference cap height
CAP_ROW = ROWS - (CAP - Y_BOTTOM) // T  # raster row of the cap line
BASE_ROW = ROWS + Y_BOTTOM // T  # first raster row below the baseline
EIGHT = np.ones((3, 3), bool)

# Vertical metrics per 1000 em of each reference face; stroke settings reach its weight.
FACES = {
    "ten": dict(
        family="Cinnangles Ten", style="Regular", file="CinnanglesTen.ttf", ps="CinnanglesTen-Regular",
        weight=400, bold=False, latin_bold=True, latin_v=True, texel_h=True, texel_v=True, space=384,
        condense=True,
        hhea=(990, -250), typo=(750, -250), win=(990, 250), reference="Minecraft Ten v2",
    ),
    "five": dict(
        family="Cinnangles Five", style="Regular", file="CinnanglesFive.ttf", ps="CinnanglesFive-Regular",
        weight=400, bold=False, latin_bold=False, latin_v=False, texel_h=False, texel_v=False, space=None,
        hhea=(1120, -280), typo=(750, -250), win=(1120, 280), reference="Minecraft Five v2",
    ),
    "five-bold": dict(
        family="Cinnangles Five", style="Bold", file="CinnanglesFive-Bold.ttf", ps="CinnanglesFive-Bold",
        weight=700, bold=True, latin_bold=True, latin_v=False, texel_h=True, texel_v=False, space=None,
        hhea=(1138, -306), typo=(750, -250), win=(1138, 306), reference="Minecraft Five v2 Bold",
    ),
}

# Sans outlines unchanged; only the em and line metrics move to Minecraft Seven v2's.
SEVEN = dict(family="Cinnangles Seven", style="Regular", file="CinnanglesSeven.ttf",
             ps="CinnanglesSeven-Regular", hhea=(800, -100), typo=(800, -100), win=(800, 100),
             reference="Minecraft Seven v2")

# Unifont-grid capitals in these ranges also get heavier horizontals in Ten.
CASED_RANGES = [(0x0000, 0x052F), (0x1C80, 0x1C8F), (0x1D00, 0x1DBF), (0x1E00, 0x1FFF),
                (0x2C60, 0x2C7F), (0xA640, 0xA69F), (0xA720, 0xA7FF), (0xAB30, 0xAB6F)]
# Box drawing, blocks and shades tile edge to edge and keep their Sans shape.
TILES = (0x2500, 0x259F)
ABOVE, BELOW = {230}, {202, 220}

COPYRIGHT = (
    "Copyright 2026 The Cinnabar Authors. Derived from Cinnangles Sans, which includes glyphs "
    "from GNU Unifont, Copyright Roman Czyborra, Paul Hardy, Qianqian Fang, Andrew Miller, "
    "Johnnie Weaver, David Corbett, Nils Moskopp, Rebecca Bettencourt, et al."
)
LICENSE = "Provenance and licensing review pending; no license is asserted."



def contours(glyphset, name):
    """Return the rectilinear contours of one source glyph."""
    pen = RecordingPen()
    glyphset[name].draw(pen)
    out, cur = [], []
    for op, args in pen.value:
        if op == "moveTo":
            cur = [args[0]]
        elif op == "lineTo":
            cur.append(args[0])
        elif op in ("closePath", "endPath"):
            if cur:
                out.append(cur)
            cur = []
        else:
            raise ValueError(f"{name}: non-rectilinear segment {op}")
    return out


def rasterize(cs, cols):
    """Nonzero-winding texel bitmap of rectilinear contours; row 0 is the window top."""
    xs = (np.arange(cols) + 0.5) * T
    ys = Y_BOTTOM + (ROWS - 1 - np.arange(ROWS) + 0.5) * T
    X, Y = np.meshgrid(xs, ys)
    wind = np.zeros((ROWS, cols), dtype=np.int32)
    for c in cs:
        for k in range(len(c)):
            (ax, ay), (bx, by) = c[k], c[(k + 1) % len(c)]
            if ax != bx or ay == by:
                continue  # only vertical edges cross a horizontal ray
            hit = (Y >= min(ay, by)) & (Y < max(ay, by)) & (ax > X)
            wind += np.where(hit, 1 if by > ay else -1, 0)
    return wind != 0


def vertical_runs(a):
    """Length of the vertical ink run through each pixel (0 on background)."""
    out = np.zeros(a.shape, int)
    for c in range(a.shape[1]):
        r = 0
        while r < a.shape[0]:
            if not a[r, c]:
                r += 1
                continue
            s = r
            while r < a.shape[0] and a[r, c]:
                r += 1
            out[s:r, c] = r - s
    return out


def widen_gaps(a, stems_only=False):
    """Duplicate each column that is a one-pixel gap between strokes, so bolding keeps it open.

    With stems_only, only gaps beside a stem at least three pixels tall count; gaps between
    diagonal steps may close.
    """
    while a.shape[1] >= 3:
        gap = a[:, :-2] & ~a[:, 1:-1] & a[:, 2:]
        if stems_only:
            v = vertical_runs(a)
            gap &= (v[:, :-2] >= 3) | (v[:, 2:] >= 3)
        cols = np.where(gap.any(0))[0] + 1
        if not len(cols):
            break
        a = np.insert(a, cols, a[:, cols], axis=1)
    return a


def bold_right(a, n):
    """OR the bitmap with itself shifted right by 1..n columns, as Minecraft draws bold."""
    out = np.zeros((a.shape[0], a.shape[1] + n), bool)
    for k in range(n + 1):
        out[:, k:k + a.shape[1]] |= a
    return out


def joins_other(labels, r, c, own):
    """True if texel (r, c) touches a component other than `own`."""
    near = labels[max(r - 1, 0):r + 2, max(c - 1, 0):c + 2]
    return bool(((near != 0) & (near != own)).any())


def safe_right(a):
    """Thicken one texel rightwards without closing a gap or joining another stroke."""
    out = np.zeros((a.shape[0], a.shape[1] + 1), bool)
    out[:, :-1] = a
    labels, _ = label(out, EIGHT)
    grown = out.copy()
    for r, c in zip(*np.nonzero(a)):
        t = c + 1
        if out[r, t] or (t + 1 < out.shape[1] and out[r, t + 1]):
            continue
        if not joins_other(labels, r, t, labels[r, c]):
            grown[r, t] = True
    return grown


def thicken_bars(a, max_run):
    """Thicken horizontal strokes one texel inside the glyph's own vertical extent.

    Vertical runs of at most `max_run` texels grow downward, or upward when resting on the
    lowest ink row. Stems, cap height and baseline stay put; gaps of one texel stay open.
    """
    rows = np.where(a.any(1))[0]
    if not len(rows):
        return a
    top, bottom = rows[0], rows[-1]
    labels, _ = label(a, EIGHT)
    out = a.copy()
    for c in range(a.shape[1]):
        col = a[:, c]
        r = top
        while r <= bottom:
            if not col[r]:
                r += 1
                continue
            s = r
            while r <= bottom and col[r]:
                r += 1
            e = r - 1
            if e - s + 1 > max_run:
                continue
            if e < bottom:
                t, beyond = e + 1, e + 2
            else:
                t, beyond = s - 1, s - 2
            if t < top or t > bottom or col[t] or (top <= beyond <= bottom and col[beyond]):
                continue
            if not joins_other(labels, t, c, labels[s, c]):
                out[t, c] = True
    return out


def runs_at(row, j):
    """Length of the run of equal values through column j of a 1-D bool row."""
    lo = hi = j
    while lo > 0 and row[lo - 1] == row[j]:
        lo -= 1
    while hi < len(row) - 1 and row[hi + 1] == row[j]:
        hi += 1
    return hi - lo + 1


def drop_column(d):
    """Remove the interior column nearest the centre that keeps stems >= 2 and gaps >= 1 wide.

    Bolding adds width; taking columns back from inside the letter keeps headings near the
    reference's set width. Pixels on a stem (a vertical run of four or more) keep two pixels
    of horizontal ink; diagonal steps may thin to one.
    """
    cols = np.where(d.any(0))[0]
    if len(cols) < 3:
        return d
    c0, c1 = cols[0], cols[-1]
    v = vertical_runs(d)
    mid = (c0 + c1) / 2
    for j in sorted(range(c0 + 1, c1), key=lambda j: (abs(j - mid), j)):
        ok = True
        for r, row in enumerate(d):
            run = runs_at(row, j)
            need = (3 if v[r, j] >= 4 * DESIGN else 2) if row[j] else 2
            if row[c0:c1 + 1].any() and run < need:
                ok = False
                break
        if ok:
            return np.delete(d, j, axis=1)
    return d


def transform(a, kind, cfg, widen=True):
    """Embolden one bitmap for the face; returns (bitmap, added advance in texels)."""
    if kind == "latin":
        if cfg["latin_bold"]:
            d = a[:, ::DESIGN]  # Latin glyphs are exact 2x horizontal upsamples
            w = d.shape[1]
            condense = cfg.get("condense")
            d = bold_right(widen_gaps(d) if widen else d, 1)
            if condense:
                while d.shape[1] > w and (narrower := drop_column(d)) is not d:
                    d = narrower
            a, added = np.repeat(d, DESIGN, axis=1), (d.shape[1] - w) * DESIGN
        else:
            a, added = bold_right(a, 1), 1  # half a design pixel
        if cfg["latin_v"]:
            a = thicken_bars(a, DESIGN)
        return a, added
    if kind == "texel":
        added = 0
        if cfg["texel_h"]:
            a, added = safe_right(a), 1
        if cfg["texel_v"] == "cased":
            a = thicken_bars(a, 1)
        return a, added
    return a, 0


def ink_cols(a):
    """Return the horizontal centre of the bitmap ink, or None."""
    cols = np.where(a.any(0))[0]
    return (cols[0] + cols[-1] + 1) / 2 if len(cols) else None


def split_mark(a, above):
    """Split a precomposed bitmap into (mark, base) at the blank row nearest its mark, or None."""
    rows = np.where(a.any(1))[0]
    if len(rows) < 2:
        return None
    span = range(rows[0], rows[-1] + 1) if above else range(rows[-1], rows[0] - 1, -1)
    for r in span:
        if not a[r].any():
            mark, base = a.copy(), a.copy()
            if above:
                mark[r:] = False
                base[:r] = False
            else:
                mark[:r] = False
                base[r:] = False
            return (mark, base) if mark.any() and base.any() else None
    return None


# Original strokes for capitals with no decomposition: base letter, right shift and added
# design pixels (row from the cap line, column), drawn on the full-height Latin base.
OVERLAYS = {
    0x0141: ("L", 0, [(3, 1), (2, 2)]),  # Ł
    0x0110: ("D", 1, [(3, 0), (3, 2)]),  # Đ
    0x00D0: ("D", 1, [(3, 0), (3, 2)]),  # Ð
}


# Capitals that share a Latin capital's shape draw with that Latin glyph in caps faces.
HOMOGLYPHS = {
    0x0410: "A", 0x0412: "B", 0x0415: "E", 0x041A: "K", 0x041C: "M", 0x041D: "H", 0x041E: "O",
    0x0420: "P", 0x0421: "C", 0x0422: "T", 0x0425: "X", 0x0405: "S", 0x0406: "I", 0x0408: "J",
    0x0391: "A", 0x0392: "B", 0x0395: "E", 0x0396: "Z", 0x0397: "H", 0x0399: "I", 0x039A: "K",
    0x039C: "M", 0x039D: "N", 0x039F: "O", 0x03A1: "P", 0x03A4: "T", 0x03A5: "Y", 0x03A7: "X",
}

# Original full-height capitals on the Latin 5x7 design grid; an eighth row is the descender.
DRAWN = {
    0x0411: ["#####", "#....", "####.", "#...#", "#...#", "#...#", "####."],  # Б
    0x0413: ["#####", "#....", "#....", "#....", "#....", "#....", "#...."],  # Г
    0x0414: [".####", ".#..#", ".#..#", ".#..#", ".#..#", ".#..#", "#####", "#...#"],  # Д
    0x0416: ["#.#.#", "#.#.#", "#.#.#", ".###.", "#.#.#", "#.#.#", "#.#.#"],  # Ж
    0x0417: [".###.", "#...#", "....#", "..##.", "....#", "#...#", ".###."],  # З
    0x0418: ["#...#", "#...#", "#..##", "#.#.#", "##..#", "#...#", "#...#"],  # И
    0x041B: [".####", ".#..#", ".#..#", ".#..#", ".#..#", ".#..#", "##..#"],  # Л
    0x041F: ["#####", "#...#", "#...#", "#...#", "#...#", "#...#", "#...#"],  # П
    0x0423: ["#...#", "#...#", "#...#", ".####", "....#", "#...#", ".###."],  # У
    0x0424: ["..#..", ".###.", "#.#.#", "#.#.#", "#.#.#", ".###.", "..#.."],  # Ф
    0x0426: ["#..#.", "#..#.", "#..#.", "#..#.", "#..#.", "#..#.", "#####", "....#"],  # Ц
    0x0427: ["#...#", "#...#", "#...#", ".####", "....#", "....#", "....#"],  # Ч
    0x0428: ["#.#.#", "#.#.#", "#.#.#", "#.#.#", "#.#.#", "#.#.#", "#####"],  # Ш
    0x0429: ["#.#.#", "#.#.#", "#.#.#", "#.#.#", "#.#.#", "#.#.#", "#####", "....#"],  # Щ
    0x042A: ["##...", ".#...", ".###.", ".#..#", ".#..#", ".#..#", ".###."],  # Ъ
    0x042B: ["#...#", "#...#", "###.#", "#.#.#", "#.#.#", "#.#.#", "###.#"],  # Ы
    0x042C: ["#....", "#....", "####.", "#...#", "#...#", "#...#", "####."],  # Ь
    0x042D: [".###.", "#...#", "....#", "..###", "....#", "#...#", ".###."],  # Э
    0x042E: ["#..#.", "#.#.#", "#.#.#", "###.#", "#.#.#", "#.#.#", "#..#."],  # Ю
    0x042F: [".####", "#...#", "#...#", ".####", "..#.#", ".#..#", "#...#"],  # Я
    0x0404: [".###.", "#...#", "#....", "###..", "#....", "#...#", ".###."],  # Є
    0x0490: ["....#", "#####", "#....", "#....", "#....", "#....", "#...."],  # Ґ
    0x0402: ["#####", "..#..", "..###", "..#.#", "..#.#", "..#.#", "..#.#", "...#."],  # Ђ
    0x040B: ["#####", "..#..", "..###", "..#.#", "..#.#", "..#.#", "..#.#"],  # Ћ
    0x0409: [".##...", ".#.#..", ".#.###", ".#.#.#", ".#.#.#", ".#.#.#", "#..##."],  # Љ
    0x040A: ["#..#..", "#..#..", "######", "#..#.#", "#..#.#", "#..#.#", "#..##."],  # Њ
    0x040F: ["#...#", "#...#", "#...#", "#...#", "#...#", "#...#", "#####", "..#.."],  # Џ
    0x0394: ["..#..", "..#..", ".#.#.", ".#.#.", "#...#", "#...#", "#####"],  # Δ
    0x039B: ["..#..", "..#..", ".#.#.", ".#.#.", "#...#", "#...#", "#...#"],  # Λ
    0x039E: ["#####", ".....", ".....", ".###.", ".....", ".....", "#####"],  # Ξ
    0x03A0: ["#####", "#...#", "#...#", "#...#", "#...#", "#...#", "#...#"],  # Π
    0x03A8: ["#.#.#", "#.#.#", "#.#.#", ".###.", "..#..", "..#..", "..#.."],  # Ψ
    0x03A9: [".###.", "#...#", "#...#", "#...#", ".#.#.", ".#.#.", "##.##"],  # Ω
}


def drawn(rows):
    """Texel bitmap and advance for a DRAWN capital: one design pixel of spacing after its ink."""
    width = len(rows[0])
    a = np.zeros((ROWS, (width + 1) * DESIGN + 2), bool)
    for r, line in enumerate(rows):
        for c, ch in enumerate(line):
            if ch == "#":
                y = CAP_ROW + r * DESIGN
                a[y:y + DESIGN, c * DESIGN:(c + 1) * DESIGN] = True
    return a, (width + 1) * DESIGN * T


def overlay(cp, src, latin, cmap, hmtx):
    """Source bitmap and advance for an OVERLAYS capital, or None if its base is missing."""
    base_ch, shift, pixels = OVERLAYS[cp]
    base = cmap.get(ord(base_ch))
    if base not in latin:
        return None
    a = np.zeros((ROWS, src[base].shape[1] + shift * DESIGN), bool)
    a[:, shift * DESIGN:] = src[base]
    for r, c in pixels:
        y, x = CAP_ROW + r * DESIGN, c * DESIGN
        a[y:y + DESIGN, x:x + DESIGN] = True
    return a, hmtx[base][0] + shift * DESIGN * T


def upper_target(cp, cmap):
    """Capital codepoint a caps face draws for cp, if the font has it."""
    ch = chr(cp)
    if ch == "ß":
        return 0x1E9E if 0x1E9E in cmap else None
    up = ch.upper()
    if len(up) != 1 or up == ch or ord(up) not in cmap:
        return None
    return ord(up)


def compose(g, cp, src, done, latin, cmap, cfg):
    """Full-height Latin capital with its accent, rebuilt from the emboldened base, or None.

    Sans shortens accented capitals to fit its ascent and draws others at Unifont height; a
    caps face needs every capital at cap height.
    """
    nfd = unicodedata.normalize("NFD", chr(cp))
    marks = [unicodedata.combining(m) for m in nfd[1:]]
    if not marks or not chr(cp).isupper():
        return None
    above = all(m in ABOVE for m in marks)
    if not above and not all(m in BELOW for m in marks):
        return None
    base = cmap.get(ord(nfd[0]))
    if base not in latin or base == g or base not in done:
        return None
    split = split_mark(src[g], above)
    lower = cmap.get(ord(chr(cp).lower())) if len(chr(cp).lower()) == 1 else None
    if split is None and lower in src and (lower in latin) == (g in latin):
        # Minecraft-style capitals cram the mark against the letter; the lowercase has room.
        split = split_mark(src[lower], above)
    if split is None:
        return None
    mark, src_base = split
    delta = ink_cols(mark) - ink_cols(src_base)
    kind = "latin" if g in latin else "texel"
    mark, _ = transform(mark, kind, dict(cfg, latin_v=False, texel_v=False))
    out, adv = done[base]
    out = out.copy()
    rows = np.where(mark.any(1))[0]
    mark = mark[rows[0]:rows[-1] + 1]
    gap = DESIGN if kind == "latin" else 1
    if above:
        top = CAP_ROW - gap - mark.shape[0]
    else:
        top = BASE_ROW
    if top < 0 or top + mark.shape[0] > ROWS:
        return None
    shift = int(round(ink_cols(out) + delta - ink_cols(mark)))
    cols = np.where(mark.any(0))[0]
    if cols[0] + shift < 0 or cols[-1] + shift >= out.shape[1]:
        return None
    region = out[top:top + mark.shape[0]]
    for c in cols:
        region[:, c + shift] |= mark[:, c]
    return out, adv


def outline(bitmap):
    """Rectilinear TrueType contours (clockwise outer) for a texel bitmap."""
    h, w = bitmap.shape
    edges = {}
    for r, c in zip(*np.nonzero(bitmap)):
        x, y = int(c), int(h - r)  # pixel spans [x, x+1] x [y-1, y], y up
        if c == 0 or not bitmap[r, c - 1]:
            edges.setdefault((x, y - 1), []).append((x, y))
        if r == 0 or not bitmap[r - 1, c]:
            edges.setdefault((x, y), []).append((x + 1, y))
        if c == w - 1 or not bitmap[r, c + 1]:
            edges.setdefault((x + 1, y), []).append((x + 1, y - 1))
        if r == h - 1 or not bitmap[r + 1, c]:
            edges.setdefault((x + 1, y - 1), []).append((x, y - 1))
    loops = []
    while edges:
        start = next(iter(edges))
        loop, prev, cur = [start], None, start
        while True:
            outs = edges[cur]
            nxt = outs[0]
            if len(outs) > 1 and prev is not None:
                # At a diagonal touch, turn right so touching pixels stay separate contours.
                dx, dy = cur[0] - prev[0], cur[1] - prev[1]
                right = (cur[0] + dy, cur[1] - dx)
                nxt = right if right in outs else outs[0]
            outs.remove(nxt)
            if not outs:
                del edges[cur]
            prev, cur = cur, nxt
            if cur == start:
                break
            loop.append(cur)
        n = len(loop)
        pts = [loop[i] for i in range(n)
               if not (loop[i - 1][0] == loop[i][0] == loop[(i + 1) % n][0]
                       or loop[i - 1][1] == loop[i][1] == loop[(i + 1) % n][1])]
        loops.append([(x * T, Y_BOTTOM + y * T) for x, y in pts])
    return loops


def build(face, sans_path, out_dir):
    """Write a caps face derived from the pinned Sans pixel grid."""
    cfg = dict(FACES[face])
    if cfg["texel_v"]:
        cfg["texel_v"] = "cased"
    f = TTFont(sans_path)
    gs = f.getGlyphSet()
    glyf, hmtx = f["glyf"], f["hmtx"]
    cmap = f.getBestCmap()
    rev = {}
    for cp, g in cmap.items():
        rev.setdefault(g, []).append(cp)
    order = f.getGlyphOrder()

    new_cmap = dict(cmap)
    for cp in cmap:
        t = upper_target(cp, cmap)
        if t is not None:
            new_cmap[cp] = cmap[t]
    used = {order[0]} | set(new_cmap.values())

    # Accented capitals may borrow the mark of their lowercase, which caps faces drop.
    lowers = {cmap.get(ord(chr(cp).lower())) for cp in cmap
              if chr(cp).isupper() and len(chr(cp).lower()) == 1
              and len(unicodedata.normalize("NFD", chr(cp))) > 1}
    src, latin = {}, set()
    for g in order:
        gl = glyf[g]
        if (g in used or g in lowers) and gl.numberOfContours > 0:
            if all(x % (T * DESIGN) == 0 and y % (T * DESIGN) == 0 for x, y in gl.coordinates):
                latin.add(g)
            src[g] = rasterize(contours(gs, g), hmtx[g][0] // T + 2)
    # Latin-grid bases referenced by accented capitals must exist even when unmapped.
    def kind_of(g):
        """Choose the stroke policy for one source glyph."""
        cps = rev.get(g, ())
        if cps and all(TILES[0] <= cp <= TILES[1] for cp in cps):
            return "keep"
        return "latin" if g in latin else "texel"

    advance = {g: hmtx[g][0] for g in src}
    for cp, rows in DRAWN.items():
        g = cmap.get(cp)
        if g in src:
            src[g], advance[g] = drawn(rows)
            latin.add(g)
    for cp, ch in HOMOGLYPHS.items():
        g, twin = cmap.get(cp), cmap.get(ord(ch))
        if g in src and twin in latin:
            src[g], advance[g] = src[twin].copy(), hmtx[twin][0]
            latin.add(g)
    for cp in OVERLAYS:
        g = cmap.get(cp)
        made = overlay(cp, src, latin, cmap, hmtx) if g in src else None
        if made is not None and g not in latin:
            src[g], advance[g] = made
            latin.add(g)

    done = {}
    for g in src:
        if g not in used:
            continue
        kind = kind_of(g)
        tcfg = cfg
        if kind == "texel" and cfg["texel_v"] and not all(
                any(lo <= cp <= hi for lo, hi in CASED_RANGES) for cp in rev.get(g, ()) or [-1]):
            tcfg = dict(cfg, texel_v=False)
        bitmap, added = transform(src[g], kind, tcfg)
        done[g] = bitmap, advance[g] + added * T
    composed = 0
    for g in list(src):
        for cp in rev.get(g, ()):
            c = compose(g, cp, src, done, latin, cmap, cfg)
            if c is not None:
                done[g] = c
                composed += 1
                break

    glyphs, metrics = {}, {}
    for g in order:
        if g not in used:
            continue
        adv = hmtx[g][0]
        if g not in done:
            if g == cmap.get(0x20) and cfg["space"]:
                adv = cfg["space"]
            glyphs[g] = TTGlyphPen(None).glyph()
            metrics[g] = (adv, 0)
            continue
        bitmap, adv = done[g]
        pen = TTGlyphPen(None)
        for loop in outline(bitmap):
            pen.moveTo(loop[0])
            for p in loop[1:]:
                pen.lineTo(p)
            pen.closePath()
        glyph = pen.glyph()
        glyph.recalcBounds(None)
        glyphs[g] = glyph
        metrics[g] = (adv, getattr(glyph, "xMin", 0))

    new_order = [g for g in order if g in glyphs]
    f.setGlyphOrder(new_order)
    f["glyf"].glyphs = glyphs
    f["glyf"].glyphOrder = new_order
    f["hmtx"].metrics = metrics
    f["maxp"].numGlyphs = len(new_order)
    for table in f["cmap"].tables:
        if table.isUnicode():
            table.cmap = {cp: g for cp, g in new_cmap.items() if cp <= 0xFFFF or table.format >= 12}
    f["post"].formatType = 3.0  # no glyph names
    f["post"].mapping, f["post"].extraNames = {}, []

    scale = UPEM / 1000
    f["head"].unitsPerEm = UPEM
    f["head"].macStyle = 1 if cfg["bold"] else 0
    hhea = f["hhea"]
    hhea.ascent, hhea.descent = (round(v * scale) for v in cfg["hhea"])
    hhea.lineGap = 0
    os2 = f["OS/2"]
    os2.sTypoAscender, os2.sTypoDescender = (round(v * scale) for v in cfg["typo"])
    os2.sTypoLineGap = 0
    os2.usWinAscent, os2.usWinDescent = (round(v * scale) for v in cfg["win"])
    os2.sCapHeight = CAP
    os2.sxHeight = CAP  # capitals stand in for lowercase
    os2.usWeightClass = cfg["weight"]
    os2.fsSelection = 0x20 if cfg["bold"] else 0x40
    os2.fsType = 0
    os2.recalcAvgCharWidth(f)

    set_names(f, cfg)

    out = Path(out_dir) / cfg["file"]
    f.save(out)
    # Windows clips ink outside the win metrics, so they must cover the bounding box.
    f = TTFont(out)
    os2 = f["OS/2"]
    os2.usWinAscent = max(os2.usWinAscent, f["head"].yMax)
    os2.usWinDescent = max(os2.usWinDescent, -f["head"].yMin)
    f.save(out)
    assert TTFont(out).getBestCmap().keys() == new_cmap.keys()
    return out, len(new_cmap), len(new_order), composed


def set_names(f, cfg):
    """Set the derived family names and pending licensing notice."""
    name = f["name"]
    name.names = []
    full = cfg["family"] if cfg["style"] == "Regular" else f"{cfg['family']} {cfg['style']}"
    entries = {0: COPYRIGHT, 1: cfg["family"], 2: cfg["style"], 3: f"1.000;{cfg['ps']}",
               4: full, 5: "Version 1.000", 6: cfg["ps"], 13: LICENSE}
    for nid, text in entries.items():
        name.setName(text, nid, 3, 1, 0x409)
        name.setName(text, nid, 1, 0, 0)


def build_seven(sans_path, out_dir):
    """Sans with a 1280 em: cap height, x-height, line height and Latin advances match Seven."""
    f = TTFont(sans_path)
    scale = UPEM / 1000
    f["head"].unitsPerEm = UPEM
    hhea, os2 = f["hhea"], f["OS/2"]
    hhea.ascent, hhea.descent = (round(v * scale) for v in SEVEN["hhea"])
    hhea.lineGap = 0
    os2.sTypoAscender, os2.sTypoDescender = (round(v * scale) for v in SEVEN["typo"])
    os2.sTypoLineGap = 0
    os2.usWinAscent, os2.usWinDescent = (round(v * scale) for v in SEVEN["win"])
    set_names(f, SEVEN)
    out = Path(out_dir) / SEVEN["file"]
    f.save(out)
    return out


def write_manifest(face, font_path, sans_path, out_path):
    """Record the generated face and its exact Sans input."""
    cfg = SEVEN if face == "seven" else FACES[face]
    data = Path(font_path).read_bytes()
    doc = {
        "schema": 1,
        "family": cfg["family"],
        "style": cfg["style"],
        "font_file": cfg["file"],
        "font_size_bytes": len(data),
        "font_sha256": hashlib.sha256(data).hexdigest(),
        "glyph_source": {
            "family": "Cinnangles Sans",
            "font_file": Path(sans_path).name,
            "font_sha256": hashlib.sha256(Path(sans_path).read_bytes()).hexdigest(),
        },
        "metrics_reference": {
            "family": cfg["reference"],
            "use": ("em size and vertical metrics only; outlines unchanged from Sans" if face == "seven"
                    else "vertical metrics, cap height and stroke weight only; no outlines"),
        },
    }
    Path(out_path).write_text(json.dumps(doc, indent=2) + "\n")


def preserve_reviewed_metadata(font_path, reviewed):
    """Keep reviewed names and timestamps without changing any generated glyph or metric table."""
    generated = TTFont(font_path, recalcTimestamp=False)
    generated["name"] = reviewed["name"]
    for field in ("created", "modified"):
        setattr(generated["head"], field, getattr(reviewed["head"], field))
    generated.save(font_path)
    checked = TTFont(font_path, recalcTimestamp=False)
    tags = (set(checked.keys()) | set(reviewed.keys())) - {"GlyphOrder"}
    differences = [tag for tag in sorted(tags) if tag not in checked or tag not in reviewed
                   or checked.getTableData(tag) != reviewed.getTableData(tag)]
    if differences:
        raise ValueError(f"regenerated tables differ from reviewed face: {differences}")


def main():
    """Generate the requested faces and optional source manifests."""
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--sans", required=True)
    ap.add_argument("--out", required=True, help="directory for the .ttf files")
    ap.add_argument("--manifests", help="directory for source manifests")
    ap.add_argument("--reviewed", help="preserve and verify metadata from existing reviewed faces")
    ap.add_argument("faces", nargs="*", default=[*FACES, "seven"])
    args = ap.parse_args()
    if unicodedata.unidata_version != "16.0.0":
        ap.error("regeneration requires Unicode 16.0.0 (Python 3.14) for the pinned caps mappings")
    for face in args.faces:
        cfg = SEVEN if face == "seven" else FACES[face]
        reviewed = TTFont(Path(args.reviewed) / cfg["file"], recalcTimestamp=False) if args.reviewed else None
        if face == "seven":
            path = build_seven(args.sans, args.out)
            print(f"seven: {path} bytes={path.stat().st_size}")
        else:
            path, cps, glyphs, composed = build(face, args.sans, args.out)
            print(f"{face}: {path} codepoints={cps} glyphs={glyphs} composed={composed} "
                  f"bytes={path.stat().st_size}")
        if reviewed is not None:
            preserve_reviewed_metadata(path, reviewed)
            print(f"{face}: every table matches the reviewed face")
        if args.manifests:
            stem = (SEVEN if face == "seven" else FACES[face])["file"].removesuffix(".ttf").removeprefix("Cinnangles").lower()
            write_manifest(face, path, args.sans, Path(args.manifests) / f"cinnangles-{stem}-source.json")


if __name__ == "__main__":
    main()
