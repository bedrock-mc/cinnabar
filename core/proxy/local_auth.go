package proxy

import (
	"context"
	"crypto/ecdsa"
	"errors"
	"net"
	"strings"
	"sync/atomic"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

// localBDSLoginError identifies a failure at the launcher's own server, before any transfer.
type localBDSLoginError struct {
	err             error
	beforeTransport bool
}

func (e *localBDSLoginError) Error() string { return e.err.Error() }
func (e *localBDSLoginError) Unwrap() error { return e.err }

// admitsOfflineRetry distinguishes unavailable account services from a refused login.
func (e *localBDSLoginError) admitsOfflineRetry() bool {
	var disconnect *minecraft.DisconnectPacketError
	if errors.As(e.err, &disconnect) {
		return disconnect.Reason == packet.DisconnectReasonHostWithoutKeys
	}
	return e.beforeTransport
}

// localBDSAuthNetwork records when account authentication has reached the local transport.
type localBDSAuthNetwork struct {
	localNetherNetNetwork
	started atomic.Bool
}

func (n *localBDSAuthNetwork) DialContext(ctx context.Context, address string) (net.Conn, error) {
	n.started.Store(true)
	return n.localNetherNetNetwork.DialContext(ctx, address)
}

func (n *localBDSAuthNetwork) DialContextIdentityProvider(ctx context.Context, address, token string, key *ecdsa.PrivateKey, provider string) (net.Conn, error) {
	n.started.Store(true)
	return n.localNetherNetNetwork.DialContextIdentityProvider(ctx, address, token, key, provider)
}

// localHopDialer confines a local retry's offline identity to its original address.
func localHopDialer(target *resolvedUpstreamTarget, address string, dialer minecraft.Dialer, account oauth2.TokenSource) minecraft.Dialer {
	if target.keylessFallback && !strings.EqualFold(address, target.address) && dialer.TokenSource == nil && account != nil {
		dialer.TokenSource = account
		dialer.IdentityData = login.IdentityData{}
	}
	return dialer
}
