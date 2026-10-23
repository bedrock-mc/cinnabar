"""Adapt the translator to Android networking and bound its optional code cache before writes."""
from pathlib import Path
import sys

# The writer's folder limit, which the patch reads before writing instead of after.
LIMIT = '''    int dynacache_limit = box64env.dynacache_limit;
    if(mapping->env && mapping->env->is_dynacache_limit_overridden)
        dynacache_limit = mapping->env->dynacache_limit;
'''
# Admits a file only when the folder stays within its limit and that much space stays free.
# Uncompressed, the file holds exactly the header and blocks.
CACHE_BUDGET = LIMIT + '''    uint64_t cache_bytes = total;
    int cache_fits = BOX64ENV(dynacache_compress)==COMP_NONE && dynacache_limit>0;
    for(int i=0; i<nblocks && cache_fits; ++i)
        cache_fits = cinnabar_cache_add(&cache_bytes, blocks[i].block.size);
    if(!cache_fits || !cinnabar_cache_admits(folder, cache_bytes, (uint64_t)dynacache_limit*1024*1024)) {
        dynarec_log(LOG_INFO, "DynaCache will not serialize cache for %s because it does not fit the cache budget\\n", mapping->fullname);
        box_free(all_header);
        return;
    }
'''
TEMPORARY = '''    // all done, now just create the file and write all this down...
    #ifndef WIN32
'''
PUBLISH = '''
    if(rename(tmpname, mapname)) {'''


def replace_once(path: Path, replacements: list[tuple[str, str]]) -> None:
    """Patch only known source signatures, accepting an already patched file."""
    text = path.read_text()
    for old, new in replacements:
        if new in text:
            assert text.count(new) == 1 and old not in text
            continue
        assert text.count(old) == 1, f'translator source changed: {path.name}'
        text = text.replace(old, new)
    path.write_text(text)


def patch(root: Path) -> None:
    """Patch the pinned translator source tree at `root` in place."""
    replace_once(root / 'src/wrapped/wrappedlibc_private.h', [
        ('GOW(getifaddrs, iFp)', 'GOWM(getifaddrs, iFEp)'),
        ('GOW(freeifaddrs, vFp)', 'GOWM(freeifaddrs, vFEp)'),
        # Results keep GNU's layout, so the unchanged native freeaddrinfo releases them.
        ('GO(getaddrinfo, iFpppp)', 'GOM(getaddrinfo, iFEpppp)'),
    ])
    replace_once(root / 'src/tools/dynacache.c', [
        ('#include "dynablock.h"\n#include "env_private.h"\n',
         '#include "dynablock.h"\n#include "cache_budget.h"\n#include "env_private.h"\n'),
        (LIMIT + PUBLISH, '    // The cache folder limit was read before writing.' + PUBLISH),
        (TEMPORARY + '    char tmpname[', TEMPORARY + CACHE_BUDGET + '    char tmpname['),
    ])


if __name__ == '__main__':
    patch(Path(sys.argv[1]))
