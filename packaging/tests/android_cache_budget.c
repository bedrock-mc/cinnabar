// Exercises the translator's code cache write budget against real files in a temporary folder.
#include "cache_budget.h"

#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>

#define CHECK(condition)                                                  \
    do {                                                                  \
        if (!(condition)) {                                               \
            fprintf(stderr, "%s:%d: %s\n", __FILE__, __LINE__, #condition); \
            exit(1);                                                      \
        }                                                                 \
    } while (0)

static const char* folder;

/// Creates or resizes `name` in the test folder as a sparse file of `bytes` bytes.
static void put(const char* name, off_t bytes) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/%s", folder, name);
    int fd = open(path, O_WRONLY | O_CREAT | O_CLOEXEC, 0600);
    CHECK(fd >= 0 && ftruncate(fd, bytes) == 0 && close(fd) == 0);
}

/// Removes `name` from the test folder.
static void drop(const char* name) {
    char path[4096];
    snprintf(path, sizeof(path), "%s/%s", folder, name);
    CHECK(unlink(path) == 0);
}

/// Checks a write of `bytes` against `limit` with `available` bytes free in the test folder.
static int fits(uint64_t bytes, uint64_t limit, uint64_t available) {
    DIR* dir = opendir(folder);
    CHECK(dir);
    int result = cinnabar_cache_fits(dir, bytes, limit, available);
    // The stream is reread from its start, so a second check agrees.
    CHECK(cinnabar_cache_fits(dir, bytes, limit, available) == result);
    CHECK(closedir(dir) == 0);
    return result;
}

int main(int argc, char** argv) {
    CHECK(argc == 2);
    folder = argv[1];
    const uint64_t limit = 1000;
    const uint64_t roomy = 1 << 20;

    uint64_t total = 7;
    CHECK(cinnabar_cache_add(&total, 5) && total == 12);
    CHECK(!cinnabar_cache_add(&total, UINT64_MAX - 11) && total == 12);
    CHECK(cinnabar_cache_add(&total, UINT64_MAX - 12) && total == UINT64_MAX);

    // An empty folder takes a full generation only while the limit stays free afterwards.
    CHECK(fits(limit, limit, 2 * limit));
    CHECK(!fits(limit, limit, 2 * limit - 1));
    CHECK(!fits(limit + 1, limit, roomy));
    CHECK(!fits(1, 0, roomy));
    // Free space plus the reserve never wraps around.
    CHECK(!fits(UINT64_MAX - 10, UINT64_MAX, UINT64_MAX));
    CHECK(!fits(2, UINT64_MAX, UINT64_MAX));

    // Earlier files, an older copy of the target and interrupted writes all count.
    put("libother.so-1-2.box64", 600);
    CHECK(fits(400, limit, roomy));
    CHECK(!fits(401, limit, roomy));
    put("bedrock_server-1-2.box64", 300);
    CHECK(fits(100, limit, roomy));
    CHECK(!fits(101, limit, roomy));
    put("bedrock_server-1-2.box64.tmp.10.0", 50);
    CHECK(fits(50, limit, roomy));
    CHECK(!fits(51, limit, roomy));
    drop("bedrock_server-1-2.box64.tmp.10.0");

    // Bookkeeping, unrelated names and folders do not count.
    put("sealed.json", 5000);
    put("attempts", 5000);
    put(".box64", 5000);
    put("notes.box64.txt", 5000);
    char nested[4096];
    snprintf(nested, sizeof(nested), "%s/nested.box64", folder);
    CHECK(mkdir(nested, 0700) == 0);
    CHECK(fits(100, limit, roomy));
    CHECK(!fits(101, limit, roomy));

    // Files already over the limit admit nothing, even an empty write.
    put("libother.so-1-2.box64", 2 * limit);
    CHECK(!fits(0, limit, roomy));
    put("libother.so-1-2.box64", 0);

    // The real file system: any free space admits one byte, and no file system has 2^63 bytes.
    CHECK(cinnabar_cache_admits(folder, 1, 300 + 1));  // The 300-byte server file plus one.
    CHECK(!cinnabar_cache_admits(folder, 1, UINT64_MAX / 2));
    CHECK(!cinnabar_cache_admits(nested, 1, 0));
    char missing[4096];
    snprintf(missing, sizeof(missing), "%s/missing", folder);
    CHECK(!cinnabar_cache_admits(missing, 1, limit));

    puts("cache budget ok");
    return 0;
}
