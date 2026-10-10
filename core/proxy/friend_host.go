package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"sync"
	"sync/atomic"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-xsapi/v2/mpsd"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/room"
	"github.com/sandertv/gophertunnel/minecraft/service/signaling/messaging"
)

// FriendWorldMaxPlayers is vanilla's player limit for a hosted world, the host included.
const FriendWorldMaxPlayers = 8

// inviteTitleID names the title Cinnabar signs in as, which Xbox invites are sent from.
var inviteTitleID = fmt.Sprint(auth.AndroidConfig.TitleID)

// errNoHostedWorld ends a friend's join when the hosted world closed while it was preparing.
var errNoHostedWorld = errors.New("the hosted world is no longer open")

// FriendWorld describes the open local world a FriendHost advertises.
type FriendWorld struct {
	ID       string
	Name     string
	GameMode string
}

// FriendHostConfig starts hosting one open local world to Xbox friends.
type FriendHostConfig struct {
	Account *authcache.Account
	// Xbox is the signed-in account's Live client; the host does not close it.
	Xbox interface {
		MPSD() *mpsd.Client
	}
	XUID, Gamertag string
	World          FriendWorld
	// LocalTarget routes accepted friends into the open world's loopback server.
	LocalTarget LocalTargetFunc
	Logger      *slog.Logger
}

// FriendHost lets Xbox friends join the open local world the way vanilla hosts do: it publishes
// the world in the Xbox Live session directory, accepts NetherNet connections signaled through
// the franchise messaging service, admits only Xbox-authenticated players holding the nonce the
// session issued them, and relays each into the offline loopback server.
type FriendHost struct {
	signaling *messaging.Conn
	listener  *minecraft.Listener
	session   *friendSession
	prepared  *preparedConnections
	log       *slog.Logger
	players   atomic.Int32
	sessions  sync.WaitGroup
	cancel    context.CancelFunc
	done      chan struct{}
}

// StartFriendHost signals, listens and publishes; Close undoes all three.
func StartFriendHost(ctx context.Context, cfg FriendHostConfig) (host *FriendHost, err error) {
	log := cfg.Logger
	if log == nil {
		log = slog.Default()
	}
	log = log.With("component", "friend-host")
	signaling, err := messaging.Dialer{Log: log}.DialContext(ctx, cfg.Account)
	if err != nil {
		return nil, fmt.Errorf("friends: signaling: %w", err)
	}
	defer func() {
		if err != nil {
			_ = signaling.Close()
		}
	}()
	runCtx, cancel := context.WithCancel(context.Background())
	host = &FriendHost{signaling: signaling, log: log, cancel: cancel, done: make(chan struct{})}
	host.prepared = newPreparedConnections("", nil, log)
	host.prepared.resolveTarget = withLocalTarget(cfg.LocalTarget, func(context.Context) (*resolvedUpstreamTarget, error) {
		return nil, errNoHostedWorld
	})
	// The session exists before the listener, so every admission check has nonces to consult.
	host.session, err = publishFriendSession(ctx, cfg.Xbox.MPSD(), cfg.XUID, friendStatus(cfg, signaling), log)
	if err != nil {
		cancel()
		return nil, errors.Join(fmt.Errorf("friends: %w", err), host.prepared.shutdown())
	}
	host.listener, err = friendListenConfig(cfg.XUID, host, host.prepared).ListenNetwork(friendNetwork(signaling, log), "")
	if err != nil {
		cancel()
		return nil, errors.Join(fmt.Errorf("friends: listen: %w", err), host.session.Close(), host.prepared.shutdown())
	}
	go host.accept(runCtx)
	log.Info("hosting local world for friends", "world", cfg.World.ID)
	return host, nil
}

// friendNetwork allows offers without SDP identity; Minecraft Login still authenticates the player.
func friendNetwork(signaling nethernet.Signaling, log *slog.Logger) minecraft.NetherNet {
	return minecraft.NetherNet{
		Signaling: signaling, Log: log,
		ListenConfig: nethernet.ListenConfig{AllowAnonymous: true},
	}
}

// friendStatus is the world card vanilla shows in friends' lists.
func friendStatus(cfg FriendHostConfig, signaling *messaging.Conn) room.Status {
	return room.Status{
		Joinability:             p2p.JoinabilityFriends,
		HostName:                cfg.Gamertag,
		OwnerID:                 cfg.XUID,
		Version:                 protocol.CurrentVersion,
		LevelID:                 cfg.World.ID,
		WorldName:               cfg.World.Name,
		WorldType:               worldType(cfg.World.GameMode),
		Protocol:                protocol.CurrentProtocol,
		MemberCount:             1,
		MaxMemberCount:          FriendWorldMaxPlayers,
		BroadcastSetting:        p2p.BroadcastSettingFriendsOfFriends,
		TransportLayer:          p2p.TransportLayerNetherNet,
		OnlineCrossPlatformGame: true,
		SupportedConnections: []p2p.Connection{{
			Type:              p2p.ConnectionTypeSignalingOverJSONRPC,
			NetherNetID:       p2p.NetherNetID(signaling.NetworkID()),
			PlayerMessagingID: signaling.PlayerMessagingID(),
		}},
	}
}

// worldType words a local world's game mode as vanilla's world card does.
func worldType(gameMode string) string {
	switch gameMode {
	case localworld.GameModeCreative:
		return room.WorldTypeCreative
	case localworld.GameModeAdventure:
		return "Adventure"
	default:
		return "Survival"
	}
}

// friendListenConfig authenticates every friend with Xbox Live and admits only the nonce the
// session issued to that XUID; the host's own account joins through the local socket instead.
func friendListenConfig(host string, h *FriendHost, prepared *preparedConnections) minecraft.ListenConfig {
	return minecraft.ListenConfig{
		FlushRate:           -1, // the relay's packet readers own flushing
		AcceptedProtocols:   []minecraft.Protocol{minecraft.DefaultProtocol},
		AllowUnknownPackets: true,
		EnableBatchReading:  true,
		MaximumPlayers:      FriendWorldMaxPlayers,
		ErrorLog:            h.log,
		Allow: func(_ net.Addr, identity login.IdentityData, client login.ClientData) (string, bool) {
			nonce, issued := h.session.nonce(identity.XUID)
			return admitFriend(host, identity.XUID, client.Nonce, nonce, issued)
		},
		PrepareResourcePackOffer: prepared.prepare,
	}
}

// admitFriend admits an authenticated XUID only with the nonce its session membership earned.
func admitFriend(host, xuid, presented, issued string, ok bool) (string, bool) {
	switch {
	case xuid == "" || xuid == host:
		return "You cannot join this world.", false
	case !ok || presented == "" || presented != issued:
		return "Join this world from your friends list.", false
	}
	return "", true
}

func (h *FriendHost) accept(ctx context.Context) {
	defer close(h.done)
	for {
		conn, err := h.listener.Accept()
		if err != nil {
			if ctx.Err() == nil && !errors.Is(err, net.ErrClosed) {
				h.log.Warn("friends: accept", "error", err)
			}
			return
		}
		downstream := conn.(*minecraft.Conn)
		prepared, err := takePreparedAfterAccept(h.prepared, downstream)
		if err != nil {
			h.log.Warn("friends: hand off", "error", err)
			continue
		}
		if prepared == nil {
			continue
		}
		h.sessions.Add(1)
		h.session.setMembers(int(h.players.Add(1)) + 1)
		go func() {
			defer h.sessions.Done()
			defer func() { h.session.setMembers(int(h.players.Add(-1)) + 1) }()
			// A friend's session ending never stops the host.
			if err := servePreparedConnection(ctx, downstream, prepared); err != nil && ctx.Err() == nil {
				h.log.Debug("friends: session ended", "xuid", downstream.IdentityData().XUID, "error", err)
			}
		}()
	}
}

// Invite sends an Xbox Live invite to the world.
func (h *FriendHost) Invite(ctx context.Context, xuid string) error {
	return h.session.invite(ctx, xuid)
}

// Done closes when signaling drops and the host can no longer accept friends.
func (h *FriendHost) Done() <-chan struct{} {
	return h.signaling.Context().Done()
}

// Close withdraws the session, stops accepting and ends every friend's session.
func (h *FriendHost) Close() error {
	h.cancel()
	sessionErr := h.session.Close()
	listenErr := h.listener.Close()
	<-h.done
	stopErr := h.prepared.shutdown()
	h.sessions.Wait()
	return errors.Join(sessionErr, listenErr, stopErr, h.signaling.Close())
}
