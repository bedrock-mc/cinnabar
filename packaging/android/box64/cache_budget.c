#include "cache_budget.h"

#include <errno.h>
#include <fcntl.h>
#include <string.h>
#include <sys/stat.h>
#include <sys/statvfs.h>

/// Suffix of the translator's published cache files.
static const char kPublished[] = ".box64";
/// Marks the translator's temporary writes.
static const char kPartial[] = ".box64.tmp.";

int cinnabar_cache_add(uint64_t* total, uint64_t bytes) {
    uint64_t sum;
    if (__builtin_add_overflow(*total, bytes, &sum)) return 0;
    *total = sum;
    return 1;
}

/// Reports whether `name` is a published or temporary translator cache file.
static int is_cache_file(const char* name) {
    size_t length = strlen(name);
    size_t suffix = sizeof(kPublished) - 1;
    return strstr(name, kPartial) || (length > suffix && !strcmp(name + length - suffix, kPublished));
}

int cinnabar_cache_fits(DIR* dir, uint64_t bytes, uint64_t limit, uint64_t available) {
    uint64_t needed = bytes;
    uint64_t total = bytes;
    if (!limit || total > limit || !cinnabar_cache_add(&needed, limit) || available < needed) return 0;
    int fd = dirfd(dir);
    if (fd < 0) return 0;
    rewinddir(dir);
    for (;;) {
        errno = 0;
        struct dirent* entry = readdir(dir);
        if (!entry) return errno ? 0 : 1;
        if (!is_cache_file(entry->d_name)) continue;
        struct stat status;
        int result;
        do {
            result = fstatat(fd, entry->d_name, &status, AT_SYMLINK_NOFOLLOW);
        } while (result && errno == EINTR);
        if (result && errno == ENOENT) continue;
        if (result || status.st_size < 0) return 0;
        if (S_ISREG(status.st_mode) && (!cinnabar_cache_add(&total, (uint64_t)status.st_size) || total > limit))
            return 0;
    }
}

int cinnabar_cache_admits(const char* folder, uint64_t bytes, uint64_t limit) {
    int saved = errno;
    DIR* dir;
    do {
        dir = opendir(folder);
    } while (!dir && errno == EINTR);
    int admitted = 0;
    if (dir) {
        struct statvfs stats;
        int result;
        do {
            result = fstatvfs(dirfd(dir), &stats);
        } while (result && errno == EINTR);
        uint64_t available;
        admitted = !result
            && !__builtin_mul_overflow((uint64_t)stats.f_bavail, (uint64_t)stats.f_frsize, &available)
            && cinnabar_cache_fits(dir, bytes, limit, available);
        closedir(dir);
    }
    errno = saved;
    return admitted;
}
