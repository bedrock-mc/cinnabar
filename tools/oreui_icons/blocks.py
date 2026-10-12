"""Isometric block art for the base pack card, the travel block and the missing-pack card."""

from pixels import Canvas, from_map, rgb, shade


class Noise:
    """A small deterministic LCG so generated textures never depend on Python's `random`."""

    def __init__(self, seed):
        self.state = seed & 0xFFFFFFFF

    def next(self, bound):
        self.state = (self.state * 1664525 + 1013904223) & 0xFFFFFFFF
        return (self.state >> 16) % bound


GRASS = [rgb("#5fa83a"), rgb("#6fbb44"), rgb("#4f9330"), rgb("#7cc650")]
SOIL = [rgb("#8a5e3c"), rgb("#9c6c46"), rgb("#74502f"), rgb("#a87a52")]
PEBBLE = rgb("#8e8a86")


def texture(palette, seed, weights=(5, 3, 2, 1)):
    """A 16x16 texture of 2x2 clusters drawn from `palette` by weight."""
    noise = Noise(seed)
    pool = [color for color, weight in zip(palette, weights) for _ in range(weight)]
    tex = Canvas(16, 16)
    for y in range(0, 16, 2):
        for x in range(0, 16, 2):
            tex.rect(x, y, 2, 2, pool[noise.next(len(pool))])
    return tex


def grass_side(seed):
    """Soil with a grass lip whose depth varies per column pair, plus a few pebbles."""
    tex = texture(SOIL, seed)
    noise = Noise(seed * 7 + 1)
    for x in range(0, 16, 2):
        depth = 3 + noise.next(3)
        tex.rect(x, 0, 2, depth, GRASS[noise.next(3)])
        tex.rect(x, depth, 2, 1, GRASS[2])
    for _ in range(3):
        tex.rect(noise.next(14), 7 + noise.next(8), 2, 1, PEBBLE)
    return tex


def iso_cube(width, side, top, left, right, shading=(0.0, -0.32, -0.16)):
    """Draws a 2:1 isometric cube `width` wide with `side`-tall faces from 16x16 textures."""
    h = width // 2
    canvas = Canvas(width, h + side)
    for y in range(canvas.height):
        for x in range(width):
            cx, cy = x + 0.5, y + 0.5
            u = (cx - h) / (2 * h) + cy / h
            v = cy / h - (cx - h) / (2 * h)
            if 0 <= u < 1 and 0 <= v < 1:
                color = shade(top.get(int(u * 16), int(v * 16)), shading[0])
            elif cx < h:
                s = cx / h
                t = (cy - h / 2 - s * h / 2) / side
                if not (0 <= t < 1):
                    continue
                color = shade(left.get(int(s * 16), int(t * 16)), shading[1])
            else:
                s = (cx - h) / h
                t = (cy - h + s * h / 2) / side
                if not (0 <= t < 1):
                    continue
                color = shade(right.get(int(s * 16), int(t * 16)), shading[2])
            canvas.set(x, y, color)
    return canvas


def planks(seed):
    """Horizontal boards with staggered seams."""
    tex = Canvas(16, 16)
    noise = Noise(seed)
    tones = [rgb("#b8874e"), rgb("#a87a44"), rgb("#c4935a")]
    for row in range(4):
        tone = tones[row % 3]
        tex.rect(0, row * 4, 16, 4, tone)
        tex.rect(0, row * 4 + 3, 16, 1, shade(tone, -0.25))
        seam = (row * 7 + noise.next(4)) % 16
        tex.rect(seam, row * 4, 1, 3, shade(tone, -0.25))
    return tex


def cobble(seed):
    """Rounded stones in a dark mortar."""
    tex = Canvas(16, 16, rgb("#5e5c5a"))
    noise = Noise(seed)
    for y in range(0, 16, 4):
        for x in range((y // 4) % 2 * 2, 16, 4):
            tone = [rgb("#9a9894"), rgb("#8a8884"), rgb("#aaa8a2")][noise.next(3)]
            tex.rect(x, y, 3, 3, tone)
            tex.set(x, y, shade(tone, 0.15))
    return tex


def base_pack():
    """The 256 px base-pack card: three textured cubes on a pale sky."""
    art = Canvas(64, 64)
    for y in range(64):
        art.rect(0, y, 64, 1, shade(rgb("#8cc4ec"), 0.45 * (1 - y / 63)))
    for x, y, w in [(4, 7, 10), (7, 5, 5), (48, 11, 12), (51, 9, 6)]:
        art.rect(x, y, w, 2, rgb("#ffffff"))
    edge = rgb("#2c2a26")
    stone = iso_cube(26, 14, cobble(3), cobble(5), cobble(7)).outline(edge)
    wood = iso_cube(26, 14, planks(4), planks(6), planks(8)).outline(edge)
    grass = iso_cube(30, 16, texture(GRASS, 11), grass_side(5), grass_side(9)).outline(edge)
    art.paste(stone, 6, 18)
    art.paste(wood, 32, 18)
    art.paste(grass, 17, 29)
    art.rect(0, 58, 64, 6, rgb("#5f9a3c"))
    art.rect(0, 58, 64, 1, rgb("#7cc650"))
    return art.scaled(4)


def overworld_block():
    """The 112 px travel block: a grass chunk tapering into a ragged earthen underside."""
    art = Canvas(56, 56)
    width, side, left, top = 48, 20, 4, 4
    h = width // 2
    cube = iso_cube(width, side, texture(GRASS, 23), grass_side(31), grass_side(37))
    noise = Noise(41)
    for x in range(0, width, 2):
        bottom = h + side - abs(x + 1 - h) // 2
        depth = max(0, (h - abs(x + 1 - h)) // 2 + noise.next(4) - 2)
        for y in range(-2, depth):
            tone = SOIL[noise.next(4)] if y < depth - 1 else SOIL[2]
            art.rect(left + x, top + bottom + y, 2, 1, shade(tone, -0.38 if x < h else -0.24))
    art.paste(cube, left, top)
    art.stamp(
        [".r.", "rYr", ".r.", ".g.", ".g."],
        {"r": rgb("#f6f2f8"), "Y": rgb("#f4c43c"), "g": rgb("#3e7c2e")},
        33,
        6,
    )
    return art.outline(rgb("#24221e")).scaled(2)


def missing_pack():
    """A 64 px dark card with a magenta question mark."""
    art = Canvas(32, 32, rgb("#2a2830"))
    art.frame(0, 0, 32, 32, rgb("#141318"), 2)
    mark = from_map(
        [
            "..mmmmmm..",
            ".mmmmmmmm.",
            "mmm....mmm",
            "mm......mm",
            ".......mmm",
            ".....mmmm.",
            "....mmm...",
            "....mm....",
            "..........",
            "....mm....",
            "....mm....",
        ],
        {"m": rgb("#f040e8")},
    )
    art.paste(mark.scaled(2), 6, 5)
    return art.scaled(2)


BASE_PACK = base_pack()
OVERWORLD = overworld_block()
MISSING_PACK = missing_pack()

