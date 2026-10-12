"""Profile, server-status and hardcore glyphs."""

from pixels import Canvas, from_map, rgb

STAT_GREY = rgb("#b1b2b5")
OUTLINE = rgb("#1c1b1f")


def line_art(rows):
    """A 12x12 single-colour line drawing shown at 2x."""
    return from_map(rows, {"#": STAT_GREY}).scaled(2)


# Profile stats: time played, blocks mined, mobs defeated, distance travelled.
HOURGLASS = line_art(
    [
        "............",
        ".##########.",
        "..#......#..",
        "..#......#..",
        "...#....#...",
        "....#..#....",
        "....#..#....",
        "...#....#...",
        "..#..##..#..",
        "..#.####.#..",
        ".##########.",
        "............",
    ]
)
PICKAXE = line_art(
    [
        "............",
        "....####....",
        "..##.##.##..",
        ".#...##...#.",
        "#....##....#",
        ".....##.....",
        ".....##.....",
        ".....##.....",
        ".....##.....",
        ".....##.....",
        ".....##.....",
        "............",
    ]
)
SWORD = line_art(
    [
        ".....##.....",
        "....#..#....",
        "....#..#....",
        "....#..#....",
        "....#..#....",
        "....#..#....",
        "....#..#....",
        ".##########.",
        ".....##.....",
        ".....##.....",
        "....####....",
        "............",
    ]
)
BOOT = line_art(
    [
        "............",
        "...#####....",
        "...#...#....",
        "...#...#....",
        "...#...#....",
        "...#...#....",
        "...#....##..",
        "...#......#.",
        "...#......#.",
        "...########.",
        "............",
        "............",
    ]
)
PROFILE_STATS = [HOURGLASS, PICKAXE, SWORD, BOOT]

# Profile overview rows: friends, followers, gallery, achievements (16 px).
_PEOPLE = {
    "k": OUTLINE,
    "a": rgb("#8aa0c0"),
    "A": rgb("#6a80a4"),
    "o": rgb("#f0a048"),
    "O": rgb("#c87c2c"),
    "s": rgb("#f2d2b0"),
    "h": rgb("#5a3c2c"),
    "g": rgb("#56c070"),
    "w": rgb("#ffffff"),
}
FRIENDS = from_map(
    [
        "................",
        ".......kkkk.....",
        "......khhhhk....",
        "......kssssk....",
        "..kkkkkssssk....",
        ".khhhhkkssk.....",
        ".kssssk.kkkkk...",
        ".kssssk.kAAAAk..",
        "..kssk.kaaaaaak.",
        ".kkkkkkkaaaaaak.",
        "koooooooOkaaaak.",
        "koooooooOkkkkkk.",
        "koooooooOk......",
        "kkkkkkkkkk......",
        "................",
        "................",
    ],
    _PEOPLE,
)
def followers():
    """A bust with a green follow badge at its shoulder."""
    canvas = from_map(
        [
            "................",
            "......kkkk......",
            ".....khhhhk.....",
            ".....kssssk.....",
            ".....kssssk.....",
            "......kssk......",
            "....kkkkkkkk....",
            "...kaaaaaaaak...",
            "..kaaaaaaaaaak..",
            "..kaaaaaaaaaak..",
            "..kaaaaaaaaaak..",
            "..kAAAAAAAAAAk..",
            "..kkkkkkkkkkkk..",
        ],
        _PEOPLE,
    )
    canvas = _grown(canvas, 16, 16)
    canvas.disc(12, 12, 4.2, OUTLINE)
    canvas.disc(12, 12, 3.3, _PEOPLE["g"])
    canvas.rect(11, 10, 2, 4, _PEOPLE["w"])
    canvas.rect(10, 11, 4, 2, _PEOPLE["w"])
    return canvas


def _grown(canvas, width, height):
    out = Canvas(width, height)
    out.paste(canvas)
    return out


FOLLOWERS = followers()

def camera():
    """A compact camera: outlined body, raised viewfinder, ringed lens and a red shutter."""
    canvas = Canvas(16, 16)
    canvas.rect(4, 3, 5, 2, OUTLINE)
    canvas.rect(5, 4, 3, 1, rgb("#5c6068"))
    canvas.rect(0, 5, 16, 10, OUTLINE)
    canvas.rect(1, 6, 14, 8, rgb("#8c94a4"))
    canvas.rect(1, 6, 14, 1, rgb("#b4bac8"))
    canvas.rect(12, 7, 2, 1, rgb("#e84c48"))
    canvas.disc(7, 10, 3.6, OUTLINE)
    canvas.disc(7, 10, 2.6, rgb("#c8ccd4"))
    canvas.disc(7, 10, 1.6, rgb("#3c78c8"))
    canvas.set(6, 9, rgb("#a8d0f8"))
    return canvas


GALLERY = camera()

TROPHY = from_map(
    [
        "................",
        "..kkkkkkkkkkkk..",
        ".kyyyyyyyyyyYYk.",
        "kyykyYyyyyyykyyk",
        "kykkyYyyyyyykkyk",
        "kykkyYyyyyyYkkyk",
        ".kyykyYyyyyYkyk.",
        "..kkkyyyyyyYkk..",
        "....kkyyyyYkk...",
        "......kyyYk.....",
        "......kyyYk.....",
        ".....kkyyYkk....",
        "....kbbbbbbbk...",
        "....kbBbbbbbk...",
        "....kkkkkkkkk...",
        "................",
    ],
    {
        "k": OUTLINE,
        "y": rgb("#f4c43c"),
        "Y": rgb("#c88c1c"),
        "b": rgb("#8a5c34"),
        "B": rgb("#c08c5c"),
    },
)
PROFILE_SUMMARY = [FRIENDS, FOLLOWERS, GALLERY, TROPHY]

def score():
    """A generic score badge: a five-point star in a heavy ring, white for role tinting."""
    canvas = Canvas(20, 20)
    canvas.ring(10, 10, 9.7, 7.6, rgb("#ffffff"))
    canvas.stamp(
        [
            "....#....",
            "....#....",
            "...###...",
            "#########",
            ".#######.",
            "..#####..",
            "..##.##..",
            ".##...##.",
        ],
        {"#": rgb("#ffffff")},
        6,
        5,
    )
    return canvas


SCORE = score()


def hardcore():
    """An outlined heart split by an engraved crack."""
    art = Canvas(10, 10)
    shape = from_map(
        [
            ".mm..mm.",
            "mmmmmmmm",
            "mmmmmmmm",
            "mmmmmmmm",
            ".mmmmmm.",
            "..mmmm..",
            "...mm...",
        ],
        {"m": rgb("#9a9b9f")},
    )
    art.paste(shape, 1, 1)
    art = art.outline(rgb("#5a5a5e"))
    art.stamp(["hh", "h"], {"h": rgb("#d8d9dc")}, 2, 2)
    art.stamp(["...d", "..d", "...d", "..d"], {"d": rgb("#5a5a5e")}, 2, 3)
    return art.scaled(2)


HARDCORE = hardcore()

# Server status: four rising bars, lit by tier.
PING_DIM = rgb("#c4c6cc")
PING_SWEEP_DIM = rgb("#6e7076")
_BARS = [(3, 3), (7, 6), (11, 9), (15, 12)]


def ping(lit, color):
    canvas = Canvas(20, 20)
    for index, (x, height) in enumerate(_BARS):
        canvas.rect(x, 17 - height, 3, height, color if index < lit else PING_DIM)
    return canvas


PINGS = [ping(4, rgb("#58c88c")), ping(3, rgb("#e8be2c")), ping(1, rgb("#f25c64"))]


def ping_strip():
    """Seven 16 px cells: a lit bar sweeps up and back; the last cell repeats the first.

    Screens loop the first six cells, so the sweep wraps without a pause.
    """
    canvas = Canvas(112, 20)
    for cell, lit in enumerate([0, 1, 2, 3, 2, 1, 0]):
        for index, (x, height) in enumerate(_BARS):
            color = rgb("#ffffff") if index == lit else PING_SWEEP_DIM
            canvas.rect(cell * 16 + x - 2, 17 - height, 3, height, color)
    return canvas


PLAYERS = from_map(
    [
        "....###.....",
        "...#####....",
        "...#####....",
        "...#####....",
        "....###.....",
        "............",
        "..#######...",
        ".#########..",
        ".#######..g.",
        ".######..ggg",
        ".######...g.",
        "............",
    ],
    {"#": rgb("#ffffff"), "g": rgb("#5cc84c")},
)
