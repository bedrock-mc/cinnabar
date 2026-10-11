"""Verify generated faces without loading any proprietary reference font.

Run with Python 3.14: python3 -m unittest discover -s tools/fonts -v
"""
import string
import unittest
from pathlib import Path

import numpy as np
from fontTools.ttLib import TTFont

import build_faces as build
from face_metrics import ADVANCES, advance_units

ROOT = Path(__file__).resolve().parents[2]


class FaceTests(unittest.TestCase):
    """Keep source coverage, declared spacing and the raster-grid contract intact."""

    @classmethod
    def setUpClass(cls):
        """Load the four shipped outputs and their common source once."""
        cls.source = TTFont(ROOT / 'assets/fonts/CinnanglesSans.ttf')
        cls.faces = {face: TTFont(ROOT / 'assets/fonts' / cfg['file'])
                     for face, cfg in {**build.FACES, 'seven': build.SEVEN}.items()}

    @classmethod
    def tearDownClass(cls):
        """Release every opened font file."""
        cls.source.close()
        for font in cls.faces.values():
            font.close()

    def test_every_printable_ascii_advance_is_declared_and_shipped(self):
        """The declared spacing applies to both capital and lowercase cmap entries."""
        for face, font in self.faces.items():
            cmap = font.getBestCmap()
            for cp in range(32, 127):
                ch = chr(cp)
                expected = advance_units(face, ch, build.UPEM)
                with self.subTest(face=face, ch=ch):
                    self.assertIsNotNone(expected)
                    self.assertEqual(font['hmtx'][cmap[cp]][0], expected)

    def test_complete_coverage_and_grid_survive_redrawing(self):
        """All mapped fallback scalars survive and no outline edge leaves the texel grid."""
        coverage = self.source.getBestCmap().keys()
        for face, font in self.faces.items():
            with self.subTest(face=face):
                self.assertEqual(font.getBestCmap().keys(), coverage)
                self.assertEqual(font['head'].unitsPerEm, build.UPEM)
                for glyph in font['glyf'].glyphs.values():
                    glyph.expand(font['glyf'])
                    if glyph.numberOfContours > 0:
                        self.assertTrue(all(x % build.T == y % build.T == 0
                                            for x, y in glyph.coordinates))

    def test_capital_aliases_share_spacing_and_drawings(self):
        """Caps faces keep ASCII case aliases and consistent Greek/Cyrillic twins."""
        for face in build.FACES:
            cmap = self.faces[face].getBestCmap()
            glyf = self.faces[face]['glyf']
            metrics = self.faces[face]['hmtx']
            for ch in string.ascii_lowercase:
                self.assertEqual(cmap[ord(ch)], cmap[ord(ch.upper())])
            for cp, ch in build.HOMOGLYPHS.items():
                with self.subTest(face=face, cp=cp):
                    self.assertEqual(metrics[cmap[cp]], metrics[cmap[ord(ch)]])
                    self.assertEqual(glyf[cmap[cp]].getCoordinates(glyf)[0],
                                     glyf[cmap[ord(ch)]].getCoordinates(glyf)[0])

    def test_seven_descenders_and_x_height(self):
        """Body letters keep a half-em x-height and one design-pixel descenders."""
        font = self.faces['seven']
        cmap = font.getBestCmap()
        for ch in 'acegmnoqprsuvwxyz':
            self.assertEqual(font['glyf'][cmap[ord(ch)]].yMax, font['OS/2'].sxHeight, ch)
        for ch in 'gjpqy':
            self.assertEqual(font['glyf'][cmap[ord(ch)]].yMin, -build.DESIGN * build.T, ch)

    def test_seven_ascii_leaves_room_for_the_carrier_bearing(self):
        """Narrowed advances must retain spacing on both sides of the source ink."""
        font = self.faces['seven']
        cmap = font.getBestCmap()
        for cp in range(33, 127):
            g = cmap[cp]
            glyph = font['glyf'][g]
            with self.subTest(ch=chr(cp)):
                self.assertLessEqual(glyph.xMax - glyph.xMin + build.DESIGN * build.T,
                                     font['hmtx'][g][0])

    def test_bold_five_keeps_width_and_adds_ink(self):
        """Bold adds weight within the shared box; narrow I uses a wider drawing."""
        regular, bold = build.face_drawings('five'), build.face_drawings('five-bold')
        for ch in string.ascii_uppercase.replace("I", ""):
            with self.subTest(ch=ch):
                self.assertEqual(regular[ch].shape, bold[ch].shape)
                self.assertTrue(np.all(bold[ch][regular[ch]]))
                self.assertGreater(np.count_nonzero(bold[ch]), np.count_nonzero(regular[ch]))

    def test_authored_artwork_is_bounded_and_capitals_keep_the_baseline(self):
        """All drawings fit the common window, with full-height display capitals."""
        for face in ADVANCES:
            for ch, bitmap in build.face_drawings(face).items():
                with self.subTest(face=face, ch=ch):
                    self.assertEqual(bitmap.shape[0], build.ROWS)
                    self.assertTrue(bitmap.any())
                    if face != 'seven' and ch.isalnum():
                        rows = np.where(bitmap.any(axis=1))[0]
                        self.assertEqual((rows[0], rows[-1]), (build.CAP_ROW, build.BASE_ROW - 1))


if __name__ == '__main__':
    unittest.main()
