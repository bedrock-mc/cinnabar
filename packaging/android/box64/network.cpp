#include <ifaddrs.h>
#include <net/if.h>
#include <netinet/in.h>
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

namespace {

/// Names the routed loopback entry; its address identifies entries this adapter allocated.
const char kRoutedLoopbackName[] = "cinnabar-lo";

/// One routed loopback entry with the addresses it points to, released as one allocation.
struct RoutedLoopback {
    ifaddrs entry;
    sockaddr_in address;
    sockaddr_in netmask;
};

/// Reports whether `item` is the kernel's running IPv4 127.0.0.1 loopback address.
bool is_running_loopback4(const ifaddrs* item) {
    const unsigned required = IFF_UP | IFF_RUNNING | IFF_LOOPBACK;
    if (!item->ifa_addr || item->ifa_addr->sa_family != AF_INET) return false;
    const auto* address = reinterpret_cast<const sockaddr_in*>(item->ifa_addr);
    return (item->ifa_flags & required) == required && address->sin_addr.s_addr == htonl(INADDR_LOOPBACK);
}

/// Appends 127.0.0.1 as an ordinary running interface when the kernel's loopback is running.
///
/// WebRTC port allocators skip loopback-typed networks by default, so with every other interface
/// down the server gathers no candidate even though the same-device client reaches 127.0.0.1. The
/// kernel entries are left unchanged; this extra entry only names a route that already exists.
void append_routed_loopback(ifaddrs* list) {
    bool loopback = false;
    ifaddrs* tail = nullptr;
    for (ifaddrs* item = list; item; item = item->ifa_next) {
        loopback |= is_running_loopback4(item);
        tail = item;
    }
    if (!loopback) return;
    auto* routed = static_cast<RoutedLoopback*>(calloc(1, sizeof(RoutedLoopback)));
    if (!routed) return;
    routed->address.sin_family = AF_INET;
    routed->address.sin_addr.s_addr = htonl(INADDR_LOOPBACK);
    routed->netmask.sin_family = AF_INET;
    routed->netmask.sin_addr.s_addr = 0xffffffffu;
    routed->entry.ifa_name = const_cast<char*>(kRoutedLoopbackName);
    routed->entry.ifa_flags = IFF_UP | IFF_RUNNING;
    routed->entry.ifa_addr = reinterpret_cast<sockaddr*>(&routed->address);
    routed->entry.ifa_netmask = reinterpret_cast<sockaddr*>(&routed->netmask);
    tail->ifa_next = &routed->entry;
}

}  // namespace

/// Enumerates IPv4 and IPv6 addresses without Android-restricted netlink requests, plus the
/// routed loopback entry the translated server needs to accept same-device clients offline.
extern "C" __attribute__((visibility("default"))) int my_getifaddrs(void*, ifaddrs** out) {
    int result = getifaddrs(out);
    if (result == 0) append_routed_loopback(*out);
    return result;
}

/// Releases lists allocated by the matching Android interface adapter, including routed entries.
extern "C" __attribute__((visibility("default"))) void my_freeifaddrs(void*, ifaddrs* list) {
    ifaddrs** link = &list;
    while (*link) {
        ifaddrs* item = *link;
        if (item->ifa_name == kRoutedLoopbackName) {
            *link = item->ifa_next;
            free(item);
        } else {
            link = &item->ifa_next;
        }
    }
    freeifaddrs(list);
}
