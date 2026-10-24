package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net"
	"sync"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-xsapi/v2/mpsd"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/redact"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/p2p"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/room"
	"github.com/sandertv/gophertunnel/minecraft/service/signaling/messaging"
)

// FriendWorldMaxPlayers is the hosted world's player limit, shared with the launcher's local servers.
const FriendWorldMaxPlayers = bridgecontract.HostedWorldMaxPlayers

// inviteTitleID names the title Cinnabar signs in as, which Xbox invites are sent from.
var inviteTitleID = fmt.Sprint(auth.AndroidConfig.TitleID)

// errNoHostedWorld ends a friend's join when the hosted world closed while it was preparing.
var errNoHostedWorld = errors.New("the hosted world is no longer open")

// ErrFriendHostReconnecting is returned while the host replaces a lost signaling connection;
// retrying later can succeed.
var ErrFriendHostReconnecting = errors.New("friends: reconnecting to Xbox Live; try again shortly")

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
	open     func(context.Context) (*friendEndpoint, error)
	config   func(friendDirectory) minecraft.ListenConfig
	prepared *preparedConnections
	log      *slog.Logger
	sessions sync.WaitGroup
	ctx      context.Context // ends every friend's relay; only Close cancels it
	cancel   context.CancelFunc
	done     chan struct{}

	mu       sync.Mutex
	endpoint *friendEndpoint
	players  int            // friends in the world; published to endpoint with mu held
	retries  sync.WaitGroup // withdrawals retried after failing; Close waits for them
}

// friendEndpoint is one signaling connection, the session it advertises and its listener.
type friendEndpoint struct {
	network        minecraft.Network
	directory      friendDirectory
	closeSignaling func() error
	listener       *minecraft.Listener
	stopOnce       sync.Once
	stopErr        error
	withdrawMu     sync.Mutex
	withdrawn      bool
}

// friendDirectory is the advertised session that issues join nonces and carries the world card.
type friendDirectory interface {
	nonce(xuid string) (string, bool)
	setMembers(count int)
	invite(ctx context.Context, xuid string) error
	Close() error
}

// friendReconnectBase and friendReconnectMax bound the wait between failed signaling reconnects,
// friendReconnectTimeout one attempt, and the withdraw bounds the wait between session withdrawals.
const (
	friendReconnectBase    = 5 * time.Second
	friendReconnectMax     = 2 * time.Minute
	friendReconnectTimeout = time.Minute
	friendWithdrawBase     = time.Second
	friendWithdrawMax      = 30 * time.Second
)

// StartFriendHost signals, publishes and listens; Close undoes all three.
func StartFriendHost(ctx context.Context, cfg FriendHostConfig) (*FriendHost, error) {
	log := cfg.Logger
	if log == nil {
		log = slog.Default()
	}
	log = log.With("component", "friend-host")
	host := newFriendHost(cfg.XUID, log, func(ctx context.Context) (*friendEndpoint, error) {
		return openFriendEndpoint(ctx, cfg, log)
	})
	host.prepared.resolveTarget = withLocalTarget(cfg.LocalTarget, func(context.Context) (*resolvedUpstreamTarget, error) {
		return nil, errNoHostedWorld
	})
	if err := host.start(ctx); err != nil {
		return nil, err
	}
	log.Info("hosting local world for friends", "world", cfg.World.ID)
	return host, nil
}

// newFriendHost admits friends of the host XUID through endpoints from open.
func newFriendHost(xuid string, log *slog.Logger, open func(context.Context) (*friendEndpoint, error)) *FriendHost {
	ctx, cancel := context.WithCancel(context.Background())
	h := &FriendHost{open: open, log: log, ctx: ctx, cancel: cancel, done: make(chan struct{})}
	h.prepared = newPreparedConnections("", nil, log)
	h.config = func(directory friendDirectory) minecraft.ListenConfig {
		return friendListenConfig(xuid, directory, h.prepared, log)
	}
	return h
}

// openFriendEndpoint dials signaling and publishes the session before any friend can connect,
// so every admission check has nonces to consult.
func openFriendEndpoint(ctx context.Context, cfg FriendHostConfig, log *slog.Logger) (*friendEndpoint, error) {
	signaling, err := messaging.Dialer{Log: log}.DialContext(ctx, cfg.Account)
	if err != nil {
		return nil, fmt.Errorf("friends: signaling: %w", err)
	}
	session, err := publishFriendSession(ctx, cfg.Xbox.MPSD(), cfg.XUID, friendStatus(cfg, signaling), log)
	if err != nil {
		return nil, errors.Join(fmt.Errorf("friends: %w", err), signaling.Close())
	}
	return &friendEndpoint{network: friendNetwork(signaling, log), directory: session, closeSignaling: signaling.Close}, nil
}

// start installs the first endpoint, failing without leaving anything running.
func (h *FriendHost) start(ctx context.Context) error {
	endpoint, err := h.openEndpoint(ctx)
	if err != nil {
		h.cancel()
		return errors.Join(err, h.prepared.shutdown())
	}
	h.mu.Lock()
	h.endpoint = endpoint
	h.mu.Unlock()
	go h.serve()
	return nil
}

// openEndpoint opens an endpoint and listens on it.
func (h *FriendHost) openEndpoint(ctx context.Context) (*friendEndpoint, error) {
	endpoint, err := h.open(ctx)
	if err != nil {
		return nil, err
	}
	// Admission reads this endpoint's own session: the listener accepts before it is installed.
	endpoint.listener, err = h.config(endpoint.directory).ListenNetwork(endpoint.network, "")
	if err != nil {
		return nil, errors.Join(fmt.Errorf("friends: listen: %w", err), endpoint.retire())
	}
	return endpoint, nil
}

// retire withdraws the session, stops accepting and closes signaling; friends' relays continue.
func (e *friendEndpoint) retire() error {
	return errors.Join(e.withdraw(), e.stop())
}

// stop closes the listener and signaling once.
func (e *friendEndpoint) stop() error {
	e.stopOnce.Do(func() {
		var listenErr error
		if e.listener != nil {
			listenErr = e.listener.Close()
		}
		e.stopErr = errors.Join(listenErr, e.closeSignaling())
	})
	return e.stopErr
}

// withdraw closes the advertised session; a failed withdrawal may be retried.
func (e *friendEndpoint) withdraw() error {
	e.withdrawMu.Lock()
	defer e.withdrawMu.Unlock()
	if e.withdrawn {
		return nil
	}
	err := e.directory.Close()
	e.withdrawn = err == nil
	return err
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
func friendListenConfig(host string, directory friendDirectory, prepared *preparedConnections, log *slog.Logger) minecraft.ListenConfig {
	return minecraft.ListenConfig{
		FlushRate:           -1, // the relay's packet readers own flushing
		AcceptedProtocols:   []minecraft.Protocol{minecraft.DefaultProtocol},
		AllowUnknownPackets: true,
		EnableBatchReading:  true,
		MaximumPlayers:      FriendWorldMaxPlayers,
		ErrorLog:            log,
		Allow: func(_ net.Addr, identity login.IdentityData, client login.ClientData) (string, bool) {
			nonce, issued := directory.nonce(identity.XUID)
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

// serve accepts friends and replaces the endpoint whenever its signaling connection is lost.
func (h *FriendHost) serve() {
	defer close(h.done)
	for {
		endpoint := h.current()
		h.accept(endpoint.listener)
		if h.ctx.Err() != nil {
			return
		}
		h.log.Warn("friends: signaling lost; reconnecting without ending friends' sessions")
		h.mu.Lock()
		h.endpoint = nil // nothing is advertised until the replacement is installed
		h.mu.Unlock()
		h.retire(endpoint)
		if !h.reconnect() {
			return
		}
	}
}

// reconnect installs a new endpoint, retrying with backoff until it succeeds or Close is called.
func (h *FriendHost) reconnect() bool {
	delay := friendReconnectBase
	for {
		attemptCtx, cancel := context.WithTimeout(h.ctx, friendReconnectTimeout)
		endpoint, err := h.openEndpoint(attemptCtx)
		cancel()
		if err == nil {
			h.mu.Lock()
			closed := h.ctx.Err() != nil
			if !closed {
				h.endpoint = endpoint
				endpoint.directory.setMembers(h.players + 1)
			}
			h.mu.Unlock()
			if closed {
				_ = endpoint.retire()
				return false
			}
			return true
		}
		if h.ctx.Err() != nil {
			return false
		}
		h.log.Warn("friends: reconnect signaling", "error", redact.Error(err), "retry_in", delay)
		select {
		case <-h.ctx.Done():
			return false
		case <-time.After(delay):
		}
		delay = min(delay*2, friendReconnectMax)
	}
}

// retire stops endpoint and keeps retrying its session withdrawal until it succeeds.
func (h *FriendHost) retire(endpoint *friendEndpoint) {
	if err := endpoint.stop(); err != nil {
		h.log.Debug("friends: stop endpoint", "error", redact.Error(err))
	}
	if endpoint.withdraw() == nil {
		return
	}
	h.retries.Add(1)
	go func() {
		defer h.retries.Done()
		delay := friendWithdrawBase
		for {
			select {
			case <-h.ctx.Done():
				_ = endpoint.withdraw() // one last attempt while the host closes
				return
			case <-time.After(delay):
			}
			err := endpoint.withdraw()
			if err == nil {
				return
			}
			h.log.Debug("friends: withdraw session", "error", redact.Error(err), "retry_in", delay)
			delay = min(delay*2, friendWithdrawMax)
		}
	}()
}

// addPlayers counts friends joining or leaving and publishes the total with the host to the
// current endpoint, under the lock that replacing the endpoint holds.
func (h *FriendHost) addPlayers(delta int) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.players += delta
	if h.endpoint != nil {
		h.endpoint.directory.setMembers(h.players + 1)
	}
}

// current is the endpoint friends are admitted through.
func (h *FriendHost) current() *friendEndpoint {
	h.mu.Lock()
	defer h.mu.Unlock()
	return h.endpoint
}

// accept relays every friend admitted through listener until it closes.
func (h *FriendHost) accept(listener *minecraft.Listener) {
	for {
		conn, err := listener.Accept()
		if err != nil {
			if h.ctx.Err() == nil && !errors.Is(err, net.ErrClosed) {
				h.log.Warn("friends: accept", "error", redact.Error(err))
			}
			return
		}
		downstream := conn.(*minecraft.Conn)
		prepared, err := takePreparedAfterAccept(h.prepared, downstream)
		if err != nil {
			h.log.Warn("friends: hand off", "error", redact.Error(err))
			continue
		}
		if prepared == nil {
			continue
		}
		h.sessions.Add(1)
		h.addPlayers(1)
		go func() {
			defer h.sessions.Done()
			defer h.addPlayers(-1)
			// A friend's session ending never stops the host.
			if err := servePreparedConnection(h.ctx, downstream, prepared); err != nil && h.ctx.Err() == nil {
				logFriendSessionEnded(h.log, downstream.IdentityData().XUID, err)
			}
		}()
	}
}

// logFriendSessionEnded records why a friend's session ended, the friend's XUID as a hash and
// the error redacted.
func logFriendSessionEnded(log *slog.Logger, xuid string, err error) {
	log.Debug("friends: session ended", "xuid", redact.Private(xuid), "error", redact.Error(err))
}

// Invite sends an Xbox Live invite to the world.
func (h *FriendHost) Invite(ctx context.Context, xuid string) error {
	endpoint := h.current()
	if endpoint == nil {
		return ErrFriendHostReconnecting
	}
	return endpoint.directory.invite(ctx, xuid)
}

// Done closes once the host has stopped for good; a lost signaling connection is replaced.
func (h *FriendHost) Done() <-chan struct{} {
	return h.done
}

// Close withdraws the session, stops accepting and ends every friend's session.
func (h *FriendHost) Close() error {
	h.mu.Lock()
	h.cancel()
	endpoint := h.endpoint
	h.mu.Unlock()
	var retireErr error
	if endpoint != nil {
		retireErr = endpoint.retire()
	}
	<-h.done
	h.retries.Wait()
	stopErr := h.prepared.shutdown()
	h.sessions.Wait()
	return errors.Join(retireErr, stopErr)
}
