#pragma once

#include <dirent.h>
#include <stdint.h>

/// Adds `bytes` to `*total`. Returns 0 and leaves `*total` unchanged when the sum overflows.
int cinnabar_cache_add(uint64_t* total, uint64_t bytes);

/// Reports whether an optional translated-code cache file of `bytes` bytes may be written to the
/// open cache folder `dir` whose file system has `available` bytes free for unprivileged writers.
///
/// It fits when every `.box64` file and interrupted `.box64.tmp.` write already in the folder,
/// including an older copy of the file being replaced, plus `bytes` stays within `limit`, and
/// writing it leaves at least `limit` bytes free. A zero `limit` or an unreadable folder admits
/// nothing. Returns 1 to admit and 0 otherwise; the directory stream is rewound and left open.
int cinnabar_cache_fits(DIR* dir, uint64_t bytes, uint64_t limit, uint64_t available);

/// Applies `cinnabar_cache_fits` to `folder` and its file system's current free space.
/// A missing folder or unknown free space admits nothing.
int cinnabar_cache_admits(const char* folder, uint64_t bytes, uint64_t limit);
