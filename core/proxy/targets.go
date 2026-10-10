package proxy

import (
	"context"
	"crypto/ecdsa"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"strconv"
	"strings"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service"
)

// A raw NetherNet target names its signaling and ID: NetherNetTargetPrefix + signaling + "/" + id.
const (
	NetherNetTargetPrefix       = "nethernet/"
	NetherNetSignalingJSONRPC   = "jsonrpc"
	NetherNetSignalingWebSocket = "websocket"
)

const (
	friendTargetPrefix = "friend_xuid/"
	realmTargetPrefix  = "realm_id/"
	realmCodePrefix    = "realm/"
)

// remoteMaxMTU caps RakNet probes to addressed servers. A 1492-byte probe over a smaller
// path is fragmented, and anycast fronts can answer the fragments from a backend that never
// answers Request 2, stalling the dial until the next probe rung, about 2 s later.
const remoteMaxMTU = 1400

// remoteRakNet dials a server over RakNet without probing it for NetherNet.
func remoteRakNet() minecraft.RakNet { return minecraft.RakNet{MaxMTU: remoteMaxMTU} }

// rakNetConnectBudget is how long vanilla gives a RakNet connection: 12 offline attempts 500 ms
// apart, then 10 s for the server to accept the connection request.
const rakNetConnectBudget = 6*time.Second + 10*time.Second

// dialTransport dials network; a RakNet server that never accepts fails instead of waiting on ctx.
func dialTransport(ctx context.Context, network minecraft.Network, address string) (net.Conn, error) {
	return dialTransportWithin(ctx, network, address, rakNetConnectBudget)
}

// dialTransportWithin applies a connect deadline to RakNet, including probed fallbacks.
func dialTransportWithin(ctx context.Context, network minecraft.Network, address string, budget time.Duration) (net.Conn, error) {
	if !minecraft.IsRakNet(network) {
		return network.DialContext(ctx, address)
	}
	bounded, cancel := context.WithTimeout(ctx, budget)
	defer cancel()
	conn, err := network.DialContext(bounded, address)
	if err != nil && ctx.Err() == nil && errors.Is(bounded.Err(), context.DeadlineExceeded) {
		err = fmt.Errorf("proxy: %s did not accept the connection within %s (%w): %w", address, budget, context.DeadlineExceeded, err)
	}
	return conn, err
}

// remoteServerNetwork is the network for a server named by host:port rather than found on the
// LAN: like vanilla it probes the address for NetherNet HTTP signaling and falls back to RakNet.
// A nil trust joins any NetherNet server.
func remoteServerNetwork(logger *slog.Logger, trust minecraft.ServerTrust) addressedServerNetwork {
	return addressedServerNetwork{minecraft.AddressNetwork{
		RakNet:      remoteRakNet(),
		NetherNet:   minecraft.NetherNet{Dialer: nethernet.Dialer{Log: logger, AllowIdentitylessServer: true}},
		ServerTrust: trust,
	}}
}

// addressedServerNetwork presents a self-signed identity on signed-out NetherNet dials, which BDS
// requires even with online-mode off; signed-in dials present the account's.
type addressedServerNetwork struct{ minecraft.AddressNetwork }

// DialContext is the signed-out dial.
func (n addressedServerNetwork) DialContext(ctx context.Context, address string) (net.Conn, error) {
	selected, err := n.Select(ctx, address)
	if err != nil {
		return nil, err
	}
	return dialSignedOut(ctx, selected, address)
}

// dialSignedOut dials the selected transport, with a self-signed identity when it is NetherNet.
func dialSignedOut(ctx context.Context, selected minecraft.Network, address string) (net.Conn, error) {
	dialer, ok := selected.(identityProviderDialer)
	if !ok {
		return dialTransport(ctx, selected, address)
	}
	identity, err := selfSignedIdentity(time.Now())
	if err != nil {
		return nil, err
	}
	return dialer.DialContextIdentityProvider(ctx, address, identity.Token, identity.PrivateKey, identity.Domain)
}

type resolvedUpstreamTarget struct {
	address    string
	network    minecraft.Network
	clientData func(*login.ClientData) // applies a joined session's login fields
	friend     interface{ Close() error }
	xbox       interface{ Close() error }
	local      interface{ Close() error }
	offline    bool // explicit own-world offline LAN selection, never an authentication fallback
	realm      bool // vanilla words a failed Realm join as its own
}

// realmJoinError marks a failure while joining a Realm.
type realmJoinError struct{ err error }

func (e *realmJoinError) Error() string { return e.err.Error() }
func (e *realmJoinError) Unwrap() error { return e.err }

// close leaves the joined session before shutting down its Xbox services.
func (target *resolvedUpstreamTarget) close() error {
	if target == nil {
		return nil
	}
	var joined error
	if target.friend != nil {
		joined = errors.Join(joined, target.friend.Close())
	}
	if target.xbox != nil {
		joined = errors.Join(joined, target.xbox.Close())
	}
	if target.local != nil {
		joined = errors.Join(joined, target.local.Close())
	}
	return joined
}

// resolveUpstreamTarget resolves address to its transport; trust decides addressed NetherNet joins.
func resolveUpstreamTarget(ctx context.Context, address string, account *authcache.Account, logger *slog.Logger, trust minecraft.ServerTrust) (*resolvedUpstreamTarget, error) {
	address = strings.TrimSpace(address)
	if address == "" {
		return nil, errors.New("upstream target is empty")
	}
	if account == nil {
		if isStableTarget(address) {
			return nil, errors.New("authenticated target requires a Microsoft session")
		}
		return &resolvedUpstreamTarget{address: address, network: remoteServerNetwork(logger, trust)}, nil
	}

	resolveContext, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	switch {
	case strings.HasPrefix(strings.ToLower(address), friendTargetPrefix):
		return resolveFriendTarget(resolveContext, address, account, logger)
	case strings.HasPrefix(strings.ToLower(address), realmTargetPrefix),
		strings.HasPrefix(strings.ToLower(address), realmCodePrefix):
		return resolveRealmTarget(resolveContext, address, account, logger)
	case strings.HasPrefix(strings.ToLower(address), NetherNetTargetPrefix):
		return resolveRawNetherNetTarget(resolveContext, address, account, logger)
	case isRawNetherNetAddress(address):
		return nil, fmt.Errorf("NetherNet target %q needs its signaling: use %sjsonrpc/<id> or %swebsocket/<id>", address, NetherNetTargetPrefix, NetherNetTargetPrefix)
	default:
		return &resolvedUpstreamTarget{address: address, network: remoteServerNetwork(logger, trust)}, nil
	}
}

func resolveRealmTarget(ctx context.Context, address string, account *authcache.Account, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	reportConnectStage(ctx, ConnectStageRealm)
	target, err := lookupRealmTarget(ctx, address, account, logger)
	if err != nil {
		return nil, &realmJoinError{err: err}
	}
	target.realm = true
	return target, nil
}

func lookupRealmTarget(ctx context.Context, address string, account *authcache.Account, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	client, err := catalog.RealmsClient(ctx, account)
	if err != nil {
		return nil, err
	}
	var realmAddress realms.RealmAddress
	if strings.HasPrefix(strings.ToLower(address), realmTargetPrefix) {
		id, parseErr := strconv.Atoi(strings.TrimSpace(address[len(realmTargetPrefix):]))
		if parseErr != nil || id <= 0 {
			return nil, fmt.Errorf("invalid realm target %q", address)
		}
		realmAddress, err = client.RealmAddress(ctx, id)
	} else {
		code := strings.TrimSpace(address[len(realmCodePrefix):])
		realm, lookupErr := client.Realm(ctx, code)
		if lookupErr == nil {
			realmAddress, err = realm.Address(ctx)
		} else {
			err = lookupErr
		}
	}
	if err != nil {
		return nil, fmt.Errorf("resolve realm %q: %w", address, err)
	}
	if strings.TrimSpace(realmAddress.Address) == "" {
		return nil, fmt.Errorf("resolve realm %q: empty address", address)
	}
	protocol := realms.ParseNetworkProtocol(string(realmAddress.NetworkProtocol))
	if protocol == realms.NetworkProtocolDefault || protocol == "" {
		return &resolvedUpstreamTarget{address: realmAddress.Address, network: remoteRakNet()}, nil
	}
	connectionType, ok := realmConnectionType(protocol)
	if !ok {
		return nil, fmt.Errorf("realm %q uses unsupported network protocol %q", address, realmAddress.NetworkProtocol)
	}
	return newNetherNetTarget(realmAddress.Address, connectionType, account, logger), nil
}

func resolveFriendTarget(ctx context.Context, address string, account *authcache.Account, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	xuid := strings.TrimSpace(address[len(friendTargetPrefix):])
	if separator := strings.IndexByte(xuid, ':'); separator >= 0 {
		xuid = xuid[:separator]
	}
	if xuid == "" {
		return nil, fmt.Errorf("invalid friend target %q", address)
	}
	xbl, err := catalog.XboxClient(ctx, account)
	if err != nil {
		return nil, err
	}
	return resolveFriendWorld(ctx, xuid, xbl, account, logger)
}

// resolveFriendWorld owns the Xbox client, handing it to the target only after a successful join.
func resolveFriendWorld(ctx context.Context, xuid string, xbl *xsapi.Client, account *authcache.Account, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	retained := false
	defer func() {
		if !retained {
			_ = xbl.Close()
		}
	}()
	worlds, err := p2p.NewClient(xbl).Worlds(ctx)
	if err != nil {
		return nil, fmt.Errorf("request friend worlds: %w", err)
	}
	world := selectFriendWorld(worlds, xuid, xbl.UserInfo().XUID)
	if world == nil {
		return nil, fmt.Errorf("friend world %q is no longer joinable", xuid)
	}
	session, err := world.Join(ctx)
	if err != nil {
		return nil, fmt.Errorf("join friend world %q: %w", xuid, err)
	}
	joined, err := p2p.ClientTargetFromSession(session)
	if err != nil {
		_ = session.Close()
		return nil, fmt.Errorf("join friend world %q: %w", xuid, err)
	}
	target := newNetherNetTarget(joined.DialAddress(), joined.ConnectionType(), account, logger)
	target.clientData = joined.ApplyClientData
	target.friend = joined
	target.xbox = xbl
	retained = true
	return target, nil
}

// selectFriendWorld returns the owner's first world the friends tab lists for self, or nil.
func selectFriendWorld(worlds []p2p.World, ownerXUID, self string) *p2p.World {
	for index := range worlds {
		if worlds[index].OwnerID == ownerXUID && catalog.FriendWorldListed(worlds[index], self) {
			return &worlds[index]
		}
	}
	return nil
}

func resolveRawNetherNetTarget(ctx context.Context, address string, account *authcache.Account, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	id, connectionType, err := parseNetherNetTarget(address)
	if err != nil {
		return nil, err
	}
	return newNetherNetTarget(id, connectionType, account, logger), nil
}

// parseNetherNetTarget splits nethernet/<signaling>/<id>; the signaling is never inferred from the ID.
func parseNetherNetTarget(address string) (string, int, error) {
	signaling, id, _ := strings.Cut(address[len(NetherNetTargetPrefix):], "/")
	connectionType, ok := map[string]int{
		NetherNetSignalingJSONRPC:   p2p.ConnectionTypeSignalingOverJSONRPC,
		NetherNetSignalingWebSocket: p2p.ConnectionTypeSignalingOverWebSocket,
	}[strings.ToLower(signaling)]
	if !ok || !isRawNetherNetAddress(id) {
		return "", 0, fmt.Errorf("invalid NetherNet target %q: want %sjsonrpc/<id> or %swebsocket/<id>", address, NetherNetTargetPrefix, NetherNetTargetPrefix)
	}
	return id, connectionType, nil
}

// newNetherNetTarget signals with the account's Minecraft service token.
func newNetherNetTarget(address string, connectionType int, account *authcache.Account, logger *slog.Logger) *resolvedUpstreamTarget {
	return &resolvedUpstreamTarget{
		address: address,
		network: newScopedNetherNetNetwork(account, connectionType, logger),
	}
}

func realmConnectionType(protocol realms.NetworkProtocol) (int, bool) {
	switch realms.ParseNetworkProtocol(string(protocol)) {
	case realms.NetworkProtocolNetherNet:
		return p2p.ConnectionTypeSignalingOverWebSocket, true
	case realms.NetworkProtocolNetherNetJSONRPC:
		return p2p.ConnectionTypeSignalingOverJSONRPC, true
	default:
		return 0, false
	}
}

func isStableTarget(address string) bool {
	lower := strings.ToLower(strings.TrimSpace(address))
	return strings.HasPrefix(lower, friendTargetPrefix) || strings.HasPrefix(lower, realmTargetPrefix) ||
		strings.HasPrefix(lower, realmCodePrefix) || strings.HasPrefix(lower, NetherNetTargetPrefix)
}

func isRawNetherNetAddress(address string) bool {
	if _, err := strconv.ParseUint(address, 10, 64); err == nil {
		return true
	}
	return uuid.Validate(address) == nil
}

// scopedNetherNetNetwork dials through gophertunnel's NetherNet so authenticated dials present
// the Login's multiplayer token and key as the SDP identity, as vanilla does.
type scopedNetherNetNetwork struct {
	signal minecraft.DialSignalingFunc // fresh signaling per dial; the transport owns and closes it
	logger *slog.Logger
}

func newScopedNetherNetNetwork(serviceSource service.TokenSource, connectionType int, logger *slog.Logger) scopedNetherNetNetwork {
	signal := func(ctx context.Context, _ string) (minecraft.SignalingConn, error) {
		conn, err := p2p.DialClientSignaling(ctx, connectionType, serviceSource, p2p.ClientSignalingOptions{Log: logger})
		if err != nil {
			return nil, fmt.Errorf("establish NetherNet signaling: %w", err)
		}
		return conn, nil
	}
	return scopedNetherNetNetwork{signal: signal, logger: logger}
}

// transport accepts identityless answers as vanilla does, while
// go-nethernet still verifies a server identity that is present.
func (network scopedNetherNetNetwork) transport() minecraft.NetherNet {
	return minecraft.NetherNet{
		DialSignaling: network.signal,
		Dialer:        nethernet.Dialer{Log: network.logger, AllowIdentitylessServer: true},
	}
}

func (network scopedNetherNetNetwork) DialContext(ctx context.Context, address string) (net.Conn, error) {
	return wrapNetherNetDial(network.transport().DialContext(ctx, address))
}

// DialContextIdentityProvider is used by minecraft.Dialer for authenticated dials.
func (network scopedNetherNetNetwork) DialContextIdentityProvider(ctx context.Context, address, token string, key *ecdsa.PrivateKey, identityProvider string) (net.Conn, error) {
	return wrapNetherNetDial(network.transport().DialContextIdentityProvider(ctx, address, token, key, identityProvider))
}

func wrapNetherNetDial(conn net.Conn, err error) (net.Conn, error) {
	if err != nil {
		return nil, fmt.Errorf("dial NetherNet: %w", err)
	}
	return conn, nil
}

func (scopedNetherNetNetwork) PingContext(context.Context, string) ([]byte, error) {
	return nil, errors.New("NetherNet ping is unsupported")
}

func (scopedNetherNetNetwork) Listen(string) (minecraft.NetworkListener, error) {
	return nil, errors.New("NetherNet listen is unsupported")
}
