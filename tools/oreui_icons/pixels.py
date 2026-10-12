"""Pixel canvas, palettes and a deterministic RGBA PNG writer; standard library only."""

import struct
import zlib

Color = tuple  # (r, g, b, a)
CLEAR = (0, 0, 0, 0)


def rgb(hex_code, alpha=255):
    """Returns an RGBA tuple for a `#rrggbb` code."""
    value = int(hex_code.lstrip("#"), 16)
    return (value >> 16, (value >> 8) & 255, value & 255, alpha)


class Canvas:
    """A straight-alpha RGBA grid whose drawing calls clip at the edges."""

    def __init__(self, width, height, fill=CLEAR):
        self.width, self.height = width, height
        self.px = [[fill] * width for _ in range(height)]

    def set(self, x, y, color):
        if 0 <= x < self.width and 0 <= y < self.height and color is not None:
            self.px[y][x] = color

    def get(self, x, y):
        return self.px[y][x]

    def rect(self, x, y, w, h, color):
        for row in range(y, y + h):
            for col in range(x, x + w):
                self.set(col, row, color)

    def frame(self, x, y, w, h, color, weight=1):
        self.rect(x, y, w, weight, color)
        self.rect(x, y + h - weight, w, weight, color)
        self.rect(x, y, weight, h, color)
        self.rect(x + w - weight, y, weight, h, color)

    def line(self, x0, y0, x1, y1, color):
        """Bresenham line, both ends inclusive."""
        dx, dy = abs(x1 - x0), -abs(y1 - y0)
        sx, sy = (1 if x0 < x1 else -1), (1 if y0 < y1 else -1)
        err = dx + dy
        while True:
            self.set(x0, y0, color)
            if x0 == x1 and y0 == y1:
                return
            e2 = 2 * err
            if e2 >= dy:
                err += dy
                x0 += sx
            if e2 <= dx:
                err += dx
                y0 += sy

    def disc(self, cx, cy, r, color):
        """Fills cells whose centres lie within `r` of the (possibly half-cell) centre."""
        for y in range(self.height):
            for x in range(self.width):
                if (x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2 <= r * r:
                    self.set(x, y, color)

    def ring(self, cx, cy, outer, inner, color):
        for y in range(self.height):
            for x in range(self.width):
                d = (x + 0.5 - cx) ** 2 + (y + 0.5 - cy) ** 2
                if inner * inner < d <= outer * outer:
                    self.set(x, y, color)

    def stamp(self, rows, palette, x=0, y=0):
        """Draws a character map; `.` and space are transparent, unknown keys are errors."""
        for dy, row in enumerate(rows):
            for dx, key in enumerate(row):
                if key in ". ":
                    continue
                self.set(x + dx, y + dy, palette[key])

    def paste(self, other, x=0, y=0):
        """Copies the opaque-or-translucent cells of `other` over this canvas."""
        for dy in range(other.height):
            for dx in range(other.width):
                color = other.px[dy][dx]
                if color[3]:
                    self.set(x + dx, y + dy, color)

    def scaled(self, factor):
        out = Canvas(self.width * factor, self.height * factor)
        for y in range(out.height):
            for x in range(out.width):
                out.px[y][x] = self.px[y // factor][x // factor]
        return out

    def flipped(self):
        out = Canvas(self.width, self.height)
        for y in range(self.height):
            out.px[y] = list(reversed(self.px[y]))
        return out

    def recolored(self, color):
        """Keeps the alpha shape and replaces every RGB."""
        out = Canvas(self.width, self.height)
        for y in range(self.height):
            for x in range(self.width):
                if self.px[y][x][3]:
                    out.px[y][x] = color[:3] + (self.px[y][x][3],)
        return out

    def outline(self, color, diagonal=False):
        """Adds a one-cell outline around every inked cell."""
        out = Canvas(self.width, self.height)
        out.px = [row[:] for row in self.px]
        steps = [(1, 0), (-1, 0), (0, 1), (0, -1)]
        if diagonal:
            steps += [(1, 1), (1, -1), (-1, 1), (-1, -1)]
        for y in range(self.height):
            for x in range(self.width):
                if self.px[y][x][3]:
                    continue
                for sx, sy in steps:
                    nx, ny = x + sx, y + sy
                    if 0 <= nx < self.width and 0 <= ny < self.height and self.px[ny][nx][3]:
                        out.px[y][x] = color
                        break
        return out

    def png(self):
        """Encodes 8-bit RGBA, non-interlaced, at a fixed zlib level."""
        raw = bytearray()
        for row in self.px:
            raw.append(0)
            for color in row:
                raw.extend(color)
        chunks = [
            (b"IHDR", struct.pack(">IIBBBBB", self.width, self.height, 8, 6, 0, 0, 0)),
            (b"IDAT", zlib.compress(bytes(raw), 9)),
            (b"IEND", b""),
        ]
        out = bytearray(b"\x89PNG\r\n\x1a\n")
        for kind, data in chunks:
            out.extend(struct.pack(">I", len(data)))
            out.extend(kind)
            out.extend(data)
            out.extend(struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF))
        return bytes(out)


def from_map(rows, palette):
    """Builds a canvas sized to a character map."""
    canvas = Canvas(max(len(row) for row in rows), len(rows))
    canvas.stamp(rows, palette)
    return canvas


def shade(color, amount):
    """Moves RGB toward white (positive) or black (negative) by `amount` in 0..1."""
    target = 255 if amount > 0 else 0
    a = abs(amount)
    return tuple(round(c + (target - c) * a) for c in color[:3]) + (color[3],)
