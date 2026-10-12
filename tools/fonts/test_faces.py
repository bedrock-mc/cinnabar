"""Check shipped faces against their committed pixel designs and spacing."""
import unittest
from pathlib import Path

from fontTools.ttLib import TTFont

import build_faces as build
import design_faces as design
import native_subset

ROOT = Path(__file__).resolve().parents[2]


class FaceTests(unittest.TestCase):
    """Verify the generated assets' coverage, grid dimensions and pair spacing."""

    @classmethod
    def setUpClass(cls):
        """Open the shipped faces and their pinned fallback once."""
        cls.source = TTFont(ROOT / 'assets/fonts/CinnanglesSans.ttf')
        cls.faces = {face: TTFont(ROOT / 'assets/fonts' / cfg['file'])
                     for face, cfg in {**build.FACES, 'seven': build.SEVEN}.items()}

    @classmethod
    def tearDownClass(cls):
        """Release the font files."""
        cls.source.close()
        for font in cls.faces.values():
            font.close()

    def test_face_and_sans_cover_every_scalar(self):
        """Each face maps exactly the Sans scalars in its native ranges; Sans draws the rest."""
        sans = self.source.getBestCmap().keys()
        for face, font in self.faces.items():
            with self.subTest(face=face):
                self.assertEqual(set(font.getBestCmap()), {cp for cp in sans if native_subset.native(cp)})
                self.assertEqual(len(set(font.getBestCmap()) | set(sans)), 55352)
                self.assertEqual(font['head'].unitsPerEm, design.UNITS_PER_EM)

    def test_pixel_dimensions_and_advances_are_shipped(self):
        """Generated ink boxes and pen widths retain the declared pixel measurements."""
        for face, font in self.faces.items():
            cmap = font.getBestCmap()
            for cp, record in design.read_design(face)['glyphs'].items():
                cp = int(cp)
                if cp not in cmap:
                    continue
                glyph = font['glyf'][cmap[cp]]
                with self.subTest(face=face, cp=cp):
                    self.assertEqual(font['hmtx'][cmap[cp]], (record['advance'], record['x']))
                    if record['rows']:
                        self.assertEqual((glyph.xMin, glyph.yMin, glyph.xMax, glyph.yMax),
                                         (record['x'], record['y'], record['x'] + sum(record['columns']),
                                          record['y'] + sum(record['heights'])))
                        self.assertTrue(all(len(row) == len(record['columns']) for row in record['rows']))
                        self.assertEqual(len(record['heights']), len(record['rows']))
                    else:
                        self.assertEqual(glyph.numberOfContours, 0)

    def test_fallback_drawings_retain_their_pixel_pitch(self):
        """Unreplaced fallback edges remain on the scaled Sans grid."""
        pitch = build.T * design.UNITS_PER_EM // build.UPEM
        for face, font in self.faces.items():
            native = {int(cp) for cp in design.read_design(face)['glyphs']}
            names = {name for cp, name in font.getBestCmap().items() if cp not in native}
            for name in names:
                glyph = font['glyf'][name]
                if glyph.numberOfContours > 0:
                    with self.subTest(face=face, glyph=name):
                        self.assertTrue(all(x % pitch == y % pitch == 0 for x, y in glyph.coordinates))

    def test_generated_kerning_matches_spacing_sources(self):
        """The shipped kern table is exactly the pairs generated from the design silhouettes."""
        for face, font in self.faces.items():
            cmap = font.getBestCmap()
            pairs = font['kern'].kernTables[0].kernTable
            expected = {(cmap[left], cmap[right]): value
                        for left, right, value in design.kerning(face)
                        if left in cmap and right in cmap}
            self.assertEqual(pairs, expected)

    def test_kerning_closes_open_pairs_only(self):
        """Overhanging pairs tighten by whole pixels; closed and cross-script pairs stay unkerned."""
        pairs = {(chr(left), chr(right)): value for left, right, value in design.kerning('seven')}
        self.assertEqual(pairs[('T', '.')], -100)
        self.assertEqual(pairs[('L', 'T')], -200)
        for pair in [('H', 'H'), ('o', 'o'), ('A', 'Б'), ('.', ',')]:
            self.assertNotIn(pair, pairs)
        self.assertTrue(all(value % 100 == 0 and -200 <= value < 0 for value in pairs.values()))

    def test_line_metrics_match_declared_faces(self):
        """Line boxes and x-height stay independent of atlas pitch."""
        for face, font in self.faces.items():
            source = design.read_design(face)
            self.assertEqual((font['hhea'].ascent, font['hhea'].descent, font['hhea'].lineGap),
                             (source['ascent'], source['descent'], 0))
            self.assertEqual(font['OS/2'].sxHeight, source['x_height'])

    def test_seven_descenders_and_x_height(self):
        """Body letters retain their five-cell x-height and one-cell descenders."""
        font = self.faces['seven']
        cmap = font.getBestCmap()
        for ch in 'acegmnoqprsuvwxyz':
            self.assertEqual(font['glyf'][cmap[ord(ch)]].yMax, 500, ch)
        for ch in 'gjpqy':
            self.assertEqual(font['glyf'][cmap[ord(ch)]].yMin, -100, ch)


if __name__ == '__main__':
    unittest.main()
