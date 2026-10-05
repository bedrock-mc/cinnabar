package proxy

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"sync"
	"sync/atomic"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

const (
	selectedPreparationTimeout = 8 * time.Second
	selectedHealthInterval     = time.Second
	selectedRetryMin           = time.Second
	selectedRetryMax           = 30 * time.Second
)

var nextSelectedTransportID atomic.Uint64

func (selector *UpstreamSelector) startTransportPreparation(ctx context.Context, logger *slog.Logger) {
	if selector == nil {
		return
	}
	selector.mu.Lock()
	selector.preparation = newSelectedTransport(ctx, 0, prepareSelectedRakNet)
	selector.preparation.logger = logger
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

// PrepareTransport keeps one explicitly selected RakNet address ready; empty cancels.
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
	target     string
	deadline   time.Time
	readySince time.Time
	started    time.Time
	id         uint64
	prepared   *preparedTransport
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
	ctx        context.Context
	logger     *slog.Logger
}

// A positive lease permits one attempt; zero retains and renews the selected prefix until claimed or cleared.
func newSelectedTransport(ctx context.Context, ttl time.Duration, prepare func(context.Context, string) *preparedTransport) *selectedTransport {
	ctx, cancel := context.WithCancel(ctx)
	selected := &selectedTransport{wake: make(chan struct{}, 1), done: make(chan struct{}), cancel: cancel, prepare: prepare, ttl: ttl, ctx: ctx}
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
	if selected.closed || selected.ctx.Err() != nil || selected.wanted != target {
		selected.mu.Unlock()
		return nil
	}
	// Invalidate a factory result that has not attached before falling back.
	selected.wanted = ""
	selected.generation++
	selected.signal()
	if pending == nil || pending.target != target || selected.ttl > 0 && !time.Now().Before(pending.deadline) {
		selected.mu.Unlock()
		return nil
	}
	// A click falls back to a fresh ordinary dial instead of waiting on speculative retries.
	if !pending.prepared.ready() {
		status := "miss_not_ready"
		if pending.prepared.failure() != nil {
			status = "miss_failed"
		}
		pending.prepared.cancel()
		selected.mu.Unlock()
		selected.report(pending, status)
		return nil
	}
	selected.pending = nil
	selected.mu.Unlock()
	selected.report(pending, "hit")
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
	const (
		setup = iota
		health
		retry
		lease
	)
	var action int
	var setupDone <-chan struct{}
	var processed uint64
	delay := selectedRetryMin
	begin := func(wanted string, generation uint64) {
		prepared := selected.prepare(ctx, wanted)
		var attached *transportSelection
		selected.mu.Lock()
		if selected.generation == generation && ctx.Err() == nil {
			pending := &transportSelection{target: wanted, deadline: time.Now().Add(selected.ttl), prepared: prepared, started: time.Now(), id: nextSelectedTransportID.Add(1)}
			selected.pending = pending
			attached = pending
			if selected.ttl > 0 {
				action = lease
				timer.Reset(selected.ttl)
			} else {
				action = setup
				setupDone = prepared.done
				timer.Reset(selectedPreparationTimeout)
			}
			prepared = nil
		}
		selected.mu.Unlock()
		if attached != nil {
			selected.report(attached, "started")
		}
		if prepared != nil {
			prepared.finish(false)
		}
	}
	for {
		select {
		case <-ctx.Done():
			selected.mu.Lock()
			selected.closed = true
			pending := selected.pending
			selected.pending = nil
			selected.mu.Unlock()
			if pending != nil {
				selected.report(pending, "cancelled")
				pending.prepared.finish(false)
			}
			return
		case <-setupDone:
			setupDone = nil
			action = health
			timer.Reset(0)
		case <-timer.C:
			selected.mu.Lock()
			if selected.generation != processed || ctx.Err() != nil {
				selected.mu.Unlock()
				continue
			}
			if action == retry {
				wanted := selected.wanted
				selected.mu.Unlock()
				if wanted != "" {
					begin(wanted, processed)
				}
				continue
			}
			pending := selected.pending
			if pending == nil {
				selected.mu.Unlock()
				continue
			}
			if action == health && pending.prepared.ready() {
				first := pending.readySince.IsZero()
				if first {
					pending.readySince = time.Now()
				} else if time.Since(pending.readySince) >= selectedPreparationTimeout {
					delay = selectedRetryMin
				}
				timer.Reset(selectedHealthInterval)
				selected.mu.Unlock()
				if first {
					selected.report(pending, "ready")
				}
				continue
			}
			selected.pending = nil
			failure := pending.prepared.failure()
			selected.mu.Unlock()
			setupDone = nil
			status := "expired"
			if action == setup {
				status = "setup_timeout"
			} else if action == health {
				status = "unhealthy"
				if failure != nil {
					status = "failed"
				}
			}
			selected.report(pending, status)
			pending.prepared.finish(false)
			if selected.ttl == 0 && retrySelectedPreparation(failure) {
				action = retry
				timer.Reset(delay)
				delay = min(delay*2, selectedRetryMax)
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
				setupDone = nil
				delay = selectedRetryMin
				if pending != nil {
					selected.report(pending, "cancelled")
					pending.prepared.finish(false)
				}
				if wanted != "" && ctx.Err() == nil {
					begin(wanted, processed)
				}
			}
		}
	}
}

func retrySelectedPreparation(err error) bool {
	var disconnect *minecraft.DisconnectPacketError
	var transfer *minecraft.TransferError
	if errors.As(err, &disconnect) {
		return disconnect.Reason == packet.DisconnectReasonTimeout || disconnect.Reason == packet.DisconnectReasonExpiredToken
	}
	return !errors.As(err, &transfer)
}

func (selected *selectedTransport) report(pending *transportSelection, status string) {
	if selected.logger != nil {
		_ = callSafely("reporting selected transport", func() error {
			selected.logger.Info("JOIN_PREPARATION", "preparation_id", pending.id, "status", status,
				"elapsed_ms", time.Since(pending.started).Seconds()*1000)
			return nil
		})
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
