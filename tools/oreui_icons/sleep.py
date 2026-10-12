"""29x33 bed-screen art: two sleepers and six dream bubbles, two 500 ms frames each.

Dreams occupy the upper rows and sleepers the lower rows, so a dream overlays a sleeper.
"""

from pixels import Canvas, from_map, rgb

W, H = 29, 33
EDGE = rgb("#2a2622")


def sleeper(blanket, hair, breath):
    canvas = Canvas(W, H)
    wood = {"w": rgb("#a8743e"), "W": rgb("#7a5028"), "e": EDGE}
    canvas.stamp(
        [
            "eee",
            "ewe",
            "ewe",
            "ewe",
            "ewe",
            "ewe",
            "ewe",
            "ewe",
            "eWe",
            "eee",
        ],
        wood,
        1,
        22,
    )
    canvas.stamp(
        ["eeee"] + ["ewwe"] * 12 + ["eWWe", "eeee"],
        wood,
        24,
        17,
    )
    canvas.rect(4, 28, 20, 2, wood["w"])
    canvas.rect(4, 30, 20, 1, wood["W"])
    canvas.rect(4, 31, 20, 1, EDGE)
    canvas.rect(5, 32, 2, 1, EDGE)
    canvas.rect(21, 32, 2, 1, EDGE)
    pillow = rgb("#f2f0ea")
    canvas.rect(17, 22, 7, 4, pillow)
    canvas.rect(17, 25, 7, 1, rgb("#cfcbc2"))
    lump = 21 - breath
    canvas.rect(4, 24, 15, 4, blanket)
    canvas.rect(7, lump + 1, 9, 24 - lump - 1, blanket)
    canvas.rect(8, lump, 7, 1, blanket)
    canvas.rect(4, 27, 15, 1, tuple(max(c - 40, 0) for c in blanket[:3]) + (255,))
    canvas.rect(16, 24, 3, 3, rgb("#f2f0ea"))
    head = from_map(
        [
            ".hhhh.",
            "hhhhhh",
            "hssssh",
            "ssssss",
            "skksss",
            ".ssss.",
        ],
        {"h": hair, "s": rgb("#e8b48c"), "k": rgb("#5a3c2c")},
    )
    canvas.paste(head, 18, 18)
    return canvas


def dream(icon, lift):
    canvas = Canvas(W, H)
    bubble = from_map(
        [
            "....ggg..ggg.......",
            "..ggwwwggwwwgg.ggg.",
            ".gwwwwwwwwwwwwgwwwg",
            "gwwwwwwwwwwwwwwwwwg",
            "gwwwwwwwwwwwwwwwwwg",
            ".gwwwwwwwwwwwwwwwwg",
            "gwwwwwwwwwwwwwwwwg.",
            "gwwwwwwwwwwwwwwwwwg",
            "gwwwwwwwwwwwwwwwwwg",
            ".gwwwwwwwwwwwwwwwg.",
            "..gggwwwgggwwwggg..",
            ".....ggg...ggg.....",
        ],
        {"g": rgb("#c4c8d0"), "w": rgb("#ffffff")},
    )
    top = 1 - lift
    canvas.paste(bubble, 1, top)
    canvas.paste(icon, 1 + (19 - icon.width) // 2, top + 2 + (8 - icon.height) // 2)
    dots = [(19, 13, 2), (21, 15, 1)] if lift else [(18, 13, 2), (21, 16, 1)]
    for x, y, size in dots:
        canvas.rect(x, y, size, size, rgb("#ffffff"))
    return canvas


DREAM_ICONS = {
    "apple": from_map(
        ["....g.", "...gg.", ".rr.rr", "rRrrrr", "rRrrrr", "rrrrrr", ".rrrr.", "..rr.."],
        {"g": rgb("#4ca83c"), "r": rgb("#e0443c"), "R": rgb("#f6a098")},
    ),
    "creeper": from_map(
        ["v.......v", "vv.vvv.vv", "vvvvvvvvv", ".vvyvyvv.", "..vvvvv..", "...v.v..."],
        {"v": rgb("#6a4c9c"), "y": rgb("#f6d84c")},
    ),
    "diamond": from_map(
        [".ccccc.", "cCcccdc", "ccccddc", ".ccddc.", "..cdc..", "...c..."],
        {"c": rgb("#5cd8e0"), "C": rgb("#d8fcff"), "d": rgb("#2ca0b4")},
    ),
    "dirthut": from_map(
        ["...rr...", "..rrrr..", ".rrrrrr.", "rrrrrrrr", ".wwwwww.", ".wwddww.", ".wwddww."],
        {"r": rgb("#c4473c"), "w": rgb("#e0c890"), "d": rgb("#7a5028")},
    ),
    "pickaxe": from_map(
        ["..sssss..", ".s..b..s.", "s...b...s", "....b....", "....b....", "....b...."],
        {"s": rgb("#7c8ca0"), "b": rgb("#a8743e")},
    ),
    "poppy": from_map(
        ["..r.r..", ".rrrrr.", "rrryrrr", ".rrrrr.", "..rgr..", "...g.g.", "...g..."],
        {"r": rgb("#e8484c"), "y": rgb("#f6d84c"), "g": rgb("#4ca83c")},
    ),
}

SLEEPERS = {
    "steve": (rgb("#3c78c8"), rgb("#3a2a20")),
    "alex": (rgb("#4caa64"), rgb("#c8a868")),
}


def frames(name):
    """The two animation frames for one sleep key stem."""
    if name in SLEEPERS:
        blanket, hair = SLEEPERS[name]
        return [sleeper(blanket, hair, 0), sleeper(blanket, hair, 1)]
    return [dream(DREAM_ICONS[name], 0), dream(DREAM_ICONS[name], 1)]


ALL = [frame for name in list(SLEEPERS) + list(DREAM_ICONS) for frame in frames(name)]
