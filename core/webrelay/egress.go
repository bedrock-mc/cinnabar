package webrelay

import (
	"errors"
	"fmt"
	"net"
	"net/netip"
	"syscall"
	"time"
)

// errBlockedUpstream marks a dial refused because its resolved destination is not a public host.
var errBlockedUpstream = errors.New("webrelay: upstream address is not public")

// reservedPrefixes are special-purpose ranges netip's predicates do not cover. NAT64 and 6to4
// prefixes are refused because they embed, and can route to, any IPv4 address.
var reservedPrefixes = []netip.Prefix{
	netip.MustParsePrefix("0.0.0.0/8"),
	netip.MustParsePrefix("100.64.0.0/10"),
	netip.MustParsePrefix("192.0.0.0/24"),
	netip.MustParsePrefix("192.0.2.0/24"),
	netip.MustParsePrefix("192.88.99.0/24"),
	netip.MustParsePrefix("198.18.0.0/15"),
	netip.MustParsePrefix("198.51.100.0/24"),
	netip.MustParsePrefix("203.0.113.0/24"),
	netip.MustParsePrefix("240.0.0.0/4"),
	netip.MustParsePrefix("64:ff9b::/96"),
	netip.MustParsePrefix("64:ff9b:1::/48"),
	netip.MustParsePrefix("100::/64"),
	netip.MustParsePrefix("2001::/23"),
	netip.MustParsePrefix("2001:db8::/32"),
	netip.MustParsePrefix("2002::/16"),
	netip.MustParsePrefix("fec0::/10"),
}

// publicUpstream reports whether addr is a public unicast host a relay session may reach.
func publicUpstream(addr netip.Addr) bool {
	addr = addr.Unmap()
	if !addr.IsValid() || !addr.IsGlobalUnicast() || addr.IsPrivate() {
		return false
	}
	for _, prefix := range reservedPrefixes {
		if prefix.Contains(addr) {
			return false
		}
	}
	return true
}

// upstreamDialConnectTimeout bounds one socket setup; RakNet bounds the handshake itself.
const upstreamDialConnectTimeout = 10 * time.Second

// newUpstreamDialer returns the dialer every relay session joins through. Its Control runs after
// name resolution, on the exact address the socket connects to, so a hostname cannot resolve its
// way past the policy. allowPrivate admits every address, for a relay that only serves loopback.
func newUpstreamDialer(allowPrivate bool) *net.Dialer {
	return &net.Dialer{
		Timeout: upstreamDialConnectTimeout,
		Control: func(_, address string, _ syscall.RawConn) error {
			if allowPrivate {
				return nil
			}
			endpoint, err := netip.ParseAddrPort(address)
			if err != nil {
				return fmt.Errorf("%w: unparsable destination", errBlockedUpstream)
			}
			if !publicUpstream(endpoint.Addr()) {
				return errBlockedUpstream
			}
			return nil
		},
	}
}
