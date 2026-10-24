#pragma once

#include <netdb.h>

/// Android's resolver command socket, served by netd before Android 10 and by the DnsResolver
/// module since; applications with network access may connect to it without extra privileges.
#define CINNABAR_DNS_PROXY_SOCKET "/dev/socket/dnsproxyd"

/// Resolves like GNU `getaddrinfo`, sending host names to the Android resolver at `socket_path`.
///
/// Absent, `*` and numeric hosts use native parsing. IDN requests and requests made while the
/// Android socket is unavailable retain the native resolver fallback.
/// Parameter errors are reported exactly as the native resolver reports them. Resolver failures
/// return the matching GNU `EAI_*` code, with timeouts as `EAI_AGAIN`. Results use GNU flag values
/// and ownership, so `freeaddrinfo` releases every list this returns.
extern "C" int cinnabar_proxy_getaddrinfo(const char* socket_path, const char* node,
                                          const char* service, const addrinfo* hints,
                                          addrinfo** result);
