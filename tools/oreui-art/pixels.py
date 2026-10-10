"""Small pixel-art primitives and a hand-chosen palette shared by the artists."""

from PIL import Image, ImageDraw

# These colours are authored here, not extracted from any reference image.
PALETTE = {
    'ink': '#25313d', 'shadow': '#3b4c5c', 'slate': '#637c8a',
    'steel': '#9cb1b9', 'mist': '#cbd9d6', 'white': '#f5f0dc',
    'green': '#63964d', 'leaf': '#98bf67', 'pine': '#345e4b',
    'moss': '#49794e', 'teal': '#428d8a', 'aqua': '#83c5bb',
    'blue': '#4c81ae', 'sky': '#93c9da', 'navy': '#344a76',
    'purple': '#8966aa', 'lilac': '#b895c8', 'plum': '#574263',
    'red': '#c45455', 'rose': '#e5897c', 'wine': '#7a3c4b',
    'gold': '#d9ae58', 'cream': '#f2d391', 'ochre': '#9d733a',
    'wood': '#956643', 'clay': '#bb8a5b', 'earth': '#604738',
}


def color(name):
    """Resolve a named palette colour, while allowing explicit RGBA colours."""
    return PALETTE.get(name, name) if isinstance(name, str) else name


class PixelArt:
    """Draw hard-edged integer shapes on a transparent RGBA canvas."""

    def __init__(self, width, height, background=(0, 0, 0, 0)):
        """Create a canvas with no antialiasing or reference-image inputs."""
        self.image = Image.new('RGBA', (width, height), color(background))
        self.draw = ImageDraw.Draw(self.image)

    def rect(self, box, fill):
        """Fill an inclusive pixel rectangle using a named palette colour."""
        self.draw.rectangle(box, fill=color(fill))

    def poly(self, points, fill):
        """Fill a polygon with integer vertices and hard pixel edges."""
        self.draw.polygon(points, fill=color(fill))

    def line(self, points, fill, width=1):
        """Join integer coordinates with a solid pixel stroke."""
        self.draw.line(points, fill=color(fill), width=width)

    def grid(self, rows, colors, at=(0, 0)):
        """Paint a text grid, treating dots as transparent and rejecting ragged rows."""
        assert len({len(row) for row in rows}) == 1
        for y, row in enumerate(rows):
            for x, token in enumerate(row):
                if token != '.':
                    self.image.putpixel((at[0] + x, at[1] + y), rgba(colors[token]))

    def paste(self, image, at):
        """Composite another original drawing over this canvas."""
        self.image.alpha_composite(image, at)


def rgba(value):
    """Convert one palette colour to an RGBA tuple."""
    from PIL import ImageColor
    value = color(value)
    return ImageColor.getcolor(value, 'RGBA') if isinstance(value, str) else value


def enlarge(image, factor):
    """Enlarge original pixels by a whole-number factor without smoothing."""
    return image.resize((image.width * factor, image.height * factor), Image.Resampling.NEAREST)


def star(art, x, y, shade='cream', radius=2):
    """Draw a small four-point glint centred on one pixel."""
    art.line([(x-radius, y), (x+radius, y)], shade)
    art.line([(x, y-radius), (x, y+radius)], shade)


def person(art, x, y, shirt='blue', skin='cream'):
    """Draw a seven-pixel-wide bust with a square head and stepped shoulders."""
    art.rect((x+2, y, x+4, y+3), 'ink')
    art.rect((x+2, y, x+4, y+2), skin)
    art.rect((x+1, y+5, x+5, y+8), shirt)
    art.rect((x, y+6, x+6, y+8), shirt)
    art.rect((x, y+9, x+6, y+9), 'ink')


def cube(art, x, y, size=18, top='leaf', left='wood', right='earth'):
    """Draw a solid isometric block with three independently coloured faces."""
    h = size // 2
    q = size // 4
    art.poly([(x+h,y), (x+size,y+q), (x+size,y+3*q),
              (x+h,y+size), (x,y+3*q), (x,y+q)], 'ink')
    art.poly([(x+h,y+1), (x+size-2,y+q), (x+h,y+2*q-1), (x+2,y+q)], top)
    art.poly([(x+1,y+q+1), (x+h-1,y+2*q), (x+h-1,y+size-2), (x+1,y+3*q-1)], left)
    art.poly([(x+h,y+2*q), (x+size-1,y+q+1), (x+size-1,y+3*q-1), (x+h,y+size-2)], right)
