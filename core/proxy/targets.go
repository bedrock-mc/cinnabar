package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"github.com/sandertv/gophertunnel/minecraft/service/signaling"
	"github.com/sandertv/gophertunnel/minecraft/service/signaling/messaging"
	"golang.org/x/oauth2"
)

const (
	friendTargetPrefix = "friend_xuid/"
	realmTargetPrefix  = "realm_id/"
	realmCodePrefix    = "realm/"
)

type resolvedUpstreamTarget struct {
	address    string
	network    minecraft.Network
	clientData loginClientData
	xbl        *xsapi.Client
	playFab    *playfab.Client
	friend     interface{ Close() error }
}

// loginClientData contains only the field that must survive a P2P join. It is
// applied to the normal downstream-derived Dialer after target resolution.
type loginClientData struct {
	nonce string
}

func (target *resolvedUpstreamTarget) close() error {
	if target == nil {
		return nil
	}
	var joined error
	if target.friend != nil {
		joined = errors.Join(joined, target.friend.Close())
	}
	if target.playFab != nil {
		joined = errors.Join(joined, target.playFab.Close())
	}
	if target.xbl != nil {
		joined = errors.Join(joined, target.xbl.Close())
	}
	return joined
}

func resolveUpstreamTarget(ctx context.Context, address string, src oauth2.TokenSource, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	address = strings.TrimSpace(address)
	if address == "" {
		return nil, errors.New("upstream target is empty")
	}
	if src == nil {
		if isStableTarget(address) {
			return nil, errors.New("authenticated target requires a Microsoft session")
		}
		return &resolvedUpstreamTarget{address: address, network: minecraft.RakNet{}}, nil
	}

	resolveContext, cancel := context.WithTimeout(ctx, 45*time.Second)
	defer cancel()
	switch {
	case strings.HasPrefix(strings.ToLower(address), friendTargetPrefix):
		return resolveFriendTarget(resolveContext, address, src, logger)
	case strings.HasPrefix(strings.ToLower(address), realmTargetPrefix),
		strings.HasPrefix(strings.ToLower(address), realmCodePrefix):
		return resolveRealmTarget(resolveContext, address, src, logger)
	case isRawNetherNetAddress(address):
		return resolveRawNetherNetTarget(resolveContext, address, src, logger)
	default:
		return &resolvedUpstreamTarget{address: address, network: minecraft.RakNet{}}, nil
	}
}

func resolveRealmTarget(ctx context.Context, address string, src oauth2.TokenSource, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	client := realms.NewClient(src, nil)
	var realmAddress realms.RealmAddress
	var err error
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
		return &resolvedUpstreamTarget{address: realmAddress.Address, network: minecraft.RakNet{}}, nil
	}
	connectionType, ok := realmConnectionType(protocol)
	if !ok {
		return nil, fmt.Errorf("realm %q uses unsupported network protocol %q", address, realmAddress.NetworkProtocol)
	}
	return newNetherNetTarget(ctx, realmAddress.Address, connectionType, "", src, nil, logger)
}

func resolveFriendTarget(ctx context.Context, address string, src oauth2.TokenSource, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	xuid := strings.TrimSpace(address[len(friendTargetPrefix):])
	if separator := strings.IndexByte(xuid, ':'); separator >= 0 {
		xuid = xuid[:separator]
	}
	if xuid == "" {
		return nil, fmt.Errorf("invalid friend target %q", address)
	}
	xbl, err := newXSAPIClient(ctx, src)
	if err != nil {
		return nil, err
	}
	closeXBLOnError := true
	defer func() {
		if closeXBLOnError {
			_ = xbl.Close()
		}
	}()
	worlds, err := p2p.NewClient(xbl).Worlds(ctx)
	if err != nil {
		return nil, fmt.Errorf("request friend worlds: %w", err)
	}
	var world *p2p.World
	for index := range worlds {
		candidate := &worlds[index]
		if candidate.OwnerID == xuid && candidate.Joinability == p2p.JoinabilityFriends {
			world = candidate
			break
		}
	}
	if world == nil {
		return nil, fmt.Errorf("friend world %q is no longer joinable", xuid)
	}
	session, err := world.Join(ctx)
	if err != nil {
		return nil, fmt.Errorf("join friend world %q: %w", xuid, err)
	}
	connection := session.Connection()
	if err := connection.Validate(); err != nil {
		_ = session.Close()
		return nil, fmt.Errorf("validate friend world connection: %w", err)
	}
	connectionType := connection.Type
	networkID := ""
	if connectionType == p2p.ConnectionTypeSignalingOverJSONRPC {
		networkID = string(connection.NetherNetID)
	}
	target, err := newNetherNetTarget(ctx, connection.Address(), connectionType, networkID, src, xbl, logger)
	if err != nil {
		_ = session.Close()
		return nil, err
	}
	target.clientData.nonce = session.Nonce()
	target.friend = session
	closeXBLOnError = false
	return target, nil
}

func resolveRawNetherNetTarget(ctx context.Context, address string, src oauth2.TokenSource, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	return newNetherNetTarget(ctx, address, p2p.ConnectionTypeSignalingOverJSONRPC, "", src, nil, logger)
}

func newNetherNetTarget(ctx context.Context, address string, connectionType int, networkID string, src oauth2.TokenSource, xbl *xsapi.Client, logger *slog.Logger) (*resolvedUpstreamTarget, error) {
	if xbl == nil {
		var err error
		xbl, err = newXSAPIClient(ctx, src)
		if err != nil {
			return nil, err
		}
	}
	serviceSource, playFab, err := newServiceTokenSource(ctx, xbl)
	if err != nil {
		_ = xbl.Close()
		return nil, err
	}
	network := scopedNetherNetNetwork{
		serviceSource:  serviceSource,
		connectionType: connectionType,
		networkID:      networkID,
		logger:         logger,
	}
	return &resolvedUpstreamTarget{
		address: address,
		network: network,
		xbl:     xbl,
		playFab: playFab,
	}, nil
}

func newXSAPIClient(ctx context.Context, src oauth2.TokenSource) (*xsapi.Client, error) {
	client, err := xsapi.ClientConfig{RTAMode: xsapi.RTALazy}.New(ctx, auth.AndroidConfig.New(src, nil))
	if err != nil {
		return nil, fmt.Errorf("login to Xbox Live: %w", err)
	}
	return client, nil
}

func newServiceTokenSource(ctx context.Context, xbl *xsapi.Client) (service.TokenSource, *playfab.Client, error) {
	discovery, err := service.Default(ctx)
	if err != nil {
		return nil, nil, fmt.Errorf("discover Minecraft services: %w", err)
	}
	env := new(service.AuthorizationEnvironment)
	if err := discovery.Environment(env); err != nil {
		return nil, nil, fmt.Errorf("resolve Minecraft services: %w", err)
	}
	playFab, err := playfab.LoginWithXbox(ctx, env.PlayFabTitleID, xbl, playfab.ClientConfig{CreateAccount: true})
	if err != nil {
		return nil, nil, fmt.Errorf("login to PlayFab: %w", err)
	}
	return env.TokenSource(playFab, service.TokenConfig{}), playFab, nil
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
	return strings.HasPrefix(lower, friendTargetPrefix) || strings.HasPrefix(lower, realmTargetPrefix) || strings.HasPrefix(lower, realmCodePrefix)
}

func isRawNetherNetAddress(address string) bool {
	if _, err := strconv.ParseUint(address, 10, 64); err == nil {
		return true
	}
	return uuid.Validate(address) == nil
}

type scopedNetherNetNetwork struct {
	serviceSource  service.TokenSource
	connectionType int
	networkID      string
	logger         *slog.Logger
}

func (network scopedNetherNetNetwork) DialContext(ctx context.Context, address string) (net.Conn, error) {
	var (
		signalingConn nethernetSignalingConn
		err           error
	)
	switch network.connectionType {
	case p2p.ConnectionTypeSignalingOverJSONRPC:
		signalingConn, err = messaging.Dialer{
			NetworkID:                  network.networkID,
			IgnoreDeliveryNotification: true,
			Log:                        network.logger,
		}.DialContext(ctx, network.serviceSource)
	case p2p.ConnectionTypeSignalingOverWebSocket:
		signalingConn, err = signaling.Dialer{Log: network.logger}.DialContext(ctx, network.serviceSource)
	default:
		return nil, fmt.Errorf("unsupported NetherNet connection type %d", network.connectionType)
	}
	if err != nil {
		return nil, fmt.Errorf("establish NetherNet signaling: %w", err)
	}
	conn, err := (nethernet.Dialer{Log: network.logger, AllowIdentitylessServer: true}).DialContext(ctx, address, signalingConn)
	if err != nil {
		_ = signalingConn.Close()
		return nil, fmt.Errorf("dial NetherNet: %w", err)
	}
	return attachSignalingLifetime(conn, signalingConn), nil
}

func (scopedNetherNetNetwork) PingContext(context.Context, string) ([]byte, error) {
	return nil, errors.New("NetherNet ping is unsupported")
}

func (scopedNetherNetNetwork) Listen(string) (minecraft.NetworkListener, error) {
	return nil, errors.New("NetherNet listen is unsupported")
}

type nethernetSignalingConn interface {
	nethernet.Signaling
	io.Closer
}

type signalingBackedConn struct {
	net.Conn
	signaling nethernetSignalingConn
	once      sync.Once
}

func attachSignalingLifetime(conn net.Conn, signalingConn nethernetSignalingConn) net.Conn {
	if transport, ok := conn.(*nethernet.Conn); ok {
		go func() {
			<-transport.Context().Done()
			_ = signalingConn.Close()
		}()
		return transport
	}
	return &signalingBackedConn{Conn: conn, signaling: signalingConn}
}

func (conn *signalingBackedConn) Close() error {
	var connErr, signalingErr error
	conn.once.Do(func() { signalingErr = conn.signaling.Close() })
	connErr = conn.Conn.Close()
	return errors.Join(connErr, signalingErr)
}
