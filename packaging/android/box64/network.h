#pragma once

#include <stdarg.h>
#include <stdio.h>
#include <sys/ioctl.h>
#include <unistd.h>

/// Owns the netlink socket used by the imported interface enumeration code.
class ScopedFd {
    int fd_;
public:
    explicit ScopedFd(int fd = -1) : fd_(fd) {}
    ~ScopedFd() { if (fd_ != -1) close(fd_); }
    ScopedFd(const ScopedFd&) = delete;
    ScopedFd& operator=(const ScopedFd&) = delete;
    int get() const { return fd_; }
    void reset(int fd) { if (fd_ != -1) close(fd_); fd_ = fd; }
};

#define ANDROID_LOG_ERROR 6
/// Reports interface lookup errors through the server's captured stderr.
static inline void async_safe_format_log(int, const char*, const char* format, ...) {
    va_list args;
    va_start(args, format);
    vfprintf(stderr, format, args);
    va_end(args);
    fputc('\n', stderr);
}
