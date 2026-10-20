package webrelay

import (
	"errors"
	"fmt"
	"net"
	"net/netip"
	"syscall"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// errBlockedUpstream marks a dial refused because its resolved destination is not a public host.
var errBlockedUpstream = errors.New("webrelay: upstream address is not public")

// publicUpstream reports whether addr is a public unicast host a relay session may reach.
func publicUpstream(addr netip.Addr) bool {
	return bridgecontract.PublicGatewayUpstream(addr)
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
