/*
 * Atomically renames one directory while refusing to replace or nest into an
 * existing destination. fetch-vanilla-assets.sh compiles this tiny helper with
 * the C toolchain already required to link Cinnabar's Rust binaries. Plain mv
 * is unsuitable because an existing directory changes its meaning to nesting.
 */

#define _GNU_SOURCE
#include <errno.h>
#include <stdio.h>
#include <string.h>

#if defined(__APPLE__)
#ifndef RENAME_EXCL
#define RENAME_EXCL 0x00000004
#endif
#elif defined(__linux__)
#include <fcntl.h>
#include <linux/fs.h>
#include <sys/syscall.h>
#include <unistd.h>
#elif defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>

static DWORD windows_rename_error;
#else
#error "atomic no-replace directory publication is unsupported on this platform"
#endif

/* Performs the platform's direct atomic no-replace rename operation. */
static int rename_directory_no_replace(const char *source, const char *destination) {
#if defined(__APPLE__)
    return renamex_np(source, destination, RENAME_EXCL);
#elif defined(__linux__)
    return (int)syscall(
        SYS_renameat2,
        AT_FDCWD,
        source,
        AT_FDCWD,
        destination,
        RENAME_NOREPLACE
    );
#else
    if (MoveFileExA(source, destination, 0) != 0) {
        return 0;
    }
    windows_rename_error = GetLastError();
    if (windows_rename_error == ERROR_ALREADY_EXISTS ||
        windows_rename_error == ERROR_FILE_EXISTS) {
        errno = EEXIST;
    } else {
        errno = EIO;
    }
    return -1;
#endif
}

int main(int argc, char **argv) {
    if (argc != 3) {
        fprintf(stderr, "usage: %s SOURCE_DIRECTORY DESTINATION_DIRECTORY\n", argv[0]);
        return 64;
    }
    if (rename_directory_no_replace(argv[1], argv[2]) == 0) {
        return 0;
    }
    if (errno == EEXIST || errno == ENOTEMPTY) {
        fprintf(stderr, "cache directory appeared during extraction: %s\n", argv[2]);
        return 3;
    }
#if defined(_WIN32)
    fprintf(
        stderr,
        "atomic no-replace directory rename failed (%s -> %s): Windows error %lu\n",
        argv[1],
        argv[2],
        (unsigned long)windows_rename_error
    );
#else
    fprintf(
        stderr,
        "atomic no-replace directory rename failed (%s -> %s): %s\n",
        argv[1],
        argv[2],
        strerror(errno)
    );
#endif
    return 4;
}
