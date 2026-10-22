#include <ifaddrs.h>
#include <stdlib.h>

/// Allocates storage for the imported implementation without a C++ runtime dependency.
void* operator new(size_t size) {
    void* result = malloc(size);
    if (!result) abort();
    return result;
}
void* operator new[](size_t size) { return operator new(size); }
void operator delete(void* pointer) noexcept { free(pointer); }
void operator delete[](void* pointer) noexcept { free(pointer); }
void operator delete(void* pointer, size_t) noexcept { free(pointer); }
void operator delete[](void* pointer, size_t) noexcept { free(pointer); }

/// Enumerates IPv4 and IPv6 addresses without Android-restricted netlink requests.
extern "C" __attribute__((visibility("default"))) int my_getifaddrs(void*, ifaddrs** out) {
    return getifaddrs(out);
}

/// Releases lists allocated by the matching Android interface adapter.
extern "C" __attribute__((visibility("default"))) void my_freeifaddrs(void*, ifaddrs* list) {
    freeifaddrs(list);
}
