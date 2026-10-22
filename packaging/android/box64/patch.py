"""Route interface allocation and release through the Android networking adapter."""
from pathlib import Path
import sys


def patch(root: Path) -> None:
    """Patch only known wrapper signatures, accepting an already patched tree."""
    header = root / 'src/wrapped/wrappedlibc_private.h'
    text = header.read_text()
    for old, new in [
        ('GOW(getifaddrs, iFp)', 'GOWM(getifaddrs, iFEp)'),
        ('GOW(freeifaddrs, vFp)', 'GOWM(freeifaddrs, vFEp)'),
    ]:
        if new in text:
            assert text.count(new) == 1 and old not in text
            continue
        assert text.count(old) == 1, 'translator interface wrapper changed'
        text = text.replace(old, new)
    header.write_text(text)


if __name__ == '__main__':
    patch(Path(sys.argv[1]))
