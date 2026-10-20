package hostbridge

import (
	"context"
	"crypto/subtle"
	"encoding/hex"
	"io"
	"net"
	"net/netip"
	"slices"
	"sync"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/google/uuid"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

var _ server.Listener = (*Listener)(nil)

// bridgeLifetime identifies the original cancellation owner until all of its peers retire.
type bridgeLifetime struct {
	cancel context.CancelFunc
}

// Listener retains one original process bridge and shares its actual connection cap with local listeners.
type Listener struct {
	mu        sync.Mutex
	resources resources
	ready     bool
	closed    bool
	incoming  chan *Conn
	slots     chan struct{}
	visitors  map[uint64]*Conn
	closing   chan struct{}
	bridge    *bridgeLifetime
	workers   sync.WaitGroup
}

// New allocates no listener, goroutine or network route before its real Config callback and bootstrap.
func New(maxPlayers int) (*Listener, error) {
	if maxPlayers < 1 || maxPlayers > hostcontract.MAX_PLAYERS {
		return nil, errProtocol
	}
	return &Listener{
		incoming: make(chan *Conn, maxPlayers), slots: make(chan struct{}, maxPlayers),
		visitors: make(map[uint64]*Conn), closing: make(chan struct{}),
	}, nil
}

// Factory captures the actual final Config.New resources rather than the earlier mutable user config.
func (l *Listener) Factory(conf server.Config) (server.Listener, error) {
	if conf.MaxPlayers < 1 || conf.MaxPlayers != cap(l.slots) || conf.MaxChunkRadius < 1 {
		return nil, errProtocol
	}
	r, err := captureResources(conf)
	if err != nil {
		return nil, err
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.ready || l.closed {
		return nil, errProtocol
	}
	l.resources, l.ready = r, true
	return l, nil
}

// Accept yields only the actual post-pack Attach reservation to Dragonfly's real world finalizer.
func (l *Listener) Accept() (session.Conn, error) {
	for {
		select {
		case <-l.closing:
			return nil, io.EOF
		case c := <-l.incoming:
			if c.ctx.Err() == nil {
				return c, nil
			}
			_ = c.Close()
		}
	}
}

// Disconnect preserves older output before the actual generated Disconnect and bounded connection close.
func (l *Listener) Disconnect(conn session.Conn, reason string) error {
	if c, ok := conn.(*Conn); ok {
		return disconnect(c, reason)
	}
	return errProtocol
}

// Close stops bridge IO and joins every owned peer, without stopping the existing local world itself.
func (l *Listener) Close() error {
	l.mu.Lock()
	if !l.closed {
		l.closed = true
		close(l.closing)
	}
	if l.bridge != nil {
		l.bridge.cancel()
	}
	peers := make([]*Conn, 0, len(l.visitors))
	for _, c := range l.visitors {
		if c != nil {
			peers = append(peers, c)
			c.stop()
		}
	}
	l.mu.Unlock()
	l.workers.Wait()
	for _, c := range peers {
		_ = c.Close()
	}
	return nil
}

// RemotePlayer checks only current concrete native connection ownership, never a display name.
func (l *Listener) RemotePlayer(id uuid.UUID) bool {
	l.mu.Lock()
	defer l.mu.Unlock()
	for _, c := range l.visitors {
		if c != nil && c.identity.Identity == id.String() {
			return true
		}
	}
	return false
}

// forget removes only the original entry; an old connection cannot delete its replacement.
func (l *Listener) forget(c *Conn) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.visitors[c.peer] == c {
		delete(l.visitors, c.peer)
	}
}

// reserve counts a connection through actual retirement, across both local and native listener routes.
func (l *Listener) reserve() (func(), bool) {
	select {
	case l.slots <- struct{}{}:
		var once sync.Once
		return func() { once.Do(func() { <-l.slots }) }, true
	default:
		return nil, false
	}
}

// Start admits one caller-pipe bootstrap without blocking the existing pause/stop command parser.
func (l *Listener) Start(parent context.Context, bootstrap hostcontract.Bootstrap) error {
	address, err := netip.ParseAddrPort(bootstrap.Address)
	capability, capErr := hex.DecodeString(bootstrap.Capability)
	opening, openingErr := hex.DecodeString(bootstrap.Opening)
	if err != nil || capErr != nil || !address.Addr().Is4() || !address.Addr().IsLoopback() || address.Port() == 0 ||
		len(capability) != hostcontract.CAPABILITY_BYTES || openingErr != nil || len(opening) != hostcontract.OPENING_BYTES {
		return errProtocol
	}
	l.mu.Lock()
	defer l.mu.Unlock()
	if !l.ready || l.closed || l.bridge != nil {
		return errProtocol
	}
	ctx, cancel := context.WithCancel(parent)
	lifetime := &bridgeLifetime{cancel: cancel}
	l.bridge = lifetime
	l.workers.Add(1)
	go func() {
		defer l.workers.Done()
		defer cancel()
		defer clear(capability)
		l.serve(ctx, bootstrap, lifetime)
	}()
	return nil
}

// connect sends the original per-opening capability to the exact numeric parent endpoint.
func connect(ctx context.Context, bootstrap hostcontract.Bootstrap, peer uint64) (*wire, error) {
	deadline, cancel := context.WithTimeout(ctx, time.Duration(hostcontract.CLOSE_MILLIS)*time.Millisecond)
	defer cancel()
	connection, err := (&net.Dialer{}).DialContext(deadline, "tcp4", bootstrap.Address)
	if err != nil {
		return nil, err
	}
	w := &wire{Conn: connection}
	watchDone := make(chan struct{})
	stopWatch := make(chan struct{})
	go func() {
		defer close(watchDone)
		select {
		case <-deadline.Done():
			_ = w.Close()
		case <-stopWatch:
		}
	}()
	defer func() { close(stopWatch); <-watchDone }()
	until, _ := deadline.Deadline()
	if w.setBase(until) != nil {
		_ = w.Close()
		return nil, errProtocol
	}
	if err := w.json(hostcontract.KindHello, hostcontract.Hello{
		Version: hostcontract.VERSION, Opening: bootstrap.Opening, Capability: bootstrap.Capability, Peer: peer,
	}); err != nil {
		_ = w.Close()
		return nil, err
	}
	kind, body, err := w.read()
	var hello hostcontract.Hello
	if err != nil || kind != hostcontract.KindHello || decode(body, &hello) != nil || !validHello(hello, bootstrap, peer) ||
		w.setBase(time.Time{}) != nil || deadline.Err() != nil {
		_ = w.Close()
		return nil, errProtocol
	}
	return w, nil
}

// validHello compares only fixed-size decoded capabilities and the original opening/version/peer.
func validHello(hello hostcontract.Hello, bootstrap hostcontract.Bootstrap, peer uint64) bool {
	actual, err := hex.DecodeString(hello.Capability)
	expected, expectedErr := hex.DecodeString(bootstrap.Capability)
	defer clear(actual)
	defer clear(expected)
	return err == nil && expectedErr == nil && len(actual) == hostcontract.CAPABILITY_BYTES &&
		subtle.ConstantTimeCompare(actual, expected) == 1 && hello.Version == hostcontract.VERSION &&
		hello.Opening == bootstrap.Opening && hello.Peer == peer
}

// resourceSnapshot returns immutable metadata; archive objects remain owned by this listener.
func (l *Listener) resourceSnapshot() hostcontract.Snapshot {
	snapshot := l.resources.snapshot
	snapshot.Packs = slices.Clone(snapshot.Packs)
	return snapshot
}
