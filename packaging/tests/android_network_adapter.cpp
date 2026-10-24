/// Exercises routed loopback admission and ownership with fixed kernel-interface lists.
#include <arpa/inet.h>
#include <assert.h>
#include <ifaddrs.h>
#include <net/if.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

extern "C" int my_getifaddrs(void*, ifaddrs**);
extern "C" void my_freeifaddrs(void*, ifaddrs*);

namespace {
struct FakeInterface { const char* name; const char* ipv4; unsigned flags; };
struct FakeEntry { ifaddrs entry; sockaddr_in address; sockaddr_in netmask; char name[IFNAMSIZ]; };
const FakeInterface* fake_interfaces;
int fake_count;
int released;
}

/// Stands in for the imported AOSP enumeration with a fixed interface list.
int getifaddrs(ifaddrs** out) {
    *out = nullptr;
    ifaddrs** link = out;
    for (int index = 0; index < fake_count; ++index) {
        auto* fake = static_cast<FakeEntry*>(calloc(1, sizeof(FakeEntry)));
        snprintf(fake->name, sizeof(fake->name), "%s", fake_interfaces[index].name);
        fake->address.sin_family = AF_INET;
        inet_pton(AF_INET, fake_interfaces[index].ipv4, &fake->address.sin_addr);
        fake->netmask.sin_family = AF_INET;
        fake->netmask.sin_addr.s_addr = htonl(0xff000000u);
        fake->entry.ifa_name = fake->name;
        fake->entry.ifa_flags = fake_interfaces[index].flags;
        fake->entry.ifa_addr = reinterpret_cast<sockaddr*>(&fake->address);
        fake->entry.ifa_netmask = reinterpret_cast<sockaddr*>(&fake->netmask);
        *link = &fake->entry;
        link = &fake->entry.ifa_next;
    }
    return 0;
}

/// Releases only entries the fake enumeration allocated, as AOSP's freeifaddrs does.
void freeifaddrs(ifaddrs* list) {
    while (list) {
        ifaddrs* next = list->ifa_next;
        assert(strcmp(list->ifa_name, "cinnabar-lo") != 0);
        free(list);
        ++released;
        list = next;
    }
}

namespace {
/// Counts entries a WebRTC host allocator would accept as a non-loopback 127.0.0.1 network.
int routed_loopbacks(const ifaddrs* list) {
    int count = 0;
    for (; list; list = list->ifa_next) {
        const auto* address = reinterpret_cast<const sockaddr_in*>(list->ifa_addr);
        if (address && address->sin_family == AF_INET && address->sin_addr.s_addr == htonl(INADDR_LOOPBACK)
            && (list->ifa_flags & (IFF_UP | IFF_RUNNING)) == (IFF_UP | IFF_RUNNING)
            && !(list->ifa_flags & IFF_LOOPBACK) && list->ifa_netmask && strncmp(list->ifa_name, "lo", 2) != 0)
            ++count;
    }
    return count;
}

/// Enumerates `interfaces`, checks the routed entry count and original order, then releases.
void check(const FakeInterface* interfaces, int count, int expected_routed) {
    fake_interfaces = interfaces;
    fake_count = count;
    released = 0;
    ifaddrs* list = nullptr;
    assert(my_getifaddrs(nullptr, &list) == 0);
    assert(routed_loopbacks(list) == expected_routed);
    const ifaddrs* item = list;
    for (int index = 0; index < count; ++index, item = item->ifa_next)
        assert(item && strcmp(item->ifa_name, interfaces[index].name) == 0 && item->ifa_flags == interfaces[index].flags);
    my_freeifaddrs(nullptr, list);
    assert(released == count);
}
}

int main() {
    const unsigned running = IFF_UP | IFF_RUNNING;
    const FakeInterface offline[] = {{"lo", "127.0.0.1", running | IFF_LOOPBACK}};
    check(offline, 1, 1);
    const FakeInterface online[] = {{"lo", "127.0.0.1", running | IFF_LOOPBACK}, {"wlan0", "10.0.2.16", running | IFF_BROADCAST}};
    check(online, 2, 1);
    const FakeInterface loopback_down[] = {{"lo", "127.0.0.1", IFF_LOOPBACK}};
    check(loopback_down, 1, 0);
    const FakeInterface other_loopback[] = {{"lo", "127.0.0.2", running | IFF_LOOPBACK}};
    check(other_loopback, 1, 0);
    check(nullptr, 0, 0);
    puts("routed loopback adapter ok");
}
