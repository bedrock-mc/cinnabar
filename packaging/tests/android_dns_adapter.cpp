/// Exercises the Android resolver adapter against a scripted resolver on a temporary socket.
#include "dns.h"

#include <arpa/inet.h>
#include <errno.h>
#include <netinet/in.h>
#include <poll.h>
#include <pthread.h>
#include <sched.h>
#include <signal.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/socket.h>
#include <sys/syscall.h>
#include <sys/un.h>
#include <unistd.h>

#include <string>
#include <thread>

#define EXPECT(condition)                                                                    \
    do {                                                                                     \
        if (!(condition)) {                                                                  \
            fprintf(stderr, "%s:%d: case \"%s\": expected %s\n", __FILE__, __LINE__,        \
                    current_case, #condition);                                               \
            abort();                                                                         \
        }                                                                                    \
    } while (0)

namespace {

const char* current_case = "setup";

/// Marks a result pointer the adapter must leave alone when a lookup fails.
addrinfo* const kUntouched = reinterpret_cast<addrinfo*>(uintptr_t{0x5a5a});

/// Builds resolver reply bytes in the framing Android's DnsProxyListener sends.
struct Reply {
    std::string bytes;

    /// Appends a three-digit response code and its NUL.
    Reply& code(const char* digits) {
        bytes.append(digits, 4);
        return *this;
    }
    /// Appends a big-endian 32-bit field.
    Reply& be32(int32_t value) {
        const uint32_t raw = htonl(static_cast<uint32_t>(value));
        bytes.append(reinterpret_cast<const char*>(&raw), sizeof(raw));
        return *this;
    }
    /// Appends raw bytes.
    Reply& raw(const void* data, size_t size) {
        bytes.append(static_cast<const char*>(data), size);
        return *this;
    }
    /// Appends one result with an address of `length` bytes and an optional canonical name.
    Reply& entry(int family, int socktype, int protocol, const void* address, int32_t length,
                 const char* canonical = nullptr) {
        // The resolver's own flags field is ignored by GNU callers, so send a distinct value.
        be32(1).be32(0x7f).be32(family).be32(socktype).be32(protocol).be32(length);
        raw(address, static_cast<size_t>(length));
        if (!canonical) return be32(0);
        const size_t size = strlen(canonical) + 1;
        return be32(static_cast<int32_t>(size)).raw(canonical, size);
    }
    /// Ends a result list.
    Reply& end() { return be32(0); }
    /// Builds a failure reply carrying a resolver error code in native byte order.
    static Reply failure(int32_t error) {
        Reply reply;
        reply.code("401").be32(sizeof(error)).raw(&error, sizeof(error));
        return reply;
    }
};

/// Builds an IPv4 endpoint for a scripted resolver result.
sockaddr_in ipv4(const char* text, uint16_t port) {
    sockaddr_in address = {};
    address.sin_family = AF_INET;
    address.sin_port = htons(port);
    inet_pton(AF_INET, text, &address.sin_addr);
    return address;
}

/// Builds an IPv6 endpoint for a scripted resolver result.
sockaddr_in6 ipv6(const char* text, uint16_t port) {
    sockaddr_in6 address = {};
    address.sin6_family = AF_INET6;
    address.sin6_port = htons(port);
    inet_pton(AF_INET6, text, &address.sin6_addr);
    return address;
}

/// Plays one resolver connection: accepts it, reads the command, then sends the scripted reply.
struct Exchange {
    std::string reply;
    /// Number of reply bytes sent before closing; the whole reply by default.
    size_t reply_limit = std::string::npos;
    /// Command bytes read before replying or closing; zero reads up to and including the NUL.
    size_t read_limit = 0;
    /// Thread to interrupt with SIGUSR1 once it blocks receiving the reply, if any.
    pid_t interrupt_tid = 0;
    pthread_t interrupt_thread = {};
};

/// Fake resolver listening on a short temporary socket path.
struct Resolver {
    char directory[32] = "/tmp/cinnabar-dns-XXXXXX";
    char path[64] = {};
    int listener = -1;
    int handled_pipe[2] = {-1, -1};
    /// Carries one byte per finished lookup, so a missing connection ends the served exchange.
    int done_pipe[2] = {-1, -1};
    std::string command;
    bool connected = false;
    std::thread thread;

    /// Creates the listener and synchronization pipes without external network access.
    void open() {
        EXPECT(mkdtemp(directory) != nullptr);
        snprintf(path, sizeof(path), "%s/dnsproxyd", directory);
        sockaddr_un address = {};
        address.sun_family = AF_UNIX;
        EXPECT(strlen(path) < sizeof(address.sun_path));
        strcpy(address.sun_path, path);
        listener = socket(AF_UNIX, SOCK_STREAM | SOCK_NONBLOCK | SOCK_CLOEXEC, 0);
        EXPECT(listener >= 0);
        EXPECT(bind(listener, reinterpret_cast<sockaddr*>(&address), sizeof(address)) == 0);
        EXPECT(listen(listener, 8) == 0);
        EXPECT(pipe(handled_pipe) == 0);
        EXPECT(pipe(done_pipe) == 0);
    }

    /// Releases the listener, pipes and temporary directory after all exchanges finish.
    void close_all() {
        close(listener);
        close(handled_pipe[0]);
        close(handled_pipe[1]);
        close(done_pipe[0]);
        close(done_pipe[1]);
        unlink(path);
        rmdir(directory);
    }

    /// Serves `exchange` on the next connection from a background thread.
    void serve(const Exchange& exchange) {
        command.clear();
        connected = false;
        thread = std::thread([this, exchange] { run(exchange); });
    }

    /// Waits for the served connection to finish after the lookup returned, and checks the lookup
    /// made exactly that one connection.
    void finish() {
        const char byte = 1;
        EXPECT(write(done_pipe[1], &byte, 1) == 1);
        thread.join();
        char drained;
        EXPECT(read(done_pipe[0], &drained, 1) == 1);
        EXPECT(connected);
        expect_no_connection();
    }

    /// Checks the adapter has not connected since the last served exchange.
    void expect_no_connection() {
        const int fd = accept(listener, nullptr, nullptr);
        if (fd >= 0) close(fd);
        EXPECT(fd < 0 && (errno == EAGAIN || errno == EWOULDBLOCK));
    }

    /// Handles one queued connection or returns when the caller finishes without connecting.
    void run(const Exchange& exchange) {
        // A connection is queued before the lookup can return and report it is done.
        pollfd ready[] = {{listener, POLLIN, 0}, {done_pipe[0], POLLIN, 0}};
        EXPECT(poll(ready, 2, -1) > 0);
        if (!(ready[0].revents & POLLIN)) return;
        connected = true;
        const int fd = accept4(listener, nullptr, nullptr, SOCK_CLOEXEC);
        EXPECT(fd >= 0);
        char byte;
        while (exchange.read_limit == 0 || command.size() < exchange.read_limit) {
            const ssize_t count = recv(fd, &byte, 1, 0);
            if (count <= 0) break;
            command.push_back(byte);
            if (exchange.read_limit == 0 && byte == '\0') break;
        }
        if (exchange.interrupt_tid) interrupt(exchange);
        const size_t size = exchange.reply_limit < exchange.reply.size() ? exchange.reply_limit
                                                                         : exchange.reply.size();
        // The adapter may stop reading at a malformed field and close first.
        send(fd, exchange.reply.data(), size, MSG_NOSIGNAL);
        close(fd);
    }

    /// Signals the client once the kernel reports it blocked in `recvfrom`, then waits for the
    /// handler to run so the reply can only arrive after the interrupted call.
    void interrupt(const Exchange& exchange) {
        char state_path[64];
        snprintf(state_path, sizeof(state_path), "/proc/self/task/%d/syscall", exchange.interrupt_tid);
        for (;;) {
            FILE* state = fopen(state_path, "r");
            EXPECT(state != nullptr);
            long number = -1;
            const int parsed = fscanf(state, "%ld", &number);
            fclose(state);
            if (parsed == 1 && number == SYS_recvfrom) break;
            sched_yield();
        }
        EXPECT(pthread_kill(exchange.interrupt_thread, SIGUSR1) == 0);
        char byte;
        EXPECT(read(handled_pipe[0], &byte, 1) == 1);
    }
};

Resolver resolver;
int handled_pipe_write = -1;

/// Acknowledges signal delivery using only an async-signal-safe write.
void record_signal(int) {
    const char byte = 1;
    (void)!write(handled_pipe_write, &byte, 1);
}

/// Builds caller hints with unspecified fields left zeroed.
addrinfo hints(int flags, int family = AF_UNSPEC, int socktype = 0, int protocol = 0) {
    addrinfo value = {};
    value.ai_flags = flags;
    value.ai_family = family;
    value.ai_socktype = socktype;
    value.ai_protocol = protocol;
    return value;
}

/// Runs one proxied lookup against `exchange`, returning the adapter's status and list.
int lookup(const Exchange& exchange, const char* node, const char* service, const addrinfo* request,
           addrinfo** result) {
    *result = kUntouched;
    resolver.serve(exchange);
    const int status = cinnabar_proxy_getaddrinfo(resolver.path, node, service, request, result);
    const int error = errno;
    resolver.finish();
    errno = error;
    return status;
}

/// Runs one proxied lookup that must fail with `expected`, leaving the result untouched.
void expect_failure(const Exchange& exchange, int expected, const addrinfo* request = nullptr) {
    addrinfo* result;
    const int status = lookup(exchange, "example.test", nullptr, request, &result);
    EXPECT(status == expected);
    EXPECT(result == kUntouched);
}

/// Checks a complete scripted reply produces the expected failure.
void expect_failure(const Reply& reply, int expected) {
    Exchange exchange;
    exchange.reply = reply.bytes;
    expect_failure(exchange, expected);
}

/// Checks the command a lookup sends, answering it with no results.
void expect_command(const char* node, const char* service, const addrinfo* request,
                    const char* expected) {
    Exchange exchange;
    exchange.reply = Reply().code("222").end().bytes;
    addrinfo* result;
    EXPECT(lookup(exchange, node, service, request, &result) == EAI_NODATA);
    EXPECT(resolver.command == std::string(expected, strlen(expected) + 1));
}

/// Checks a request is handled exactly as the native resolver handles it, without connecting.
void expect_native(const char* node, const char* service, const addrinfo* request) {
    addrinfo* expected = nullptr;
    addrinfo* actual = nullptr;
    const int native = getaddrinfo(node, service, request, &expected);
    EXPECT(cinnabar_proxy_getaddrinfo(resolver.path, node, service, request, &actual) == native);
    resolver.expect_no_connection();
    if (native != 0) return;
    const addrinfo* left = expected;
    const addrinfo* right = actual;
    for (; left && right; left = left->ai_next, right = right->ai_next) {
        EXPECT(left->ai_family == right->ai_family && left->ai_socktype == right->ai_socktype);
        EXPECT(left->ai_protocol == right->ai_protocol && left->ai_addrlen == right->ai_addrlen);
        EXPECT(memcmp(left->ai_addr, right->ai_addr, left->ai_addrlen) == 0);
    }
    EXPECT(!left && !right);
    freeaddrinfo(expected);
    freeaddrinfo(actual);
}

/// Checks a request is refused with `expected` before any connection is made.
void expect_refused(const char* node, const char* service, int expected) {
    addrinfo* result = kUntouched;
    EXPECT(cinnabar_proxy_getaddrinfo(resolver.path, node, service, nullptr, &result) == expected);
    EXPECT(result == kUntouched);
    resolver.expect_no_connection();
}

/// Checks the returned endpoint's family, length and complete address bytes.
void expect_address(const addrinfo* item, const void* address, socklen_t length) {
    EXPECT(item->ai_addrlen == length);
    EXPECT(item->ai_family == reinterpret_cast<const sockaddr*>(address)->sa_family);
    EXPECT(memcmp(item->ai_addr, address, length) == 0);
}

const sockaddr_in kIpv4 = ipv4("192.0.2.1", 443);
const sockaddr_in kSecondIpv4 = ipv4("198.51.100.7", 443);
const sockaddr_in6 kIpv6 = ipv6("2001:db8::1", 443);
const sockaddr_in6 kMappedIpv4 = ipv6("::ffff:192.0.2.1", 443);
const sockaddr_in6 kMappedSecondIpv4 = ipv6("::ffff:198.51.100.7", 443);

void test_commands() {
    current_case = "command with hints";
    addrinfo request = hints(AI_CANONNAME | AI_ADDRCONFIG, AF_UNSPEC, SOCK_STREAM);
    expect_command("example.test", "443", &request, "getaddrinfo example.test 443 1026 0 1 0 0");

    current_case = "command without hints uses GNU defaults";
    expect_command("example.test", nullptr, nullptr, "getaddrinfo example.test ^ 1024 0 0 0 0");

    current_case = "passive numeric service";
    request = hints(AI_PASSIVE | AI_NUMERICSERV, AF_INET, SOCK_DGRAM, IPPROTO_UDP);
    expect_command("example.test", "53", &request, "getaddrinfo example.test 53 9 2 2 17 0");

    current_case = "empty service";
    request = hints(0, AF_INET6, SOCK_STREAM, IPPROTO_TCP);
    expect_command("example.test", "", &request, "getaddrinfo example.test ^ 0 10 1 6 0");

    current_case = "IPv6 V4MAPPED asks for both families";
    request = hints(AI_V4MAPPED | AI_ALL, AF_INET6);
    expect_command("example.test", nullptr, &request, "getaddrinfo example.test ^ 0 0 0 0 0");

    current_case = "IPv4 V4MAPPED keeps its family";
    request = hints(AI_V4MAPPED, AF_INET);
    expect_command("example.test", nullptr, &request, "getaddrinfo example.test ^ 0 2 0 0 0");
}

/// A successful two-family reply with a canonical name on its first result.
Reply success_reply() {
    return Reply()
        .code("222")
        .entry(AF_INET, SOCK_STREAM, IPPROTO_TCP, &kIpv4, sizeof(kIpv4), "example.test")
        .entry(AF_INET6, SOCK_DGRAM, IPPROTO_UDP, &kIpv6, sizeof(kIpv6))
        .end();
}

void test_success() {
    current_case = "IPv4 and IPv6 results";
    Exchange exchange;
    exchange.reply = success_reply().bytes;
    const addrinfo request = hints(AI_CANONNAME | AI_ADDRCONFIG);
    addrinfo* result;
    EXPECT(lookup(exchange, "example.test", "443", &request, &result) == 0);
    EXPECT(result && result->ai_next && !result->ai_next->ai_next);
    expect_address(result, &kIpv4, sizeof(kIpv4));
    EXPECT(result->ai_socktype == SOCK_STREAM && result->ai_protocol == IPPROTO_TCP);
    EXPECT(result->ai_flags == (AI_CANONNAME | AI_ADDRCONFIG));
    EXPECT(result->ai_canonname && strcmp(result->ai_canonname, "example.test") == 0);
    const addrinfo* second = result->ai_next;
    expect_address(second, &kIpv6, sizeof(kIpv6));
    EXPECT(second->ai_socktype == SOCK_DGRAM && second->ai_protocol == IPPROTO_UDP);
    EXPECT(second->ai_flags == (AI_CANONNAME | AI_ADDRCONFIG) && !second->ai_canonname);
    freeaddrinfo(result);

    current_case = "results carry GNU default flags without hints";
    EXPECT(lookup(exchange, "example.test", nullptr, nullptr, &result) == 0);
    for (const addrinfo* item = result; item; item = item->ai_next)
        EXPECT(item->ai_flags == (AI_V4MAPPED | AI_ADDRCONFIG));
    freeaddrinfo(result);
}

void test_resolver_errors() {
    struct Case { int32_t resolver; int gnu; };
    const Case cases[] = {
        {1, EAI_ADDRFAMILY}, {2, EAI_AGAIN},    {3, EAI_BADFLAGS}, {4, EAI_FAIL},
        {5, EAI_FAMILY},     {6, EAI_MEMORY},   {7, EAI_NODATA},   {8, EAI_NONAME},
        {9, EAI_SERVICE},    {10, EAI_SOCKTYPE}, {11, EAI_FAIL},    {12, EAI_BADFLAGS},
        {13, EAI_FAIL},      {14, EAI_OVERFLOW}, {255, EAI_AGAIN},  {0, EAI_FAIL},
        {99, EAI_FAIL},      {-1, EAI_FAIL},
    };
    current_case = "resolver error codes";
    for (const Case& item : cases) expect_failure(Reply::failure(item.resolver), item.gnu);
}

void test_truncation() {
    const std::string replies[] = {success_reply().bytes, Reply::failure(8).bytes};
    current_case = "reply truncated at every offset";
    for (const std::string& reply : replies) {
        for (size_t offset = 0; offset < reply.size(); ++offset) {
            Exchange exchange;
            exchange.reply = reply;
            exchange.reply_limit = offset;
            errno = 0;
            expect_failure(exchange, EAI_SYSTEM);
            EXPECT(errno == ECONNRESET);
        }
    }
}

void test_malformed() {
    const char unterminated[] = {'a', 'b', 'c'};
    const char oversized[sizeof(sockaddr_storage) + 1] = {};
    current_case = "address longer than sockaddr_storage";
    expect_failure(Reply().code("222").entry(AF_INET, 0, 0, oversized, sizeof(oversized)).end(), EAI_FAIL);
    current_case = "negative address length";
    expect_failure(Reply().code("222").be32(1).be32(0).be32(AF_INET).be32(0).be32(0).be32(-1), EAI_FAIL);
    current_case = "negative name length after a valid result";
    expect_failure(Reply()
                       .code("222")
                       .entry(AF_INET, 0, 0, &kIpv4, sizeof(kIpv4), "example.test")
                       .be32(1).be32(0).be32(AF_INET6).be32(0).be32(0).be32(sizeof(kIpv6))
                       .raw(&kIpv6, sizeof(kIpv6))
                       .be32(-1),
                   EAI_FAIL);
    current_case = "unterminated canonical name";
    expect_failure(Reply()
                       .code("222")
                       .be32(1).be32(0).be32(AF_INET).be32(0).be32(0).be32(sizeof(kIpv4))
                       .raw(&kIpv4, sizeof(kIpv4))
                       .be32(sizeof(unterminated)).raw(unterminated, sizeof(unterminated))
                       .end(),
                   EAI_FAIL);
    current_case = "failure payload longer than an error code";
    expect_failure(Reply().code("401").be32(8).be32(8).be32(0), EAI_FAIL);
    current_case = "rejected command";
    Reply rejected;
    rejected.bytes = std::string("500 Command too long", 21);
    expect_failure(rejected, EAI_FAIL);
}

void test_odd_results() {
    sockaddr_in6 family_mismatch = kIpv6;
    family_mismatch.sin6_family = AF_INET;
    sockaddr_un local = {};
    local.sun_family = AF_UNIX;
    current_case = "unusable results are skipped";
    Exchange exchange;
    exchange.reply = Reply()
                         .code("222")
                         .entry(AF_INET, 0, 0, &kIpv6, sizeof(kIpv6), "skipped.test")
                         .entry(AF_INET6, 0, 0, &family_mismatch, sizeof(family_mismatch))
                         .entry(AF_INET, SOCK_STREAM, 0, &kIpv4, sizeof(kIpv4))
                         .entry(AF_UNIX, 0, 0, &local, sizeof(local))
                         .entry(AF_INET6, SOCK_STREAM, 0, &kIpv6, sizeof(kIpv6))
                         .entry(AF_INET, 0, 0, &kIpv4, 0)
                         .end()
                         .bytes;
    addrinfo* result;
    EXPECT(lookup(exchange, "example.test", nullptr, nullptr, &result) == 0);
    EXPECT(result && result->ai_next && !result->ai_next->ai_next);
    expect_address(result, &kIpv4, sizeof(kIpv4));
    expect_address(result->ai_next, &kIpv6, sizeof(kIpv6));
    freeaddrinfo(result);

    current_case = "only unusable results";
    expect_failure(Reply().code("222").entry(AF_UNIX, 0, 0, &local, sizeof(local), "x").end(), EAI_FAIL);
    current_case = "no results";
    expect_failure(Reply().code("222").end(), EAI_NODATA);
}

void test_v4mapped() {
    addrinfo* result;
    Exchange exchange;

    current_case = "IPv4-only answer is mapped";
    exchange.reply = Reply()
                         .code("222")
                         .entry(AF_INET, SOCK_STREAM, 0, &kIpv4, sizeof(kIpv4), "example.test")
                         .entry(AF_INET, SOCK_STREAM, 0, &kSecondIpv4, sizeof(kSecondIpv4))
                         .end()
                         .bytes;
    addrinfo request = hints(AI_V4MAPPED | AI_CANONNAME, AF_INET6);
    EXPECT(lookup(exchange, "example.test", nullptr, &request, &result) == 0);
    expect_address(result, &kMappedIpv4, sizeof(kMappedIpv4));
    EXPECT(result->ai_canonname && strcmp(result->ai_canonname, "example.test") == 0);
    expect_address(result->ai_next, &kMappedSecondIpv4, sizeof(kMappedSecondIpv4));
    EXPECT(!result->ai_next->ai_next);
    freeaddrinfo(result);

    current_case = "IPv6 answers replace IPv4 ones";
    exchange.reply = Reply()
                         .code("222")
                         .entry(AF_INET, SOCK_STREAM, 0, &kIpv4, sizeof(kIpv4), "example.test")
                         .entry(AF_INET6, SOCK_STREAM, 0, &kIpv6, sizeof(kIpv6))
                         .entry(AF_INET, SOCK_STREAM, 0, &kSecondIpv4, sizeof(kSecondIpv4))
                         .end()
                         .bytes;
    EXPECT(lookup(exchange, "example.test", nullptr, &request, &result) == 0);
    expect_address(result, &kIpv6, sizeof(kIpv6));
    EXPECT(result->ai_canonname && strcmp(result->ai_canonname, "example.test") == 0);
    EXPECT(!result->ai_next);
    freeaddrinfo(result);

    current_case = "AI_ALL keeps mapped IPv4 answers in order";
    request = hints(AI_V4MAPPED | AI_ALL | AI_CANONNAME, AF_INET6);
    EXPECT(lookup(exchange, "example.test", nullptr, &request, &result) == 0);
    expect_address(result, &kMappedIpv4, sizeof(kMappedIpv4));
    EXPECT(result->ai_canonname && strcmp(result->ai_canonname, "example.test") == 0);
    expect_address(result->ai_next, &kIpv6, sizeof(kIpv6));
    expect_address(result->ai_next->ai_next, &kMappedSecondIpv4, sizeof(kMappedSecondIpv4));
    EXPECT(!result->ai_next->ai_next->ai_next);
    for (const addrinfo* item = result; item; item = item->ai_next)
        EXPECT(item->ai_flags == (AI_V4MAPPED | AI_ALL | AI_CANONNAME));
    freeaddrinfo(result);
}

void test_local_handling() {
    current_case = "numeric IPv4 host";
    addrinfo request = hints(0, AF_UNSPEC, SOCK_STREAM);
    expect_native("127.0.0.1", "80", &request);
    current_case = "numeric IPv6 host";
    expect_native("::1", "80", &request);
    current_case = "absent host";
    request = hints(AI_PASSIVE, AF_UNSPEC, SOCK_STREAM);
    expect_native(nullptr, "80", &request);
    current_case = "wildcard host";
    expect_native("*", "80", &request);
    current_case = "AI_NUMERICHOST name";
    request = hints(AI_NUMERICHOST);
    expect_native("example.test", nullptr, &request);
    current_case = "unknown flag";
    request = hints(0x8000);
    expect_native("example.test", nullptr, &request);
    current_case = "unsupported family";
    request = hints(0, AF_PACKET);
    expect_native("example.test", nullptr, &request);
    current_case = "IPv4 literal for an IPv6-only request";
    request = hints(0, AF_INET6);
    expect_native("192.0.2.1", nullptr, &request);
    current_case = "canonical name without a host";
    request = hints(AI_CANONNAME);
    expect_native(nullptr, "80", &request);

    const char* const names[] = {"", "a b", "a\tb", "a\nb", "a\rb", "a'b", "a\"b", "a\\b", "^x", "a^"};
    current_case = "host that is not a single command argument";
    for (const char* name : names) expect_refused(name, nullptr, EAI_NONAME);
    current_case = "service that is not a single command argument";
    expect_refused("example.test", "8 0", EAI_SERVICE);
    expect_refused("example.test", "^", EAI_SERVICE);
}

/// Checks a resolver that closes during a command larger than any socket buffer cannot raise
/// SIGPIPE, whose default action would end this process.
void test_closed_during_command() {
    current_case = "resolver closes while the command is sent";
    std::string host(16 << 20, 'a');
    host += ".test";
    Exchange exchange;
    exchange.read_limit = 12;
    addrinfo* result;
    errno = 0;
    EXPECT(lookup(exchange, host.c_str(), nullptr, nullptr, &result) == EAI_SYSTEM);
    EXPECT(errno == EPIPE || errno == ECONNRESET);
    EXPECT(result == kUntouched);
    EXPECT(resolver.command == "getaddrinfo ");
}

/// Interrupts the lookup while it waits for the reply; the interrupted receive must resume.
/// Needs the kernel's per-thread syscall state to know the lookup is blocked receiving.
void test_interrupted_receive() {
    current_case = "signal while waiting for the reply";
    char state_path[64];
    snprintf(state_path, sizeof(state_path), "/proc/self/task/%ld/syscall", syscall(SYS_gettid));
    if (access(state_path, R_OK) != 0) {
        puts("missing fixture: /proc thread syscall state; interrupted receive not checked");
        return;
    }
    struct sigaction action = {};
    action.sa_handler = record_signal;
    sigemptyset(&action.sa_mask);
    action.sa_flags = 0;  // no SA_RESTART, so the blocked receive fails with EINTR
    EXPECT(sigaction(SIGUSR1, &action, nullptr) == 0);
    Exchange exchange;
    exchange.reply = success_reply().bytes;
    exchange.interrupt_tid = static_cast<pid_t>(syscall(SYS_gettid));
    exchange.interrupt_thread = pthread_self();
    addrinfo* result;
    EXPECT(lookup(exchange, "example.test", nullptr, nullptr, &result) == 0);
    expect_address(result, &kIpv4, sizeof(kIpv4));
    freeaddrinfo(result);
    signal(SIGUSR1, SIG_DFL);
}

/// Checks unreachable resolver sockets fall back to the native resolver. The fixture checks
/// that the fallback is reported once on stderr across these calls.
void test_unreachable(const char* missing, const char* too_long) {
    const addrinfo request = hints(0, AF_UNSPEC, SOCK_STREAM);
    addrinfo* expected = nullptr;
    const int native = getaddrinfo("localhost", "80", &request, &expected);
    if (native == 0) freeaddrinfo(expected);
    for (const char* path : {missing, missing, too_long}) {
        current_case = path == missing ? "missing resolver socket" : "resolver path too long";
        addrinfo* result = nullptr;
        EXPECT(cinnabar_proxy_getaddrinfo(path, "localhost", "80", &request, &result) == native);
        if (native == 0) freeaddrinfo(result);
    }
}

}  // namespace

int main() {
    signal(SIGPIPE, SIG_DFL);
    resolver.open();
    handled_pipe_write = resolver.handled_pipe[1];
    test_commands();
    test_success();
    test_resolver_errors();
    test_truncation();
    test_malformed();
    test_odd_results();
    test_v4mapped();
    test_local_handling();
    test_closed_during_command();
    test_interrupted_receive();
    const std::string missing = std::string(resolver.directory) + "/missing";
    const std::string too_long = std::string(resolver.directory) + "/" + std::string(sizeof(sockaddr_un::sun_path), 'x');
    test_unreachable(missing.c_str(), too_long.c_str());
    resolver.close_all();
    puts("android resolver adapter ok");
}
