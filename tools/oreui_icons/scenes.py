"""Empty-inbox and profile-error illustrations: one small floating island and one prop each."""

from blocks import GRASS, SOIL, Noise
from pixels import Canvas, from_map, rgb, shade

EDGE = rgb("#24221e")
WOOD = {"w": rgb("#b8874e"), "W": rgb("#8a6034"), "d": rgb("#5c3c20")}
STONE = {"s": rgb("#a4a2a0"), "S": rgb("#7a7876"), "k": rgb("#4a4846")}
WHITE = rgb("#f6f4ee")


def island(width, depth, seed):
    """A floating island: a grassy lip over soil that tapers to a ragged point, outlined."""
    land = Canvas(width, depth + 5)
    noise = Noise(seed)
    land.rect(0, 0, width, 1, GRASS[3])
    land.rect(0, 1, width, 2, GRASS[0])
    for x in range(0, width, 2):
        land.rect(x, 2, 2, 1 + noise.next(2), GRASS[2])
    for x in range(width):
        middle = abs(x + 0.5 - width / 2) / (width / 2)
        bottom = round(depth * (1 - middle**1.6)) + noise.next(2)
        for y in range(3, 3 + bottom):
            if land.get(x, y)[3]:
                continue
            tone = SOIL[noise.next(4)] if (x // 2 + y // 2) % 3 else SOIL[2]
            land.set(x, y, shade(tone, -0.12 * (y - 3) / max(depth, 1)))
    framed = Canvas(width + 2, depth + 7)
    framed.paste(land, 1, 1)
    return framed.outline(EDGE)


def cloud(width):
    """A flat pixel cloud with a pale underside."""
    canvas = Canvas(width, 5)
    canvas.rect(2, 0, width // 2 - 1, 1, WHITE)
    canvas.rect(1, 1, width - 3, 1, WHITE)
    canvas.rect(0, 2, width, 2, WHITE)
    canvas.rect(1, 4, width - 2, 1, rgb("#d4dce8"))
    return canvas


def bird(canvas, x, y):
    canvas.stamp(["k...k", ".k.k.", "..k.."], {"k": rgb("#c8ccd4")}, x, y)


def sparkle(canvas, x, y, color=rgb("#fff2a0")):
    canvas.stamp([".y.", "yYy", ".y."], {"y": color, "Y": rgb("#ffffff")}, x, y)


def scene(prop, prop_dx, prop_dy, seed, decorate=None, width=60):
    """Centres an island with its prop on a 128x48 canvas."""
    canvas = Canvas(128, 48)
    land = island(width, 12, seed)
    left = (128 - land.width) // 2
    canvas.paste(land, left, 30)
    flora(canvas, left + 3, left + land.width - 4, 31, seed)
    canvas.paste(prop, left + prop_dx, 31 - prop.height + prop_dy)
    if decorate:
        decorate(canvas)
    return canvas


def flora(canvas, start, end, ground, seed):
    """Scatters grass tufts and the odd flower along an island's lip."""
    noise = Noise(seed * 13 + 5)
    x = start + noise.next(4)
    while x < end:
        if noise.next(3):
            canvas.stamp(["g.g", ".g."], {"g": GRASS[2]}, x, ground - 2)
        else:
            petal = [rgb("#f6f2f8"), rgb("#f0d038"), rgb("#e85060")][noise.next(3)]
            canvas.stamp(["p", "g"], {"p": petal, "g": GRASS[2]}, x + 1, ground - 2)
        x += 5 + noise.next(7)


def clouds_and_birds(canvas):
    canvas.paste(cloud(14), 8, 8)
    canvas.paste(cloud(10), 104, 14)
    bird(canvas, 24, 20)
    bird(canvas, 96, 6)


NOTICE_BOARD = from_map(
    [
        ".eeeeeeeeeeeeeeeeeee.",
        "eWWWWWWWWWWWWWWWWWWWe",
        "eWwwwwwwwwwwwwwwwwwWe",
        "eWwwwwrwwwwwwwwwwwwWe",
        "eWwwwpppppwwwwwwwwwWe",
        "eWwwwpppppwwwppppwwWe",
        "eWwwwpppppwwwppppwwWe",
        "eWwwwpppppwwwppppwwWe",
        "eWwwwPPPPPwwwPPPPwwWe",
        "eWwwwwwwwwwwwwwwwwwWe",
        "eWWWWWWWWWWWWWWWWWWWe",
        ".eedeeeeeeeeeeeeedee.",
        "..ede...........ede..",
        "..ede...........ede..",
        "..ede...........ede..",
        "..ede...........ede..",
    ],
    WOOD | {"e": EDGE, "p": WHITE, "P": rgb("#c8c4bc"), "r": rgb("#e04434")},
)

TOWER = from_map(
    [
        ".......f.......",
        ".......fRR.....",
        ".......fRRR....",
        ".......f.......",
        ".k.k.k.f.k.k.k.",
        "kskskskskskskSk",
        "ksssssssssssSSk",
        "ksSsssssssssSSk",
        "kssssskkksssSSk",
        "ksssskdddkssSSk",
        "kssssskkksssSSk",
        "ksssssssssSsSSk",
        "ksssssssssssSSk",
        "kssssskkkssssSk",
        "ksSsskWWWkssSSk",
        "kssskWWWWWksSSk",
        "kssskWWdWWksSSk",
        "kssskWWWWWksSSk",
        "kkkkkkkkkkkkkkk",
    ],
    STONE | WOOD | {"f": rgb("#d8d4cc"), "R": rgb("#e04848")},
)

MAILBOX = from_map(
    [
        "...eeeeeeeeee..",
        "..eBBBBBBBBBBe.",
        ".eBbbbbbbbbbBBe",
        ".eBbbbbbbbbbbBe",
        ".eBbbbbbbbbbbBe",
        ".eeeeeeeeeeeeee",
        ".eBbbbbbbbbbbBe",
        ".eBBBBBBBBBBBBe",
        ".eeeeeeeeeeeeee",
        "......ede......",
        "...rr.ede......",
        "...rrrrde......",
        "......ede......",
        "......ede......",
    ],
    WOOD | {"e": EDGE, "B": rgb("#3c6cc8"), "b": rgb("#5c8ce0"), "r": rgb("#e04434")},
)

GIFT = from_map(
    [
        "....rr...rr....",
        "...rRRr.rRRr...",
        "....rrRrRrr....",
        "eeeeeeeRRRreeee",
        "eGGGGGRRRRGGGGe",
        "eGgggggRRgggGGe",
        "eeeeeeeRReeeeee",
        ".eGgggRRggggGe.",
        ".eGgggRRggggGe.",
        ".eGgggRRggggGe.",
        ".eGgggRRggggGe.",
        ".eGGGGRRGGGGGe.",
        ".eeeeeeeeeeeee.",
    ],
    {"e": EDGE, "G": rgb("#3c9a6c"), "g": rgb("#56c08c"), "r": rgb("#f6d24c"), "R": rgb("#e0a828")},
)

BUBBLE_SIGN = from_map(
    [
        "..eeeeeeeeeeeeeee..",
        ".epppppppppppppppe.",
        "epppppppppppppppppe",
        "epppppppppppppppppe",
        "epppppppppppppppppe",
        "ePppppppppppppppPPe",
        ".ePPPPPPPPPPPPPPPe.",
        "..eeeeePPeeeeeeee..",
        "......ePe..........",
        "......ee...........",
        "........ede........",
        "........ede........",
        "........ede........",
        "........ede........",
    ],
    WOOD | {"e": EDGE, "p": WHITE, "P": rgb("#c8c4bc")},
)


def stars(canvas):
    for x, y in [(14, 6), (30, 14), (46, 4), (82, 9), (100, 3), (112, 16), (20, 22)]:
        sparkle(canvas, x, y)


TELESCOPE = from_map(
    [
        "..............ee",
        "............eebbe",
        "..........eebbbbe",
        "........eebbbbbe.",
        "......eebbbbbbe..",
        ".....ebbbbbbbe...",
        "....eBBbbbbbe....",
        ".....eBBBbee.....",
        "......eeeLe......",
        "......dL.dL......",
        ".....dL...dL.....",
        "....dL.....dL....",
        "...dL.......dL...",
    ],
    {"e": EDGE, "b": rgb("#d0a040"), "B": rgb("#9c7428"), "d": rgb("#5c3c20"), "L": rgb("#8a6034")},
)

CRACKED = from_map(
    [
        "eeeeeeeeeeeeeeee",
        "esssssssksssssSe",
        "essSssskssssssSe",
        "essssssksssSssSe",
        "esssssskkssssssSe"[:16],
        "essSssssksssssSe",
        "esssssskkksssssSe"[:16],
        "essssskssssSsSSe",
        "esssskksssssssSe",
        "esSsskssssssssSe",
        "essssksssssSssSe",
        "essskssssssssSSe",
        "esSSSkSSSSSSSSSe",
        "eeeeeeeeeeeeeeee",
    ],
    STONE | {"e": EDGE},
)

WARNING = from_map(
    [
        "....ee....",
        "...eyye...",
        "...eyye...",
        "..eyykye..",
        "..eyykye..",
        ".eyyykyye.",
        ".eyyyyyye.",
        "eyyyykyyye",
        "eeeeeeeeee",
        "....de....",
        "....de....",
        "....de....",
        "....de....",
    ],
    {"e": EDGE, "y": rgb("#f6c83c"), "k": rgb("#2c2a26"), "d": rgb("#5c3c20")},
)


def generic_error():
    canvas = Canvas(128, 48)
    land = island(56, 11, 19)
    canvas.paste(land, 36, 30)
    canvas.paste(CRACKED, 50, 18)
    canvas.paste(WARNING, 70, 18)
    canvas.stamp(["S.", ".S"], STONE, 44, 28)
    canvas.stamp(["k"], STONE, 47, 29)
    canvas.paste(cloud(14), 10, 10)
    canvas.paste(cloud(10), 102, 6)
    return canvas


def connection_error():
    canvas = Canvas(128, 48)
    left = island(34, 10, 23)
    right = island(34, 10, 29)
    canvas.paste(left, 18, 28)
    canvas.paste(right, 76, 28)
    rope = rgb("#8a6034")
    plank = rgb("#c4935a")
    for x in range(52, 62):
        drop = (x - 52) // 2
        canvas.set(x, 29 + drop, rope)
        if x % 2 == 0:
            canvas.rect(x, 30 + drop, 2, 1, plank)
    for x in range(68, 77):
        drop = (76 - x) // 2
        canvas.set(x, 29 + drop, rope)
        if x % 2 == 1:
            canvas.rect(x, 30 + drop, 2, 1, plank)
    canvas.stamp(
        ["..y.", ".yy.", "yyyy", ".yy.", ".y..", "y..."],
        {"y": rgb("#f6d84c")},
        63,
        20,
    )
    canvas.paste(cloud(12), 12, 8)
    canvas.paste(cloud(14), 98, 12)
    return canvas


INBOX_EMPTY = [
    scene(NOTICE_BOARD, 20, 0, 3, clouds_and_birds),
    scene(TOWER, 22, 0, 5, clouds_and_birds),
    scene(MAILBOX, 22, 0, 7, clouds_and_birds),
    scene(GIFT, 22, 0, 9, lambda c: [sparkle(c, 40, 12), sparkle(c, 86, 8), sparkle(c, 96, 22)]),
    scene(BUBBLE_SIGN, 20, 0, 13, clouds_and_birds),
]

PROFILE_ERRORS = [
    scene(TELESCOPE, 18, 0, 17, stars).scaled(2),
    generic_error().scaled(2),
    connection_error().scaled(2),
]
