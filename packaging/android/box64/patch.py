"""Route interface enumeration and name resolution through the Android networking adapters."""
from pathlib import Path
import sys


def patch(root: Path) -> None:
    """Patch only known wrapper signatures, accepting an already patched tree."""
    header = root / 'src/wrapped/wrappedlibc_private.h'
    text = header.read_text()
    for old, new in [
        ('GOW(getifaddrs, iFp)', 'GOWM(getifaddrs, iFEp)'),
        ('GOW(freeifaddrs, vFp)', 'GOWM(freeifaddrs, vFEp)'),
        # Results keep GNU's layout, so the unchanged native freeaddrinfo releases them.
        ('GO(getaddrinfo, iFpppp)', 'GOM(getaddrinfo, iFEpppp)'),
    ]:
        if new in text:
            assert text.count(new) == 1 and old not in text
            continue
        assert text.count(old) == 1, 'translator interface wrapper changed'
        text = text.replace(old, new)
    header.write_text(text)


if __name__ == '__main__':
    patch(Path(sys.argv[1]))
