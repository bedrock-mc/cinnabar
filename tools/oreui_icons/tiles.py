"""Framed 24 px category tiles: a 10x10 drawing inside a one-texel frame, shown at 2x."""

from pixels import Canvas, from_map, rgb, shade

FRAME = rgb("#1c1b1f")


def tile(rows, palette, bevel="", body=""):
    """Returns the 24 px tile for a 10x10 character map.

    Cells of each `bevel` key catch light where they border cells outside `body` (default:
    the bevel keys) above or to the left, and shade where they do so below or to the right.
    """
    body = body or bevel
    art = Canvas(12, 12, FRAME)
    art.stamp(rows, palette, 1, 1)
    for y, row in enumerate(rows):
        for x, key in enumerate(row):
            if key not in bevel:
                continue
            same = lambda dx, dy: (
                0 <= y + dy < len(rows) and 0 <= x + dx < len(row) and rows[y + dy][x + dx] in body
            )
            if not same(0, -1) or not same(-1, 0):
                art.set(x + 1, y + 1, shade(palette[key], 0.28))
            elif not same(0, 1) or not same(1, 0):
                art.set(x + 1, y + 1, shade(palette[key], -0.22))
    return art.scaled(2)


SKY = {"s": rgb("#7fb6e6"), "S": rgb("#a9d0f0")}

# World settings categories.
GENERAL = tile(
    [
        "SSSSSSSSSS",
        "SooooooooS",
        "soWWWWWWos",
        "soWllllWos",
        "soWWWWWWos",
        "soWlllWWos",
        "sooooooooS",
        "ssssppssss",
        "ggggppgggg",
        "GGGGGGGGGG",
    ],
    SKY
    | {
        "o": rgb("#5a3a20"),
        "W": rgb("#d2a86c"),
        "l": rgb("#8a5a30"),
        "p": rgb("#7a5230"),
        "g": rgb("#6cae48"),
        "G": rgb("#4a8a34"),
    },
    bevel="W",
    body="Wl",
)

ADVANCED = tile(
    [
        "tttttttttt",
        "tttttttttt",
        "taaaaaaaat",
        "tAaaaaaaAt",
        "ttAAAAAAtt",
        "tttdaadttt",
        "tttdaadttt",
        "ttdaaaadtt",
        "tdddddddtt",
        "tttttttttt",
    ],
    {
        "t": rgb("#3c6f78"),
        "a": rgb("#9aa0a8"),
        "A": rgb("#6c727a"),
        "d": rgb("#50555c"),
    },
    bevel="aA",
)

MULTIPLAYER = tile(
    [
        "yyyyyyyyyy",
        "yhhhyyyyyy",
        "yfffyyHHHy",
        "yfefyyFFFy",
        "ybbbyyFEFy",
        "bbbbbyrrry",
        "bbbbbrrrrr",
        "ybbbyrrrrr",
        "ydydyyrrry",
        "ydydyydydy",
    ],
    {
        "y": rgb("#e4c98c"),
        "h": rgb("#3b2c24"),
        "H": rgb("#c08a3c"),
        "f": rgb("#e8b48c"),
        "F": rgb("#9c6a48"),
        "e": rgb("#3b2c24"),
        "E": rgb("#2a1c14"),
        "b": rgb("#3a78c2"),
        "r": rgb("#c4473c"),
        "d": rgb("#3c3a48"),
    },
)

CHEATS = tile(
    [
        "nnnnnnnnnn",
        "nnnnwwnnnn",
        "nnnnccnnwn",
        "nnnmmmmnnn",
        "nnmPPPPmnn",
        "nnmPpPPmnn",
        "nnmPPPpmnn",
        "nnmPpPPmnn",
        "nnnmmmmnnn",
        "nwnnnnnnnn",
    ],
    {
        "n": rgb("#2c2348"),
        "w": rgb("#fff2a8"),
        "c": rgb("#b08850"),
        "m": rgb("#dcd6f0"),
        "P": rgb("#9a4ad8"),
        "p": rgb("#d79af5"),
    },
    bevel="m",
)

RESOURCE_PACKS = tile(
    [
        "kkkkkkkkkk",
        "kRRRRRkkkk",
        "kRrRrRkkkk",
        "kRRYYYYYkk",
        "kRrYyYyYkk",
        "kkkYYBBBBB",
        "kkkYyBbBbB",
        "kkkkkBBBBB",
        "kkkkkBbBbB",
        "kkkkkBBBBB",
    ],
    {
        "k": rgb("#3a3f4c"),
        "R": rgb("#d0543c"),
        "r": rgb("#ef8c6c"),
        "Y": rgb("#e0b23c"),
        "y": rgb("#f6d878"),
        "B": rgb("#3c8ad0"),
        "b": rgb("#7ab8ec"),
    },
    bevel="RYB",
    body="RrYyBb",
)

BEHAVIOUR_PACKS = tile(
    [
        "vvvvvvvvvv",
        "vvvvLLvvvv",
        "vvLvLLvLvv",
        "vvLLLLLLvv",
        "vLLLvvLLLv",
        "vLLLvvLLLv",
        "vvLLLLLLvv",
        "vvLvLLvLvv",
        "vvvvLLvvvv",
        "vvvvvvvvvv",
    ],
    {"v": rgb("#24506a"), "L": rgb("#e6c2ee")},
    bevel="L",
)

EXPERIMENTAL = tile(
    [
        "dddddddddd",
        "dddqqqqddd",
        "ddddqqdddd",
        "ddddqqdddd",
        "dddqeeqddd",
        "ddqeeeeqdd",
        "dqeeFeeeqd",
        "dqeeeeFeqd",
        "dqqqqqqqqd",
        "dddddddddd",
    ],
    {
        "d": rgb("#7a3a5a"),
        "q": rgb("#d8e4e8"),
        "e": rgb("#7cd04c"),
        "F": rgb("#c8f49c"),
    },
)

WORLD_CATEGORIES = [
    GENERAL,
    ADVANCED,
    MULTIPLAYER,
    CHEATS,
    RESOURCE_PACKS,
    BEHAVIOUR_PACKS,
    EXPERIMENTAL,
]

# Play tabs: Worlds, Realms, Servers.
WORLDS_TAB = tile(
    [
        "nnnnnnnnnn",
        "nnnnnnnmmn",
        "nwnnnnnmnn",
        "nnnnnnnnnn",
        "nnnnrrnnwn",
        "nnnrhhrnnn",
        "nnnhhdhnnn",
        "gggghdhggg",
        "GgGGGGGGgG",
        "ffffffffff",
    ],
    {
        "n": rgb("#24305c"),
        "m": rgb("#f4eec0"),
        "w": rgb("#c8d0f0"),
        "r": rgb("#b04434"),
        "h": rgb("#d8b880"),
        "d": rgb("#704828"),
        "g": rgb("#5ca040"),
        "G": rgb("#3e7c2e"),
        "f": rgb("#7a5634"),
    },
)

REALMS_TAB = tile(
    [
        "SSSSSSSSSS",
        "SSfFSSSSSS",
        "SSfSSSSSSS",
        "SccccSSSSS",
        "SccccSScSc",
        "SckkcSSccc",
        "SccccScccc",
        "SckkccckkC",
        "SccccccccC",
        "CCCCCCCCCC",
    ],
    SKY
    | {
        "f": rgb("#c8c8cc"),
        "F": rgb("#e04848"),
        "c": rgb("#b8b4ac"),
        "C": rgb("#7e7a74"),
        "k": rgb("#4a4650"),
    },
    bevel="c",
    body="ckfF",
)

SERVERS_TAB = tile(
    [
        "zzzzzzzzzz",
        "zrrrrrrrrz",
        "zrlrreLrrz",
        "zrrrrrrrrz",
        "zrrrrrrrrz",
        "zrlrreLrrz",
        "zrrrrrrrrz",
        "zrrrrrrrrz",
        "zrlrreerrz",
        "zrrrrrrrrz",
    ],
    {
        "z": rgb("#1f3440"),
        "r": rgb("#5c6670"),
        "l": rgb("#7cf07c"),
        "L": rgb("#f0c040"),
        "e": rgb("#3a424a"),
    },
    bevel="r",
    body="rlLe",
)

PLAY_TABS = [WORLDS_TAB, REALMS_TAB, SERVERS_TAB]

# Inbox categories: News, Realms, Invites, Marketplace Pass (12 px), Feedback.
NEWS = tile(
    [
        "bbbbbbbbbb",
        "bppppppppb",
        "bpHHHHHHpb",
        "bppppppppb",
        "bpiiplllpb",
        "bpiipppppb",
        "bpiiplllpb",
        "bppppppppb",
        "bpllllllpb",
        "bPPPPPPPPb",
    ],
    {
        "b": rgb("#3a5a7a"),
        "p": rgb("#f0ebe0"),
        "P": rgb("#c4bcac"),
        "H": rgb("#2c2c30"),
        "l": rgb("#9a968e"),
        "i": rgb("#e0884c"),
    },
)

INVITES = tile(
    [
        "aaaaaaaaaa",
        "aaaaaaaaaw",
        "aaaaaaawwa",
        "aaaaawwwwa",
        "aaawwwwWaa",
        "awwwwwWWaa",
        "aaaWWWwwaa",
        "aaaaWawaaa",
        "aaaaaaawaa",
        "aaaaaaaaaa",
    ],
    {"a": rgb("#4c9ad8"), "w": rgb("#f6f6f0"), "W": rgb("#b8c8d4")},
)

FEEDBACK = tile(
    [
        "mmmmmmmmmm",
        "mmwwwwwwmm",
        "mwwwwwwwwm",
        "mwwwwwwwwm",
        "mwdwdwdwwm",
        "mwwwwwwwwm",
        "mmwwwwwwmm",
        "mmmwwmmmmm",
        "mmmwmmmmmm",
        "mmmmmmmmmm",
    ],
    {"m": rgb("#4a8a5c"), "w": rgb("#f4f2ea"), "d": rgb("#3c4a44")},
)


def marketplace_pass():
    """The 12 px pass tile: a gold ticket on a framed indigo card."""
    return from_map(
        [
            "kkkkkkkkkkkk",
            "knnnnnnnnnnk",
            "knnnnnnnnnnk",
            "knyyyyyyyynk",
            "knyYyyyyyynk",
            "knnyyoyyynnk",
            "knnyyoyyynnk",
            "knyyyyyyyynk",
            "knyyyyyyyOnk",
            "knnnnnnnnnnk",
            "knnnnnnnnnnk",
            "kkkkkkkkkkkk",
        ],
        {
            "k": FRAME,
            "n": rgb("#3c3c8c"),
            "y": rgb("#f2c440"),
            "Y": rgb("#fff0a0"),
            "o": rgb("#b07a18"),
            "O": rgb("#c89020"),
        },
    )


INBOX = [NEWS, REALMS_TAB, INVITES, marketplace_pass(), FEEDBACK]

# Settings categories, in sidebar order.
ACCESSIBILITY = tile(
    [
        "tttttttttt",
        "ttttwwtttt",
        "tWttwwttWt",
        "ttWwwwwWtt",
        "ttttwwtttt",
        "ttttwwtttt",
        "tttwwwwttt",
        "tttwttwttt",
        "ttwwttwwtt",
        "tttttttttt",
    ],
    {"t": rgb("#2b8a98"), "w": rgb("#ffffff"), "W": rgb("#d6f2f4")},
)

KEYBOARD_MOUSE = tile(
    [
        "ssssssssss",
        "ssssssssss",
        "dddddddddd",
        "dkdkdWdkdd",
        "ddWdWdWdkd",
        "dkdkdkdkdd",
        "ddkkkkkkdd",
        "dddddddddd",
        "ssssssssss",
        "ssssssssss",
    ],
    {
        "s": rgb("#8a93a8"),
        "d": rgb("#2c3038"),
        "k": rgb("#a8afbc"),
        "W": rgb("#f4d24c"),
    },
)

CONTROLS = tile(
    [
        "pppppppppp",
        "pppppppppp",
        "ppccccccpp",
        "pccccccccp",
        "ccwcccccrc",
        "cwwwccbcgc",
        "ccwcccccyc",
        "cccccccccc",
        "cccppppccc",
        "pccppppccp",
    ],
    {
        "p": rgb("#e8b4c8"),
        "c": rgb("#2c3c74"),
        "w": rgb("#c8c8d0"),
        "r": rgb("#e8544c"),
        "g": rgb("#5cc85c"),
        "b": rgb("#4c8ce8"),
        "y": rgb("#f0c840"),
    },
    bevel="c",
    body="cwrgby",
)

TOUCH = tile(
    [
        "oooooooooo",
        "ooddddddoo",
        "oodssssdoo",
        "oodsrrsdoo",
        "oodrWWrdoo",
        "oodrWWrdoo",
        "oodsrrsdoo",
        "oodssssdoo",
        "ooddkdddoo",
        "oooooooooo",
    ],
    {
        "o": rgb("#e0a050"),
        "d": rgb("#2a2c34"),
        "s": rgb("#5ca8d8"),
        "r": rgb("#a4d4f0"),
        "W": rgb("#ffffff"),
        "k": rgb("#6a6e78"),
    },
    bevel="d",
    body="dsrWk",
)

PARTY = tile(
    [
        "ssssssssss",
        "sRRsssBBss",
        "RrRRsBbBBs",
        "RRRRsBBBBs",
        "sRRssBBBBs",
        "ssRYYsBBss",
        "sslYyYlBss",
        "sslYYYslss",
        "ssslYsslss",
        "sssslllsss",
    ],
    {
        "s": rgb("#3c3454"),
        "R": rgb("#e04c5c"),
        "r": rgb("#f6a0a8"),
        "B": rgb("#44b4e4"),
        "b": rgb("#b0e4f8"),
        "Y": rgb("#f0c838"),
        "y": rgb("#fff0a0"),
        "l": rgb("#c8c0d8"),
    },
)

WORK_BENCH = tile(
    [
        "bbbbbbbbbb",
        "bHHHHHHHbb",
        "bHhhhhhHHb",
        "bHHHHHHHbb",
        "bbbbllbbbb",
        "bbbbllbbbb",
        "bbbbllbbbb",
        "bbbbllbbbb",
        "bbbbLLbbbb",
        "bbbbbbbbbb",
    ],
    {
        "b": rgb("#c87c3c"),
        "H": rgb("#6c727c"),
        "h": rgb("#b4bac4"),
        "l": rgb("#7a4a24"),
        "L": rgb("#4c2c14"),
    },
    bevel="H",
)

PAINTING = tile(
    [
        "gggggggggg",
        "ggppppppgg",
        "gpRppBpppg",
        "pppppppYpp",
        "pGpggppppg",
        "pppggpWppp",
        "gppppppppg",
        "gppppppppg",
        "ggpppppggg",
        "gggggggggg",
    ],
    {
        "g": rgb("#48506c"),
        "p": rgb("#e4c494"),
        "R": rgb("#e04434"),
        "B": rgb("#3c7cd8"),
        "Y": rgb("#f0d038"),
        "G": rgb("#4cae44"),
        "W": rgb("#f8f8f8"),
    },
    bevel="p",
    body="pRBYGW",
)

SOUND = tile(
    [
        "nnnnnnnnnn",
        "nnnnnnnnwn",
        "nnnSnnwnnw",
        "nnSSnnnwnw",
        "SSSSnwnwnw",
        "SSSSnwnwnw",
        "nnSSnnnwnw",
        "nnnSnnwnnw",
        "nnnnnnnnwn",
        "nnnnnnnnnn",
    ],
    {"n": rgb("#2c4c3c"), "S": rgb("#e8e4d8"), "w": rgb("#8ce0a8")},
)

ACCOUNT = tile(
    [
        "uuuuuuuuuu",
        "uccccccccu",
        "ucffccllcu",
        "ucffcccccu",
        "ucbbcclccu",
        "uccccccccu",
        "ucLLLLLLcu",
        "uccccccccu",
        "uuuuuuuuuu",
        "uuuuuuuuuu",
    ],
    {
        "u": rgb("#5a5e66"),
        "c": rgb("#eceae4"),
        "f": rgb("#e8a878"),
        "b": rgb("#4c78c8"),
        "l": rgb("#8c8c90"),
        "L": rgb("#b4b4b8"),
    },
    bevel="c",
    body="cflbL",
)

SUBSCRIPTIONS = tile(
    [
        "rrrrrrrrrr",
        "rrrrrrrrrr",
        "ggggggggggr"[:10],
        "gGGGGGGGGg",
        "gGGGyGGGGg",
        "gGGyyyGGGg",
        "gGGGyGGGGg",
        "gGGGGGGllg",
        "gggggggggg",
        "rrrrrrrrrr",
    ],
    {
        "r": rgb("#7a2c44"),
        "g": rgb("#2c6c5c"),
        "G": rgb("#48a088"),
        "y": rgb("#f8e070"),
        "l": rgb("#c8ecdc"),
    },
    bevel="G",
    body="Gyl",
)

CHEST = tile(
    [
        "kkkkkkkkkk",
        "kkkkrrkkkk",
        "kkkbbbbkkk",
        "kkkkbbkkkk",
        "kkbbbbbbkk",
        "kbbBbbbbbk",
        "kbBbbbbbbk",
        "kbbbbbbbbk",
        "kbbbbbbbBk",
        "kkBBBBBBkk",
    ],
    {
        "k": rgb("#38485c"),
        "r": rgb("#d84838"),
        "b": rgb("#c8a070"),
        "B": rgb("#946c40"),
    },
    bevel="b",
)

STORAGE = tile(
    [
        "mmmmmmmmmm",
        "mccccccccm",
        "mcddddddcm",
        "mcdddHddcm",
        "mccccccccm",
        "mcddddddcm",
        "mcdddHddcm",
        "mccccccccm",
        "mccmmmmccm",
        "mmmmmmmmmm",
    ],
    {
        "m": rgb("#4c4250"),
        "c": rgb("#8a7a90"),
        "d": rgb("#b8aac0"),
        "H": rgb("#2c2430"),
    },
    bevel="c",
    body="cdH",
)

LANGUAGE = tile(
    [
        "fwwwwwffff",
        "fwwkwwffff",
        "fwkwkwffff",
        "fwkkkwffff",
        "fwkwkwffff",
        "fwwwwwyyyy",
        "fwfffyyyyy",
        "fffffyKKKy",
        "fffffyyKyy",
        "fffffffyyf",
    ],
    {
        "f": rgb("#7c4cb0"),
        "w": rgb("#f4f0fa"),
        "k": rgb("#6a40a0"),
        "y": rgb("#f8d860"),
        "K": rgb("#5c3c1c"),
    },
)

COMMAND = tile(
    [
        "oooooooooo",
        "oxxxxxxxxo",
        "oxwxxxxxxo",
        "oxxwxxxxxo",
        "oxxxwxxxxo",
        "oxxwxxxxxo",
        "oxwxxwwwxo",
        "oxxxxxxxxo",
        "oxxxxxxxxo",
        "oooooooooo",
    ],
    {"o": rgb("#8a9cb0"), "x": rgb("#183c34"), "w": rgb("#7cf0a0")},
)

SETTINGS = [
    ACCESSIBILITY,
    KEYBOARD_MOUSE,
    CONTROLS,
    TOUCH,
    PARTY,
    WORK_BENCH,
    PAINTING,
    SOUND,
    ACCOUNT,
    SUBSCRIPTIONS,
    CHEST,
    STORAGE,
    LANGUAGE,
    COMMAND,
]
