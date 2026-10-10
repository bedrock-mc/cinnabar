"""Original small symbols, drawn from grids and integer-coordinate recipes."""

from pixels import PixelArt, cube, person, star


MASKS = {
    'chevron-left': ['...#', '..##', '.##.', '##..', '.##.', '..##', '...#'],
    'chevron-up': ['...#...', '..###..', '.##.##.', '##...##'],
    'chevron-down': ['##...##', '.##.##.', '..###..', '...#...'],
    'reset': [
        '............', '....#####...', '...##...##..', '..##.....##.',
        '..##........', '######......', '.####.......', '..##.....##.',
        '...##...##..', '....#####...', '............', '............',
    ],
    'external-link': [
        '.....#######', '.......#####', '........####', '.......##.##',
        '##....##...#', '##...##.....', '##..##......', '##..........',
        '##........##', '##........##', '############', '############',
    ],
}


def mask_icon(name):
    """Render an original monochrome glyph at its declared native size."""
    rows = MASKS[name]
    art = PixelArt(len(rows[0]), len(rows))
    art.grid(rows, {'#': 'white'})
    return art.image


def book(art, x, y, cover='red'):
    """Draw a closed field book with a pale page edge and clasp."""
    art.rect((x, y, x+13, y+16), 'ink')
    art.rect((x+1, y+1, x+11, y+13), cover)
    art.rect((x+3, y+1, x+3, y+12), 'white')
    art.rect((x+2, y+14, x+12, y+15), 'mist')
    art.rect((x+10, y+6, x+13, y+8), 'gold')


def envelope(art, x, y):
    """Draw a sealed envelope with a diagonal fold and a wax seal."""
    art.rect((x, y, x+17, y+12), 'ink')
    art.rect((x+1, y+1, x+16, y+11), 'cream')
    art.line([(x+1,y+2),(x+8,y+7),(x+16,y+2)], 'ochre')
    art.rect((x+7,y+6,x+10,y+9), 'red')


def chest(art, x, y, shade='clay'):
    """Draw a storage trunk with metal corners and a central latch."""
    art.rect((x, y+2, x+17, y+15), 'ink')
    art.rect((x+2, y, x+15, y+1), 'ink')
    art.rect((x+1, y+3, x+16, y+6), shade)
    art.rect((x+2, y+1, x+15, y+2), 'gold')
    art.rect((x+1, y+8, x+16, y+14), 'wood')
    for dx in (2,14):
        art.rect((x+dx,y+3,x+dx+1,y+14), 'ochre')
    art.rect((x+7,y+6,x+10,y+10), 'ink')
    art.rect((x+8,y+7,x+9,y+9), 'cream')


def landscape_badge(art):
    """Draw a map tile with a winding stream and a single mountain."""
    art.poly([(3,3),(18,2),(21,5),(20,20),(5,21),(2,18)], 'ink')
    art.rect((4,4,18,18), 'leaf')
    art.poly([(5,12),(10,5),(15,12)], 'pine')
    art.poly([(8,8),(10,5),(12,8)], 'white')
    art.line([(18,10),(13,12),(15,15),(9,18),(5,18)], 'sky', 2)


def realms(art):
    """Draw a floating watchtower over a small violet island."""
    art.poly([(2,16),(11,13),(21,16),(12,22)], 'plum')
    art.poly([(2,16),(11,13),(21,16),(12,18)], 'lilac')
    art.rect((8,6,15,15), 'ink')
    art.rect((9,7,14,14), 'mist')
    art.rect((11,10,12,15), 'navy')
    for x in (7,11,15):
        art.rect((x,4,x+1,7), 'white')
    art.rect((9,6,14,7), 'white')
    star(art,4,7,'gold',1)
    star(art,19,3,'lilac',1)


def server(art):
    """Draw three server drawers with green indicator lights."""
    art.rect((4,2,19,21), 'ink')
    for y in (3,9,15):
        art.rect((5,y,18,y+4), 'slate')
        art.rect((5,y,18,y), 'steel')
        art.rect((7,y+2,11,y+2), 'shadow')
        art.rect((15,y+1,16,y+2), 'leaf')


def accessibility(art):
    """Draw an open-armed standing figure within four corner marks."""
    for x in (2,19):
        for y in (2,19):
            art.rect((x,y,x+2,y+2), 'blue')
    art.rect((10,3,13,6), 'white')
    art.line([(5,9),(11,11),(18,9)], 'aqua', 2)
    art.rect((10,8,13,14), 'white')
    art.line([(11,14),(8,20)], 'white', 2)
    art.line([(12,14),(15,20)], 'white', 2)


def keyboard(art):
    """Draw a compact keyboard and its wired mouse."""
    art.rect((1,10,16,20), 'ink')
    art.rect((2,11,15,19), 'steel')
    for y in (12,15):
        for x in (3,6,9,12):
            art.rect((x,y,x+1,y+1), 'shadow')
    art.rect((5,18,12,18), 'white')
    art.line([(19,6),(19,3),(12,3),(12,7)], 'slate')
    art.rect((17,7,22,16), 'ink')
    art.rect((18,8,21,15), 'mist')
    art.rect((19,8,20,10), 'blue')


def controls(art):
    """Draw a gamepad with a directional cross and two coloured buttons."""
    art.poly([(5,5),(18,5),(21,8),(22,18),(17,20),(14,16),(9,16),(6,20),(1,18),(2,8)], 'ink')
    art.poly([(5,6),(18,6),(20,9),(21,17),(18,18),(15,14),(8,14),(5,18),(2,17),(3,9)], 'steel')
    art.rect((5,8,6,13), 'ink')
    art.rect((3,10,8,11), 'ink')
    art.rect((16,8,17,9), 'leaf')
    art.rect((18,11,19,12), 'red')
    art.rect((10,9,12,10), 'shadow')


def touch(art):
    """Draw a touch screen with a visible fingertip and tap rays."""
    art.rect((3,2,16,21), 'ink')
    art.rect((4,3,15,18), 'blue')
    art.rect((6,5,13,15), 'sky')
    art.rect((9,19,10,19), 'mist')
    art.poly([(10,14),(10,9),(12,9),(12,14),(17,13),(20,16),(18,21),(13,21),(8,17),(8,15)], 'cream')
    art.line([(17,7),(20,5)], 'gold')
    art.line([(18,10),(22,10)], 'gold')


def workbench(art):
    """Draw a carpenter's bench with a vise, board, and four sturdy legs."""
    art.rect((2,9,21,13), 'ink')
    art.rect((3,9,20,11), 'clay')
    for x in (4,17):
        art.rect((x,14,x+2,21), 'wood')
    art.rect((5,18,18,19), 'earth')
    art.rect((7,5,18,8), 'cream')
    art.rect((4,7,6,15), 'steel')
    art.rect((3,15,7,16), 'slate')


def painting(art):
    """Draw a framed sunset over overlapping mountains."""
    art.rect((2,3,21,20), 'ink')
    art.rect((3,4,20,19), 'gold')
    art.rect((5,6,18,17), 'sky')
    art.rect((14,7,16,9), 'cream')
    art.poly([(5,15),(10,9),(15,16),(18,12),(18,17),(5,17)], 'pine')
    art.line([(3,4),(20,4)], 'cream')


def sound(art):
    """Draw a speaker cabinet with a square cone and sound rays."""
    art.rect((3,3,15,21), 'ink')
    art.rect((4,4,14,20), 'wood')
    art.rect((6,6,12,8), 'gold')
    art.rect((6,11,12,17), 'ink')
    art.rect((7,12,11,16), 'slate')
    art.rect((8,13,10,15), 'steel')
    art.line([(18,7),(20,10),(20,14),(18,17)], 'aqua')
    art.rect((17,10,17,14), 'sky')


def account(art):
    """Draw a portrait card with a visible identity tab."""
    art.rect((3,2,20,21), 'ink')
    art.rect((4,3,19,20), 'blue')
    art.rect((7,2,16,4), 'gold')
    person(art,8,7,'mist','cream')
    art.rect((7,18,16,18), 'navy')


def calendar(art):
    """Draw a subscription calendar with a star on the active day."""
    art.rect((3,4,20,21), 'ink')
    art.rect((4,5,19,9), 'purple')
    art.rect((4,10,19,20), 'white')
    for x in (7,16):
        art.rect((x,2,x+1,6), 'steel')
    for x in (6,9,12,15):
        art.rect((x,12,x+1,13), 'slate')
    star(art,13,17,'ochre',2)


def storage(art):
    """Draw two archive drawers with label slots."""
    art.rect((3,3,20,21), 'ink')
    for y in (4,13):
        art.rect((4,y,19,y+6), 'teal')
        art.rect((4,y,19,y), 'aqua')
        art.rect((8,y+2,15,y+4), 'ink')
        art.rect((9,y+2,14,y+3), 'mist')


def language(art):
    """Draw overlapping speech cards with Latin and abstract script marks."""
    art.poly([(2,3),(15,3),(15,14),(7,14),(4,17),(4,14),(2,14)], 'ink')
    art.rect((3,4,14,13), 'white')
    art.grid(['.##.', '#..#', '####', '#..#', '#..#'], {'#':'navy'}, (6,6))
    art.poly([(12,11),(22,11),(22,20),(20,20),(20,23),(17,20),(12,20)], 'blue')
    art.line([(14,14),(20,14)], 'white')
    art.line([(17,12),(17,17),(15,18)], 'white')
    art.line([(15,16),(19,18)], 'white')


def command(art):
    """Draw a terminal panel with a prompt and a cursor."""
    art.rect((2,4,21,20), 'ink')
    art.rect((3,5,20,7), 'clay')
    art.rect((3,8,20,19), 'shadow')
    art.line([(5,10),(8,13),(5,16)], 'leaf', 2)
    art.rect((11,16,16,17), 'white')
    art.rect((17,6,18,6), 'cream')


def news(art):
    """Draw a folded newspaper with a photo and crisp column marks."""
    art.rect((3,3,19,21), 'ink')
    art.rect((4,4,18,20), 'white')
    art.rect((6,6,16,7), 'blue')
    art.rect((6,10,10,14), 'teal')
    art.poly([(6,14),(8,11),(10,14)], 'leaf')
    for y in (10,13,16,18):
        art.rect((12 if y<16 else 6,y,16,y), 'slate')
    art.rect((20,7,21,19), 'steel')


def feedback(art):
    """Draw a comment card with a pencil laid across its lower corner."""
    art.poly([(2,3),(19,3),(19,16),(10,16),(5,20),(5,16),(2,16)], 'ink')
    art.rect((3,4,18,15), 'white')
    art.rect((5,6,15,7), 'teal')
    art.rect((5,10,11,11), 'slate')
    art.line([(12,19),(20,11)], 'ochre', 4)
    art.line([(13,18),(21,10)], 'gold', 2)
    art.poly([(10,21),(11,17),(14,20)], 'cream')


def advanced(art):
    """Draw three tuning sliders with independently placed controls."""
    for x,y,shade in ((5,8,'sky'),(11,15,'gold'),(18,10,'leaf')):
        art.rect((x,3,x+1,20), 'ink')
        art.rect((x+1,3,x+1,20), 'steel')
        art.rect((x-2,y-2,x+3,y+2), 'ink')
        art.rect((x-1,y-1,x+2,y+1), shade)


def flask(art):
    """Draw a square-necked experimental flask with bubbles."""
    art.poly([(8,3),(15,3),(15,5),(14,5),(14,10),(20,19),(19,22),(4,22),(3,19),(9,10),(9,5),(8,5)], 'ink')
    art.poly([(10,5),(13,5),(13,11),(18,19),(18,20),(5,20),(5,19),(10,11)], 'mist')
    art.poly([(8,14),(15,14),(18,19),(18,20),(5,20),(5,19)], 'purple')
    art.rect((8,16,9,17), 'lilac')
    star(art,18,5,'gold',1)
    art.rect((11,11,12,12), 'purple')


def wand(art):
    """Draw a diagonal creative wand with three sparks."""
    art.line([(5,20),(17,8)], 'ink', 5)
    art.line([(5,19),(17,7)], 'wood', 3)
    art.line([(14,10),(17,7)], 'white', 3)
    star(art,7,5,'gold',2)
    star(art,20,14,'lilac',2)
    star(art,15,2,'cream',1)


def packs(art, behavior=False):
    """Draw layered pack cards, with either colour swatches or a logic path."""
    art.rect((6,2,20,17), 'ink')
    art.rect((7,3,19,16), 'slate')
    art.rect((2,6,17,22), 'ink')
    art.rect((3,7,16,21), 'clay' if behavior else 'mist')
    if behavior:
        art.line([(6,10),(12,10),(12,18),(7,18)], 'earth', 2)
        for x,y in ((5,9),(11,13),(6,17)):
            art.rect((x,y,x+2,y+2), 'cream')
    else:
        for x,y,c in ((5,10,'leaf'),(11,10,'sky'),(5,16,'purple'),(11,16,'red')):
            art.rect((x,y,x+3,y+3), c)


def small_icon(name):
    """Draw a 12-pixel status symbol without shrinking a larger drawing."""
    art = PixelArt(12,12)
    if name == 'players':
        person(art,1,1,'mist','white')
        art.rect((9,6,10,7), 'leaf')
    elif name == 'pass':
        art.rect((1,2,10,9), 'ink')
        art.rect((2,3,9,8), 'gold')
        star(art,6,5,'white',2)
        art.rect((1,5,2,6), (0,0,0,0))
        art.rect((9,5,10,6), (0,0,0,0))
    return art.image


def summary(name):
    """Draw profile overview symbols directly on their sixteen-pixel grid."""
    art = PixelArt(16,16)
    if name in ('friends','followers'):
        person(art,1,3,'mist','white')
        if name == 'friends':
            person(art,8,4,'steel','mist')
        else:
            art.rect((10,7,14,8),'white')
            art.rect((12,5,12,10),'white')
    elif name == 'gallery':
        art.rect((1,2,14,13),'mist')
        art.rect((3,4,12,11),'shadow')
        art.rect((10,5,11,6),'white')
        art.poly([(3,11),(7,6),(11,11)],'steel')
    else:
        art.rect((4,2,11,7),'white')
        art.line([(3,3),(1,3),(1,7),(5,9)],'steel',2)
        art.line([(12,3),(14,3),(14,7),(10,9)],'steel',2)
        art.rect((6,8,9,11),'steel')
        art.rect((4,12,11,13),'white')
    return art.image


def stat(name):
    """Draw a high-contrast monochrome statistic symbol with stepped shading."""
    art = PixelArt(24,24)
    if name == 'clock':
        art.poly([(7,2),(16,2),(21,7),(21,16),(16,21),(7,21),(2,16),(2,7)],'slate')
        art.poly([(8,4),(15,4),(19,8),(19,15),(15,19),(8,19),(4,15),(4,8)],'mist')
        art.line([(11,6),(11,12),(16,14)],'shadow',2)
    elif name == 'pickaxe':
        art.line([(5,20),(16,9)],'slate',4)
        art.line([(5,19),(15,9)],'mist',2)
        art.poly([(5,4),(14,3),(21,10),(20,15),(17,10),(12,7),(5,7)],'steel')
        art.line([(6,4),(13,4),(19,10)],'mist')
    elif name == 'sword':
        art.poly([(18,2),(22,2),(22,6),(10,18),(6,14)],'mist')
        art.line([(20,4),(9,15)],'steel')
        art.line([(4,12),(12,20)],'slate',3)
        art.line([(7,17),(3,21)],'steel',3)
    else:
        for x,y in ((3,3),(12,6)):
            art.poly([(x,y),(x+6,y),(x+6,y+9),(x+8,y+9),(x+8,y+14),(x,y+14)],'slate')
            art.rect((x+1,y+1,x+5,y+9),'steel')
            art.rect((x+1,y+10,x+7,y+12),'mist')
            art.rect((x+2,y+3,x+4,y+4),'shadow')
    return art.image


def heart():
    """Draw a stone-grey heart with a recessed central fissure."""
    art = PixelArt(20,20)
    rows = [
        '..####....####..', '.#rrrr#..#rrrr#.', '#rrRRrr##rrRRrr#',
        '#rRrrrrrrrrrrRr#', '#rRrrrrrrrrrrrr#', '#rrrrrrrrrrrrrr#',
        '.#rrrrrrrrrrrr#.', '..#rrr####rrr#..', '...#rrr##rrr#...',
        '....#rrrrrr#....', '.....#rrrr#.....', '......#rr#......',
        '.......##.......',
    ]
    art.grid(rows, {'#':'ink','r':'steel','R':'mist'}, (2,3))
    art.line([(8,7),(7,10),(10,10),(9,13)],'shadow')
    return art.image


def gamerscore():
    """Draw a generic score medallion with an original diamond emblem."""
    art = PixelArt(20,20)
    art.poly([(6,1),(13,1),(18,6),(18,13),(13,18),(6,18),(1,13),(1,6)],'steel')
    art.poly([(6,3),(13,3),(16,6),(16,13),(13,16),(6,16),(3,13),(3,6)],'white')
    art.poly([(9,5),(14,9),(9,14),(5,9)],'shadow')
    art.rect((8,8,10,10),'mist')
    return art.image


def icon(name):
    """Dispatch a named drawing, returning its native unscaled RGBA pixels."""
    if name in MASKS:
        return mask_icon(name)
    if name in ('players','pass'):
        return small_icon(name)
    if name in ('friends','followers','gallery','achievements'):
        return summary(name)
    if name in ('clock','pickaxe','sword','boots'):
        return stat(name)
    if name == 'hardcore':
        return heart()
    if name == 'gamerscore':
        return gamerscore()
    art = PixelArt(24,24)
    painters = {
        'worlds':landscape_badge, 'realms':realms, 'servers':server,
        'accessibility':accessibility, 'keyboard':keyboard, 'controls':controls,
        'touch':touch, 'workbench':workbench, 'painting':painting,
        'sound':sound, 'account':account, 'subscriptions':calendar,
        'storage':storage, 'language':language, 'command':command,
        'news':news, 'feedback':feedback, 'advanced':advanced,
        'experimental':flask, 'cheats':wand,
    }
    if name in painters:
        painters[name](art)
    elif name in ('party','multiplayer'):
        person(art,2,7,'blue','cream')
        person(art,15,7,'purple','mist')
        person(art,8,3,'leaf','cream')
    elif name == 'invites':
        envelope(art,3,6)
    elif name == 'chest':
        chest(art,3,4)
    elif name == 'general':
        book(art,5,3,'green')
    elif name in ('resources','behavior'):
        packs(art,name=='behavior')
    else:
        raise ValueError(f'No icon recipe: {name}')
    return art.image
