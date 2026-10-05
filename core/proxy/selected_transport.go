package proxy

import (
	"context"
	"log/slog"
	"net"
	"sync"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
)

const selectedTransportTTL = 5 * time.Second

func (selector *UpstreamSelector) startTransportPreparation(ctx context.Context) {
	if selector == nil {
		return
	}
	selector.mu.Lock()
	selector.preparation = newSelectedTransport(ctx, selectedTransportTTL, prepareSelectedRakNet)
	selector.mu.Unlock()
}

func (selector *UpstreamSelector) stopTransportPreparation() {
	if selector == nil {
		return
	}
	selector.mu.Lock()
	preparation := selector.preparation
	selector.preparation = nil
	selector.mu.Unlock()
	if preparation != nil {
		preparation.close()
	}
}

// PrepareTransport gives an explicitly selected RakNet address a bounded head start; empty cancels.
// It does not change the route selected by Set or authenticate a player.
func (selector *UpstreamSelector) PrepareTransport(address string) {
	if selector == nil {
		return
	}
	selector.mu.Lock()
	if selector.preparation != nil {
		selector.preparation.set(address)
	}
	selector.mu.Unlock()
}

func (selector *UpstreamSelector) claimTransport(address string) *preparedTransport {
	if selector == nil {
		return nil
	}
	selector.mu.Lock()
	defer selector.mu.Unlock()
	if selector.target != address || selector.preparation == nil {
		return nil
	}
	return selector.preparation.claim(address)
}

type transportSelection struct {
	target   string
	deadline time.Time
	prepared *preparedTransport
}

// selectedTransport owns one idle transport, never an authenticated game session.
type selectedTransport struct {
	mu         sync.Mutex
	wanted     string
	generation uint64
	pending    *transportSelection
	closed     bool
	wake       chan struct{}
	done       chan struct{}
	cancel     context.CancelFunc
	prepare    func(context.Context, string) *preparedTransport
	ttl        time.Duration
}

func newSelectedTransport(ctx context.Context, ttl time.Duration, prepare func(context.Context, string) *preparedTransport) *selectedTransport {
	ctx, cancel := context.WithCancel(ctx)
	selected := &selectedTransport{wake: make(chan struct{}, 1), done: make(chan struct{}), cancel: cancel, prepare: prepare, ttl: ttl}
	go selected.run(ctx)
	return selected
}

func (selected *selectedTransport) signal() {
	select {
	case selected.wake <- struct{}{}:
	default:
	}
}

func (selected *selectedTransport) set(target string) {
	selected.mu.Lock()
	if !selected.closed && selected.wanted != target {
		selected.wanted = target
		selected.generation++
		selected.signal()
	}
	selected.mu.Unlock()
}

func (selected *selectedTransport) keep(target string) {
	selected.mu.Lock()
	if selected.wanted != target {
		selected.wanted = ""
		selected.generation++
		selected.signal()
	}
	selected.mu.Unlock()
}

func (selected *selectedTransport) claim(target string) *preparedTransport {
	selected.mu.Lock()
	pending := selected.pending
	if selected.closed || selected.wanted != target || pending == nil || pending.target != target || !time.Now().Before(pending.deadline) {
		selected.mu.Unlock()
		return nil
	}
	selected.wanted = ""
	selected.generation++
	selected.signal()
	// A click falls back to a fresh ordinary dial instead of waiting on speculative retries.
	if !pending.prepared.ready() {
		pending.prepared.cancel()
		selected.mu.Unlock()
		return nil
	}
	selected.pending = nil
	selected.mu.Unlock()
	return pending.prepared
}

func (selected *selectedTransport) close() {
	selected.cancel()
	<-selected.done
}

func (selected *selectedTransport) run(ctx context.Context) {
	defer close(selected.done)
	timer := time.NewTimer(time.Hour)
	timer.Stop()
	defer timer.Stop()
	var processed uint64
	for {
		select {
		case <-ctx.Done():
			selected.mu.Lock()
			selected.closed = true
			pending := selected.pending
			selected.pending = nil
			selected.mu.Unlock()
			if pending != nil {
				pending.prepared.finish(false)
			}
			return
		case <-timer.C:
			selected.mu.Lock()
			pending := selected.pending
			selected.pending = nil
			selected.mu.Unlock()
			if pending != nil {
				pending.prepared.finish(false)
			}
		case <-selected.wake:
			for {
				selected.mu.Lock()
				if selected.generation == processed || ctx.Err() != nil {
					selected.mu.Unlock()
					break
				}
				processed = selected.generation
				wanted := selected.wanted
				pending := selected.pending
				selected.pending = nil
				selected.mu.Unlock()
				timer.Stop()
				if pending != nil {
					pending.prepared.finish(false)
				}
				selected.mu.Lock()
				if selected.generation == processed && wanted != "" && ctx.Err() == nil {
					selected.pending = &transportSelection{target: wanted, deadline: time.Now().Add(selected.ttl), prepared: selected.prepare(ctx, wanted)}
					timer.Reset(selected.ttl)
				}
				selected.mu.Unlock()
			}
		}
	}
}

// transportSocket closes an abandoned UDP socket without RakNet's graceful-close delay.
type transportSocket struct {
	mu     sync.Mutex
	conn   net.Conn
	closed bool
}

func (socket *transportSocket) DialContext(ctx context.Context, network, address string) (net.Conn, error) {
	conn, err := (&net.Dialer{}).DialContext(ctx, network, address)
	if err != nil {
		return nil, err
	}
	socket.mu.Lock()
	if socket.closed {
		socket.mu.Unlock()
		_ = conn.Close()
		return nil, net.ErrClosed
	}
	socket.conn = conn
	socket.mu.Unlock()
	return conn, nil
}

func (socket *transportSocket) close() {
	socket.mu.Lock()
	socket.closed = true
	conn := socket.conn
	socket.conn = nil
	socket.mu.Unlock()
	if conn != nil {
		_ = conn.Close()
	}
}

func prepareSelectedRakNet(ctx context.Context, address string) *preparedTransport {
	socket := new(transportSocket)
	network := minecraft.RakNet{Logger: slog.New(slog.DiscardHandler), UpstreamDialer: socket}
	return newOwnedPreparedTransport(ctx, network, address, socket.close)
}

type fallbackPreparedTransport struct {
	minecraft.Network
	prepared *preparedTransport
}

func (network fallbackPreparedTransport) DialContext(ctx context.Context, address string) (net.Conn, error) {
	conn, err := network.prepared.DialContext(ctx, address)
	if err != nil && ctx.Err() == nil {
		network.prepared.finish(false)
		return network.Network.DialContext(ctx, address)
	}
	return conn, err
}

func dialWithSelectedTransport(ctx context.Context, selector *UpstreamSelector, network minecraft.Network, address string,
	dial func(context.Context, minecraft.Network, string) (*minecraft.Conn, error),
) (connection *minecraft.Conn, err error) {
	switch network.(type) {
	case minecraft.RakNet, *minecraft.RakNet:
		if prepared := selector.claimTransport(address); prepared != nil {
			defer func() { prepared.finish(connection != nil && err == nil) }()
			reportJoinDuration(ctx, "selected_transport", time.Now(), true)
			return dial(ctx, fallbackPreparedTransport{Network: network, prepared: prepared}, address)
		}
	}
	return dialWithPreparedTransport(ctx, network, address, dial)
}
