"""Measured advances in thousandths of an em, independent of our pixel drawings.

Only scalar spacing measurements are recorded. Ink stays on the 64-unit grid;
advances round to the nearest font unit. Lowercase display letters use capitals.
"""
import string

TEN_ADVANCES = {
    **dict.fromkeys(string.ascii_uppercase + string.digits, 595),
    'C': 525, 'I': 315, 'L': 525, 'M': 805, 'N': 665, 'Q': 615, 'W': 805, '1': 455,
    ' ': 245, '!': 315, '"': 595, '#': 735, '$': 595, '%': 735, '&': 735, "'": 315,
    '(': 595, ')': 595, '*': 525, '+': 525, ',': 315, '-': 455, '.': 315, '/': 735,
    ':': 315, ';': 315, '<': 595, '=': 525, '>': 595, '?': 665, '@': 952, '[': 465,
    '\\': 735, ']': 465, '^': 455, '_': 805, '`': 315, '{': 525, '|': 315, '}': 525,
    '~': 525,
}
SEVEN_ADVANCES = {
    **dict.fromkeys(string.ascii_letters + string.digits, 600),
    'I': 400, 'f': 500, 'i': 200, 'j': 400, 'k': 500, 'l': 300, 't': 400,
    ' ': 300, '!': 200, '"': 400, '#': 600, '$': 600, '%': 600, '&': 600, "'": 200,
    '(': 500, ')': 500, '*': 400, '+': 600, ',': 200, '-': 400, '.': 200, '/': 600,
    ':': 200, ';': 200, '<': 500, '=': 600, '>': 500, '?': 600, '@': 700, '[': 400,
    '\\': 600, ']': 400, '^': 600, '_': 600, '`': 300, '{': 500, '|': 200, '}': 500,
    '~': 700,
}
FIVE_ADVANCES = {
    **dict.fromkeys(string.ascii_uppercase + string.digits, 840), 'I': 560, '1': 560,
    ' ': 420, '!': 280, '"': 560, '#': 840, '$': 840, '%': 840, '&': 840, "'": 280,
    '(': 420, ')': 420, '*': 560, '+': 560, ',': 280, '-': 420, '.': 280, '/': 840,
    ':': 280, ';': 280, '<': 560, '=': 560, '>': 560, '?': 840,
}
FIVE_BOLD_ADVANCES = {
    **FIVE_ADVANCES, 'I': 579, '1': 577, '!': 318, '"': 578, "'": 318,
    '(': 449, ')': 449, '*': 570, '+': 570, ',': 318, '-': 440, '.': 318,
    ':': 318, ';': 318, '<': 579, '=': 571, '>': 579, '@': 981, '[': 449,
    '\\': 840, ']': 449, '^': 579, '_': 840, '`': 449, '{': 579, '|': 318,
    '}': 579, '~': 710,
}
# The regular face has no @ glyph; its advance uses our own compatible spacing.
# The remaining punctuation advances are measured like the rest of the table.
FIVE_ADVANCES.update({'@': 980, '[': 420, '\\': 840, ']': 420, '^': 560,
                      '_': 840, '`': 420, '{': 560, '|': 280, '}': 560, '~': 700})
ADVANCES = {'ten': TEN_ADVANCES, 'seven': SEVEN_ADVANCES,
            'five': FIVE_ADVANCES, 'five-bold': FIVE_BOLD_ADVANCES}


def advance_units(face, ch, upem):
    """Return the declared ASCII advance rounded to font units, or None."""
    if face != 'seven' and ch in string.ascii_lowercase:
        ch = ch.upper()
    value = ADVANCES[face].get(ch)
    return None if value is None else round(value * upem / 1000)
