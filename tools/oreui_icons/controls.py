"""Button and tab nine-slices, switch marks, action masks, the glimmer sheet and the loader."""

from pixels import Canvas, from_map, rgb, shade

OUTLINE = rgb("#1c1b1f")
FOCUS = rgb("#ffffff")
MASK = rgb("#000000")


def raised(face, highlight, edge, depth, outline=OUTLINE):
    """5x7 raised face for slices (2, 2, 4, 2): bevelled 3x3 face over a two-row side."""
    return from_map(
        [
            "ooooo",
            "ohhho",
            "ohfeo",
            "oeeeo",
            "odddo",
            "odddo",
            "ooooo",
        ],
        {"o": outline, "h": highlight, "f": face, "e": edge, "d": depth},
    )


def pressed(face, highlight, edge, outline=OUTLINE):
    """5x5 lowered face for slices (2, 2, 2, 2)."""
    return from_map(
        ["ooooo", "ohhho", "ohfeo", "oeeeo", "ooooo"],
        {"o": outline, "h": highlight, "f": face, "e": edge},
    )


def focused(inner):
    """Wraps a face in the dark outline and a white focus ring (slices grow by one)."""
    out = Canvas(inner.width + 2, inner.height + 2, FOCUS)
    out.rect(1, 1, inner.width, inner.height, OUTLINE)
    out.paste(inner, 1, 1)
    return out


def role(face, depth, hover_lift, outline=OUTLINE):
    """Default, hovered, focused and pressed faces for one button role."""
    hover = shade(face, hover_lift)
    default = raised(face, shade(face, 0.22), shade(face, 0.1), depth, outline)
    hovered = raised(hover, shade(hover, 0.25), shade(hover, 0.12), depth, outline)
    down = shade(face, -0.18)
    return [
        default,
        hovered,
        focused(raised(face, shade(face, 0.22), shade(face, 0.1), depth)),
        pressed(down, shade(down, 0.14), shade(down, 0.06)),
    ]


PRIMARY = role(rgb("#438a34"), rgb("#1f4a17"), 0.2)
SECONDARY = role(rgb("#d4d5d9"), rgb("#5c5c62"), 0.06, rgb("#050506"))
NEUTRAL = role(rgb("#4c4d50"), rgb("#2f3033"), 0.3)
DESTRUCTIVE = role(rgb("#c8383a"), rgb("#a01f22"), 0.22)

DISABLED = from_map(
    ["ooo", "ofo", "odo", "odo", "ooo"],
    {"o": rgb("#56565b"), "f": rgb("#aeafb4"), "d": rgb("#8a8b90")},
)


def tab_faces():
    """Play tab faces: default, hovered, pressed, focused and pressed-focused."""
    face = rgb("#4c4d50")
    hover = rgb("#5c5d61")
    down = rgb("#38393c")
    default = raised(face, shade(face, 0.22), shade(face, 0.1), rgb("#2f3033"))
    lowered = pressed(down, shade(down, 0.14), shade(down, 0.06))
    return [
        default,
        raised(hover, shade(hover, 0.2), shade(hover, 0.1), rgb("#2f3033")),
        lowered,
        focused(default),
        focused(lowered),
    ]


TABS = tab_faces()

# Switch marks are alpha masks tinted by role.
SWITCH_ON = Canvas(8, 42, rgb("#ffffff"))
for _x, _y in [(0, 0), (7, 0), (0, 41), (7, 41)]:
    SWITCH_ON.set(_x, _y, (0, 0, 0, 0))
SWITCH_OFF = Canvas(12, 12)
SWITCH_OFF.ring(6, 6, 5.6, 3.9, rgb("#242425"))

CHEVRON_LEFT = from_map(["...#", "..##", ".##.", "##..", ".##.", "..##", "...#"], {"#": MASK})
CHEVRON_UP = from_map(["...#...", "..###..", ".##.##.", "##...##"], {"#": MASK})
CHEVRON_DOWN = from_map(["##...##", ".##.##.", "..###..", "...#..."], {"#": MASK})


def plus():
    """A plus inside a thin ring."""
    canvas = Canvas(24, 24)
    canvas.ring(12, 12, 11.2, 9.6, MASK)
    canvas.rect(11, 7, 2, 10, MASK)
    canvas.rect(7, 11, 10, 2, MASK)
    return canvas


PLUS = plus()

ARROW_BACK = from_map(
    [
        "....#.......",
        "...##.......",
        "..###.......",
        ".#########..",
        "##########..",
        ".#########..",
        "..###.......",
        "...##.......",
        "....#.......",
    ],
    {"#": MASK},
)
CROSS = from_map(
    ["##...##", "###.###", ".#####.", "..###..", ".#####.", "###.###", "##...##"],
    {"#": MASK},
)


def search():
    canvas = Canvas(12, 12)
    canvas.ring(5, 5, 4.6, 3.1, MASK)
    for step in range(3):
        canvas.rect(8 + step, 8 + step, 2, 2, MASK)
    return canvas


SEARCH = search()
PLAYER = from_map(
    [
        "....####....",
        "...######...",
        "...######...",
        "...######...",
        "....####....",
        "............",
        "..########..",
        ".##########.",
        ".##########.",
        ".##########.",
        ".##########.",
        "............",
    ],
    {"#": MASK},
)
EDIT = from_map(
    [
        "........##..",
        ".......####.",
        "......####..",
        ".....####...",
        "....####....",
        "...####.....",
        "..####......",
        "..###.......",
        "..##........",
        "............",
        "############",
        "............",
    ],
    {"#": MASK},
)
CHECK = from_map(
    ["......##", ".....##.", "##..##..", ".####...", "..##....", "........"],
    {"#": MASK},
)
FILTER = from_map(
    [
        "............",
        "............",
        "############",
        "############",
        "............",
        "..########..",
        "..########..",
        "............",
        "....####....",
        "....####....",
        "............",
        "............",
    ],
    {"#": MASK},
)
EXTERNAL = from_map(
    [
        "......######",
        ".......#####",
        "........####",
        ".......#####",
        "#####.###..#",
        "#....###....",
        "#...###.....",
        "#....#......",
        "#..........#",
        "#..........#",
        "#..........#",
        "############",
    ],
    {"#": MASK},
)
RESET = from_map(
    [
        "............",
        "#...####....",
        "#.##....##..",
        "##........#.",
        "#####.....#.",
        "...........#",
        "...........#",
        "..........#.",
        "..........#.",
        "...##...##..",
        ".....###....",
        "............",
    ],
    {"#": MASK},
)

ACTIONS = [ARROW_BACK, CROSS, SEARCH, PLAYER, EDIT, CHECK, FILTER]


def glimmer_sheet():
    """Nine 24 px frames: a soft diagonal shine crosses the tile, then a white rim settles.

    Screens hold the last frame while a category stays selected.
    """
    sheet = Canvas(216, 24)
    for frame in range(9):
        art = Canvas(12, 12)
        centre = frame * 3 - 1
        for y in range(12):
            for x in range(12):
                distance = abs(x + y - centre)
                if frame < 8 and distance == 0:
                    art.set(x, y, (255, 255, 255, 200))
                elif frame < 8 and distance == 1:
                    art.set(x, y, (255, 255, 255, 90))
        if frame >= 7:
            art.frame(0, 0, 12, 12, (255, 255, 255, 120 if frame == 7 else 235))
        sheet.paste(art.scaled(2), frame * 24, 0)
    return sheet


GLIMMER = glimmer_sheet()

_QUADRANTS = [(0, 0), (4, 0), (4, 4), (0, 4)]


def loading_frames():
    """Ten 100 ms frames: four blocks stack clockwise, hold, then clear in the same order."""
    counts = [(0, 1), (0, 2), (0, 3), (0, 4), (0, 4), (1, 4), (2, 4), (3, 4), (4, 4), (4, 4)]
    frames = []
    for first, last in counts:
        art = Canvas(7, 7)
        for index in range(first, last):
            x, y = _QUADRANTS[index]
            art.rect(x, y, 3, 3, rgb("#ffffff"))
        if first == last:
            art.rect(3, 3, 1, 1, rgb("#ffffff"))
        frames.append(art.scaled(4))
    return frames


LOADING = loading_frames()
