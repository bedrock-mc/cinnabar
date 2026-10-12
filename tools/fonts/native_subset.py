"""Limit a generated face to the ranges it draws natively; Cinnangles Sans supplies the rest."""
import sys

from fontTools import subset
from fontTools.ttLib import TTFont

# Latin, Greek, Cyrillic and symbol ranges, plus the replacement character.
NATIVE_RANGES = (
    (0x0020, 0x007E), (0x00A0, 0x024F), (0x0250, 0x02AF), (0x0370, 0x052F),
    (0x1D00, 0x1D7F), (0x1E00, 0x1EFF), (0x2000, 0x209F), (0x20A0, 0x218F),
    (0x2190, 0x22FF), (0x2500, 0x25FF), (0x2600, 0x27BF), (0xFFFD, 0xFFFD),
)


def native(cp):
    """Whether the face, rather than the Sans fallback, draws this scalar."""
    return any(lo <= cp <= hi for lo, hi in NATIVE_RANGES)


def subset_native(path):
    """Drop every mapped scalar outside NATIVE_RANGES, leaving kept outlines and metrics as is."""
    font = TTFont(path, recalcTimestamp=False)
    options = subset.Options()
    options.glyph_names = False
    options.notdef_outline = True
    options.hinting = True
    options.legacy_kern = True
    options.name_IDs = ["*"]
    options.name_languages = ["*"]
    options.name_legacy = True
    options.layout_features = []
    options.prune_unicode_ranges = False
    options.recalc_bounds = False
    options.recalc_timestamp = False
    subsetter = subset.Subsetter(options)
    subsetter.populate(unicodes=[cp for cp in font.getBestCmap() if native(cp)])
    subsetter.subset(font)
    # The subsetter also keeps bidi-mirrored partners; Sans draws those too.
    for table in font["cmap"].tables:
        table.cmap = {cp: glyph for cp, glyph in table.cmap.items() if native(cp)}
    font.save(path)


if __name__ == "__main__":
    for argument in sys.argv[1:]:
        subset_native(argument)
