package proxy

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/rsa"
	"errors"
	"fmt"
	"github.com/df-mc/go-nethernet/endpoint"
	"log/slog"
	"net"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/go-jose/go-jose/v4"
	"github.com/go-jose/go-jose/v4/jwt"
	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"golang.org/x/oauth2"

	"github.com/sandertv/gophertunnel/minecraft/p2p"
)

// The join picks the owner's world the friends tab lists, never an invite-only or empty one.
func TestSelectFriendWorldPicksAListedWorld(t *testing.T) {
	listed := p2p.World{OwnerID: "1", HostName: "Host", MemberCount: 1, BroadcastSetting: p2p.BroadcastSettingFriendsOfFriends}
	invite, empty, other := listed, listed, listed
	invite.BroadcastSetting, invite.WorldName = p2p.BroadcastSettingInviteOnly, "invite"
	empty.MemberCount, empty.WorldName = 0, "empty"
	other.OwnerID, other.WorldName = "2", "other"
	listed.WorldName = "listed"
	if got := selectFriendWorld([]p2p.World{invite, empty, other, listed}, "1", "self"); got == nil || got.WorldName != "listed" {
		t.Fatalf("selected %+v, want the listed world", got)
	}
	if got := selectFriendWorld([]p2p.World{invite, empty}, "1", "self"); got != nil {
		t.Fatalf("selected %+v, want none", got)
	}
}

// memorySignaling is an in-process NetherNet signaling peer on a shared hub.
type memorySignaling struct {
	hub    *memorySignalHub
	id     string
	ctx    context.Context
	cancel context.CancelFunc

	mu        sync.Mutex
	notifiers map[int]nethernet.Notifier
	next      int
}

type memorySignalHub struct {
	mu    sync.Mutex
	peers map[string]*memorySignaling
}

func (hub *memorySignalHub) peer(id string) *memorySignaling {
	ctx, cancel := context.WithCancel(context.Background())
	peer := &memorySignaling{hub: hub, id: id, ctx: ctx, cancel: cancel, notifiers: map[int]nethernet.Notifier{}}
	hub.mu.Lock()
	hub.peers[id] = peer
	hub.mu.Unlock()
	return peer
}

func (s *memorySignaling) Signal(_ context.Context, signal *nethernet.Signal) error {
	s.hub.mu.Lock()
	remote := s.hub.peers[signal.NetworkID]
	s.hub.mu.Unlock()
	if remote == nil {
		return fmt.Errorf("no peer %q", signal.NetworkID)
	}
	delivered := *signal
	delivered.NetworkID = s.id
	remote.mu.Lock()
	notifiers := make([]nethernet.Notifier, 0, len(remote.notifiers))
	for _, n := range remote.notifiers {
		notifiers = append(notifiers, n)
	}
	remote.mu.Unlock()
	for _, n := range notifiers {
		n.NotifySignal(&delivered)
	}
	return nil
}

func (s *memorySignaling) Notify(n nethernet.Notifier) func() {
	s.mu.Lock()
	id := s.next
	s.next++
	s.notifiers[id] = n
	s.mu.Unlock()
	return func() {
		s.mu.Lock()
		delete(s.notifiers, id)
		s.mu.Unlock()
	}
}

func (s *memorySignaling) Context() context.Context { return s.ctx }
func (s *memorySignaling) Credentials(context.Context) (*nethernet.Credentials, error) {
	return nil, nil
}
func (s *memorySignaling) NetworkID() string { return s.id }
func (s *memorySignaling) PongData([]byte)   {}
func (s *memorySignaling) Close() error      { s.cancel(); return nil }

// syntheticMultiplayerToken is an RS256 token shaped like the authorization service's, with cpk bound to key.
func syntheticMultiplayerToken(t *testing.T, key *ecdsa.PublicKey) string {
	t.Helper()
	issuer, err := rsa.GenerateKey(rand.Reader, 2048)
	if err != nil {
		t.Fatal(err)
	}
	signer, err := jose.NewSigner(jose.SigningKey{Algorithm: jose.RS256, Key: issuer}, nil)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	token, err := jwt.Signed(signer).Claims(map[string]any{
		"iss": "https://authorization.example/",
		"iat": now.Unix(),
		"exp": now.Add(time.Hour).Unix(),
		"cpk": jose.JSONWebKey{Key: key},
	}).Serialize()
	if err != nil {
		t.Fatal(err)
	}
	return token
}

// dialIdentityListener dials a listener that, like a Realm, refuses anonymous offers (code 37).
func dialIdentityListener(t *testing.T, dial func(scopedNetherNetNetwork, context.Context, string) (net.Conn, error)) (net.Conn, error, string) {
	t.Helper()
	hub := &memorySignalHub{peers: map[string]*memorySignaling{}}
	server := hub.peer("realm")
	var seen atomic.Value
	listener, err := nethernet.ListenConfig{
		Log: slog.New(slog.DiscardHandler),
		VerifyClientToken: func(_ context.Context, token string) (*ecdsa.PublicKey, error) {
			seen.Store(token)
			return nil, nil
		},
	}.Listen(server)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	go func() {
		if conn, err := listener.Accept(); err == nil {
			t.Cleanup(func() { _ = conn.Close() })
		}
	}()
	network := scopedNetherNetNetwork{signal: func(context.Context, string) (minecraft.SignalingConn, error) {
		return hub.peer("client"), nil
	}}
	ctx, cancel := context.WithTimeout(context.Background(), 20*time.Second)
	defer cancel()
	conn, err := dial(network, ctx, "realm")
	token, _ := seen.Load().(string)
	return conn, err, token
}

func TestNetherNetDialPresentsTheLoginTokenBoundToTheLoginKey(t *testing.T) {
	key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	token := syntheticMultiplayerToken(t, &key.PublicKey)
	conn, err, seen := dialIdentityListener(t, func(network scopedNetherNetNetwork, ctx context.Context, address string) (net.Conn, error) {
		return network.DialContextIdentityProvider(ctx, address, token, key, "https://authorization.example/")
	})
	if err != nil {
		t.Fatalf("identity dial: %v", err)
	}
	_ = conn.Close()
	if seen != token {
		t.Fatal("listener did not receive the Login's multiplayer token")
	}
}

func TestNetherNetDialRejectsATokenBoundToAnotherKey(t *testing.T) {
	key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	other, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	token := syntheticMultiplayerToken(t, &other.PublicKey)
	_, err, _ := dialIdentityListener(t, func(network scopedNetherNetNetwork, ctx context.Context, address string) (net.Conn, error) {
		return network.DialContextIdentityProvider(ctx, address, token, key, "https://authorization.example/")
	})
	if !isIdentityNotAllowed(err) {
		t.Fatalf("mismatched cpk dial error = %v, want code %d", err, nethernet.ErrorCodeIdentityNotAllowed)
	}
}

func TestNetherNetAnonymousDialIsRefusedLikeTheRealm(t *testing.T) {
	_, err, _ := dialIdentityListener(t, func(network scopedNetherNetNetwork, ctx context.Context, address string) (net.Conn, error) {
		return network.DialContext(ctx, address)
	})
	if !isIdentityNotAllowed(err) {
		t.Fatalf("anonymous dial error = %v, want code %d", err, nethernet.ErrorCodeIdentityNotAllowed)
	}
}

func isIdentityNotAllowed(err error) bool {
	return err != nil && strings.Contains(err.Error(), fmt.Sprintf("(code: %d)", nethernet.ErrorCodeIdentityNotAllowed))
}

// minecraft.Dialer only passes the identity to networks with this method.
var _ interface {
	DialContextIdentityProvider(context.Context, string, string, *ecdsa.PrivateKey, string) (net.Conn, error)
} = scopedNetherNetNetwork{}

// A raw NetherNet ID carries its signaling explicitly; a bare ID is refused rather than guessed.
func TestNetherNetTargetsNameTheirSignaling(t *testing.T) {
	id := "5db3882f-99fe-4648-97dd-1b55492e1cc9"
	for address, want := range map[string]int{
		"nethernet/jsonrpc/" + id:      p2p.ConnectionTypeSignalingOverJSONRPC,
		"NetherNet/WebSocket/12345678": p2p.ConnectionTypeSignalingOverWebSocket,
	} {
		if _, got, err := parseNetherNetTarget(strings.ToLower(address)); err != nil || got != want {
			t.Fatalf("%s = %d, %v; want %d", address, got, err, want)
		}
	}
	for _, address := range []string{"nethernet/jsonrpc/", "nethernet/carrier/" + id, "nethernet/websocket/host:1"} {
		if _, _, err := parseNetherNetTarget(address); err == nil {
			t.Fatalf("%s parsed", address)
		}
	}
	if _, err := resolveUpstreamTarget(context.Background(), id, authcache.NewAccount(context.Background(), "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "unused"}), nil), nil, nil); err == nil || !strings.Contains(err.Error(), "nethernet/jsonrpc/<id>") {
		t.Fatalf("bare ID error = %v", err)
	}
}

// A joined friend world's login fields (its nonce) reach the upstream Dialer through the native target.
func TestConnectAppliesTheJoinedSessionClientData(t *testing.T) {
	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.DiscardHandler))
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{network: minecraft.RakNet{}, clientData: func(data *login.ClientData) { data.Nonce = "joined-nonce" }}, nil
	}
	var nonce string
	connections.dialTarget = func(_ context.Context, _ *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		nonce = dialer.ClientData.Nonce
		return nil, errors.New("stop")
	}
	if _, err := connections.connect(context.Background(), dialerTestDownstream{protocol: minecraft.DefaultProtocol}); err == nil {
		t.Fatal("connect succeeded without an upstream")
	}
	if nonce != "joined-nonce" {
		t.Fatalf("dialer nonce = %q, want the joined session's", nonce)
	}
}

// MTU probes to an addressed server or a transfer hop fit the capped path, so none is fragmented.
func TestAddressedRakNetProbesFitTheCappedPath(t *testing.T) {
	server, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	address := server.LocalAddr().String()
	addressed, err := resolveUpstreamTarget(context.Background(), address, nil, slog.Default(), nil)
	if err != nil {
		t.Fatal(err)
	}
	networks := map[string]minecraft.Network{
		"addressed":    addressed.network,
		"transfer hop": networkForAddress(&resolvedUpstreamTarget{address: "entry.example:19132"}, address, nil),
	}
	for name, network := range networks {
		ctx, cancel := context.WithTimeout(context.Background(), 300*time.Millisecond)
		dialed := make(chan struct{})
		go func() {
			defer close(dialed)
			if conn, err := network.DialContext(ctx, address); err == nil {
				_ = conn.Close()
			}
		}()
		buffer := make([]byte, 2048)
		_ = server.SetReadDeadline(time.Now().Add(time.Second))
		n, _, err := server.ReadFrom(buffer)
		cancel()
		<-dialed
		if err != nil {
			t.Fatalf("%s: no MTU probe arrived: %v", name, err)
		}
		const openConnectionRequest1 = 0x05
		if buffer[0] != openConnectionRequest1 || n > remoteMaxMTU-28 {
			t.Fatalf("%s: first probe id %#x carries %d bytes, want an Open Connection Request 1 of at most %d", name, buffer[0], n, remoteMaxMTU-28)
		}
	}
}

// A signed-out join that the probe sends to NetherNet presents a self-signed identity, since BDS
// refuses anonymous HTTP offers even with online-mode off.
func TestSignedOutAddressedNetherNetDialPresentsAnIdentity(t *testing.T) {
	signaling := endpoint.HandlerConfig{Logger: slog.New(slog.DiscardHandler)}.New()
	t.Cleanup(func() { _ = signaling.Close() })
	listener, err := nethernet.ListenConfig{Log: slog.New(slog.DiscardHandler), DisableTrickleICE: true}.Listen(signaling)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	go func() {
		if conn, err := listener.Accept(); err == nil {
			t.Cleanup(func() { _ = conn.Close() })
		}
	}()
	server := httptest.NewServer(signaling)
	t.Cleanup(server.Close)

	target, err := resolveUpstreamTarget(t.Context(), server.Listener.Addr().String(), nil, slog.New(slog.DiscardHandler), nil)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	conn, err := target.network.DialContext(ctx, target.address)
	if err != nil {
		t.Fatalf("signed-out NetherNet dial: %v", err)
	}
	_ = conn.Close()
}
