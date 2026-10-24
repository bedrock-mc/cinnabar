#include "dns.h"

#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/un.h>
#include <unistd.h>

// The resolver's command and response format is the one AOSP's DnsProxyListener serves and
// Bionic's android_getaddrinfo_proxy sends; it is unchanged from Android 8 to current AOSP.

namespace {

/// Bionic `AI_*` request flags; GNU uses different values for all but the first two.
enum BionicFlag : int {
    kBionicPassive = 0x1,
    kBionicCanonName = 0x2,
    kBionicNumericServ = 0x8,
    kBionicAddrConfig = 0x400,
};

/// Bionic `EAI_*` codes and the resolver's timeout code, as resolver failures report them.
enum BionicError : int32_t {
    kBionicAddrFamily = 1,
    kBionicAgain = 2,
    kBionicBadFlags = 3,
    kBionicFamily = 5,
    kBionicMemory = 6,
    kBionicNoData = 7,
    kBionicNoName = 8,
    kBionicService = 9,
    kBionicSockType = 10,
    kBionicBadHints = 12,
    kBionicOverflow = 14,
    kResolverTimeout = 255,
};

/// Response codes, each sent as three digits and a NUL.
const char kQueryResult[4] = "222";
const char kOperationFailed[4] = "401";

/// Lets the resolver use the caller's default network, including any VPN and private DNS.
const unsigned kNetIdUnset = 0;

/// GNU request flags the resolver supports directly or this adapter emulates.
const int kProxiedFlags = AI_PASSIVE | AI_CANONNAME | AI_NUMERICSERV | AI_ADDRCONFIG | AI_V4MAPPED | AI_ALL;

/// Set once the unreachable resolver socket has been reported.
int unreachable_reported = 0;

/// One result in the layout GNU `freeaddrinfo` releases: the entry and its address share one
/// allocation, and a canonical name is allocated separately.
struct Entry {
    addrinfo info;
    sockaddr_storage address;
};

/// Converts a resolver failure to GNU's code. The resolver does not send errno, so its system
/// error and codes without a GNU equivalent become non-recoverable failures.
int gnu_error(int32_t code) {
    switch (code) {
    case kBionicAddrFamily: return EAI_ADDRFAMILY;
    case kBionicAgain:
    case kResolverTimeout: return EAI_AGAIN;
    case kBionicBadFlags:
    case kBionicBadHints: return EAI_BADFLAGS;
    case kBionicFamily: return EAI_FAMILY;
    case kBionicMemory: return EAI_MEMORY;
    case kBionicNoData: return EAI_NODATA;
    case kBionicNoName: return EAI_NONAME;
    case kBionicService: return EAI_SERVICE;
    case kBionicSockType: return EAI_SOCKTYPE;
    case kBionicOverflow: return EAI_OVERFLOW;
    default: return EAI_FAIL;
    }
}

/// Converts the GNU request flags the resolver understands to Bionic's values.
int bionic_flags(int flags) {
    int converted = 0;
    if (flags & AI_PASSIVE) converted |= kBionicPassive;
    if (flags & AI_CANONNAME) converted |= kBionicCanonName;
    if (flags & AI_NUMERICSERV) converted |= kBionicNumericServ;
    if (flags & AI_ADDRCONFIG) converted |= kBionicAddrConfig;
    return converted;
}

/// Reports a response that breaks the resolver protocol and returns the failure for the caller.
int malformed(const char* detail) {
    fprintf(stderr, "cinnabar: malformed Android resolver response: %s\n", detail);
    return EAI_FAIL;
}

/// Reads exactly `size` bytes. Returns false with errno set on failure, using ECONNRESET when the
/// resolver closes the connection before the response is complete.
bool read_exact(int fd, void* data, size_t size) {
    auto* bytes = static_cast<char*>(data);
    while (size > 0) {
        const ssize_t count = recv(fd, bytes, size, 0);
        if (count < 0 && errno == EINTR) continue;
        if (count < 0) return false;
        if (count == 0) {
            errno = ECONNRESET;
            return false;
        }
        bytes += count;
        size -= static_cast<size_t>(count);
    }
    return true;
}

/// Reads one big-endian 32-bit protocol field.
bool read_be32(int fd, int32_t* value) {
    uint32_t raw;
    if (!read_exact(fd, &raw, sizeof(raw))) return false;
    *value = static_cast<int32_t>(ntohl(raw));
    return true;
}

/// Writes all `size` bytes, without raising SIGPIPE if the resolver has gone away.
bool write_all(int fd, const char* data, size_t size) {
    while (size > 0) {
        const ssize_t count = send(fd, data, size, MSG_NOSIGNAL);
        if (count < 0 && errno == EINTR) continue;
        if (count <= 0) {
            if (count == 0) errno = EIO;
            return false;
        }
        data += count;
        size -= static_cast<size_t>(count);
    }
    return true;
}

/// Connects to the resolver socket, returning -1 with errno set when it cannot be reached.
int connect_resolver(const char* path) {
    sockaddr_un address = {};
    address.sun_family = AF_UNIX;
    const size_t length = strlen(path);
    if (length >= sizeof(address.sun_path)) {
        errno = ENAMETOOLONG;
        return -1;
    }
    memcpy(address.sun_path, path, length + 1);
    const int fd = socket(AF_UNIX, SOCK_STREAM | SOCK_CLOEXEC, 0);
    if (fd < 0) return -1;
    int result;
    do {
        result = connect(fd, reinterpret_cast<const sockaddr*>(&address), sizeof(address));
    } while (result != 0 && errno == EINTR);
    if (result != 0) {
        const int error = errno;
        close(fd);
        errno = error;
        return -1;
    }
    return fd;
}

/// Reports whether `text` stays a single resolver command argument. Like Bionic's client this
/// refuses separators, quotes and the `^` null marker; `\` would start an escape sequence.
bool is_plain_argument(const char* text) {
    return strcspn(text, " \t\n\r^'\"\\") == strlen(text);
}

/// Reports whether an entry carries a complete IPv4 or IPv6 address of its stated family.
bool is_usable(const addrinfo* item) {
    socklen_t expected = 0;
    if (item->ai_family == AF_INET) expected = sizeof(sockaddr_in);
    if (item->ai_family == AF_INET6) expected = sizeof(sockaddr_in6);
    return expected != 0 && item->ai_addrlen == expected && item->ai_addr->sa_family == item->ai_family;
}

/// Reads result entries into `list`, giving each the caller's GNU flags as GNU does. Returns 0 or
/// a GNU error; entries without a usable address are released and counted in `skipped`.
int read_entries(int fd, int flags, addrinfo** list, unsigned* skipped) {
    addrinfo** tail = list;
    for (;;) {
        int32_t more;
        if (!read_be32(fd, &more)) return EAI_SYSTEM;
        if (more == 0) return 0;
        auto* entry = static_cast<Entry*>(calloc(1, sizeof(Entry)));
        if (!entry) return EAI_MEMORY;
        addrinfo* item = &entry->info;
        item->ai_flags = flags;
        item->ai_addr = reinterpret_cast<sockaddr*>(&entry->address);
        // Linked before it is filled, so every failure below releases it with the list.
        *tail = item;
        int32_t resolver_flags, length, name_length;
        if (!read_be32(fd, &resolver_flags) || !read_be32(fd, &item->ai_family) ||
            !read_be32(fd, &item->ai_socktype) || !read_be32(fd, &item->ai_protocol) ||
            !read_be32(fd, &length)) {
            return EAI_SYSTEM;
        }
        if (length < 0 || static_cast<size_t>(length) > sizeof(entry->address)) return malformed("address length");
        if (!read_exact(fd, &entry->address, static_cast<size_t>(length))) return EAI_SYSTEM;
        item->ai_addrlen = static_cast<socklen_t>(length);
        if (!read_be32(fd, &name_length)) return EAI_SYSTEM;
        if (name_length < 0) return malformed("name length");
        if (name_length > 0) {
            item->ai_canonname = static_cast<char*>(malloc(static_cast<size_t>(name_length)));
            if (!item->ai_canonname) return EAI_MEMORY;
            if (!read_exact(fd, item->ai_canonname, static_cast<size_t>(name_length))) return EAI_SYSTEM;
            if (item->ai_canonname[name_length - 1] != '\0') return malformed("unterminated name");
        }
        if (is_usable(item)) {
            tail = &item->ai_next;
        } else {
            *tail = nullptr;
            ++*skipped;
            free(item->ai_canonname);
            free(entry);
        }
    }
}

/// Sends one prepared command and reads its answer into `list`. Returns 0 or a GNU error, with
/// errno set for `EAI_SYSTEM`.
int query_resolver(int fd, const char* command, size_t size, int flags, addrinfo** list) {
    if (!write_all(fd, command, size)) return EAI_SYSTEM;
    char code[4];
    if (!read_exact(fd, code, sizeof(code))) return EAI_SYSTEM;
    if (memcmp(code, kOperationFailed, sizeof(code)) == 0) {
        int32_t length, error;
        if (!read_be32(fd, &length)) return EAI_SYSTEM;
        if (length != static_cast<int32_t>(sizeof(error))) return malformed("failure length");
        // The resolver runs on this device and sends its code in native byte order.
        if (!read_exact(fd, &error, sizeof(error))) return EAI_SYSTEM;
        return gnu_error(error);
    }
    if (memcmp(code, kQueryResult, sizeof(code)) != 0) {
        // Rejected commands get a text reply such as "500 Command too long".
        fprintf(stderr, "cinnabar: Android resolver rejected a lookup with code %.3s\n", code);
        return EAI_FAIL;
    }
    unsigned skipped = 0;
    const int status = read_entries(fd, flags, list, &skipped);
    if (skipped) fprintf(stderr, "cinnabar: skipped %u Android resolver results without a usable address\n", skipped);
    if (status == 0 && !*list) return skipped ? EAI_FAIL : EAI_NODATA;
    return status;
}

/// Rewrites an IPv4 result as the IPv4-mapped IPv6 address GNU returns for `AI_V4MAPPED`.
void map_to_ipv6(addrinfo* item) {
    const sockaddr_in ipv4 = *reinterpret_cast<const sockaddr_in*>(item->ai_addr);
    sockaddr_in6 ipv6 = {};
    ipv6.sin6_family = AF_INET6;
    ipv6.sin6_port = ipv4.sin_port;
    ipv6.sin6_addr.s6_addr[10] = 0xff;
    ipv6.sin6_addr.s6_addr[11] = 0xff;
    memcpy(&ipv6.sin6_addr.s6_addr[12], &ipv4.sin_addr, sizeof(ipv4.sin_addr));
    memcpy(item->ai_addr, &ipv6, sizeof(ipv6));
    item->ai_family = AF_INET6;
    item->ai_addrlen = sizeof(ipv6);
}

/// Applies GNU `AI_V4MAPPED` to an IPv6 request answered for both families: IPv4 results become
/// IPv4-mapped addresses, or are dropped when IPv6 results exist and `all` is false. A dropped
/// canonical name moves to the new first entry.
void apply_v4mapped(addrinfo** list, bool all) {
    bool ipv6 = false;
    for (const addrinfo* item = *list; item; item = item->ai_next) ipv6 |= item->ai_family == AF_INET6;
    char* canonical = nullptr;
    for (addrinfo** link = list; *link;) {
        addrinfo* item = *link;
        if (item->ai_family != AF_INET) {
            link = &item->ai_next;
        } else if (!ipv6 || all) {
            map_to_ipv6(item);
            link = &item->ai_next;
        } else {
            *link = item->ai_next;
            if (canonical) free(item->ai_canonname);
            else canonical = item->ai_canonname;
            free(item);
        }
    }
    if (canonical && *list && !(*list)->ai_canonname) {
        (*list)->ai_canonname = canonical;
    } else {
        free(canonical);
    }
}

}  // namespace

extern "C" int cinnabar_proxy_getaddrinfo(const char* socket_path, const char* node,
                                          const char* service, const addrinfo* hints,
                                          addrinfo** result) {
    addrinfo request = {};
    request.ai_flags = AI_V4MAPPED | AI_ADDRCONFIG;  // GNU's request without hints
    if (hints) request = *hints;
    if (!node || strcmp(node, "*") == 0 || (request.ai_flags & ~kProxiedFlags) != 0) {
        return getaddrinfo(node, service, hints, result);
    }

    // The native resolver validates the request and parses numeric hosts without a name server.
    // A single socket type keeps it from sorting by route; the full request is repeated below.
    addrinfo probe = request;
    probe.ai_flags = (request.ai_flags & ~AI_ADDRCONFIG) | AI_NUMERICHOST;
    if (probe.ai_socktype == 0 && probe.ai_protocol == 0) probe.ai_socktype = SOCK_DGRAM;
    addrinfo* numeric = nullptr;
    const int classified = getaddrinfo(node, nullptr, &probe, &numeric);
    if (classified == 0) {
        freeaddrinfo(numeric);
        return getaddrinfo(node, service, hints, result);
    }
    if (classified != EAI_NONAME) return classified;

    if (!node[0] || !is_plain_argument(node)) return EAI_NONAME;
    if (service && !service[0]) service = nullptr;
    if (service && !is_plain_argument(service)) return EAI_SERVICE;
    // GNU answers an IPv6 AI_V4MAPPED request from both families, so ask the resolver for both.
    const bool mapped = request.ai_family == AF_INET6 && (request.ai_flags & AI_V4MAPPED);

    const int fd = connect_resolver(socket_path);
    if (fd < 0) {
        if (!__atomic_exchange_n(&unreachable_reported, 1, __ATOMIC_RELAXED)) {
            fprintf(stderr, "cinnabar: Android resolver unavailable (%s); using the native resolver\n",
                    strerror(errno));
        }
        return getaddrinfo(node, service, hints, result);
    }
    char* command = nullptr;
    const int length = asprintf(&command, "getaddrinfo %s %s %d %d %d %d %u", node,
                                service ? service : "^", bionic_flags(request.ai_flags),
                                mapped ? AF_UNSPEC : request.ai_family, request.ai_socktype,
                                request.ai_protocol, kNetIdUnset);
    if (length < 0) {
        close(fd);
        return EAI_MEMORY;
    }
    addrinfo* list = nullptr;
    // The command's NUL terminator ends it for the resolver.
    const int status = query_resolver(fd, command, static_cast<size_t>(length) + 1, request.ai_flags, &list);
    const int error = errno;
    free(command);
    close(fd);
    if (status != 0) {
        freeaddrinfo(list);
        errno = error;
        return status;
    }
    if (mapped) apply_v4mapped(&list, (request.ai_flags & AI_ALL) != 0);
    *result = list;
    return 0;
}

/// Replaces the guest's `getaddrinfo`. The bundled GNU resolver reads its configuration from a
/// prefix this application cannot access, so it would query a name server at 127.0.0.1.
extern "C" __attribute__((visibility("default"))) int my_getaddrinfo(void*, const char* node,
                                                                      const char* service,
                                                                      const addrinfo* hints,
                                                                      addrinfo** result) {
    return cinnabar_proxy_getaddrinfo(CINNABAR_DNS_PROXY_SOCKET, node, service, hints, result);
}
