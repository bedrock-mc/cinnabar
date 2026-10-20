package hostbridge

import (
	"context"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// serve owns the sole control stream and all of its bounded peer reservations.
func (l *Listener) serve(parent context.Context, bootstrap hostcontract.Bootstrap, lifetime *bridgeLifetime) {
	ctx, cancel := context.WithCancel(parent)
	var peers sync.WaitGroup
	retired := false
	retire := func() {
		if retired {
			return
		}
		retired = true
		cancel()
		peers.Wait()
		l.mu.Lock()
		remaining := make([]*Conn, 0, len(l.visitors))
		for _, c := range l.visitors {
			if c != nil {
				remaining = append(remaining, c)
			}
		}
		l.mu.Unlock()
		for _, c := range remaining {
			_ = c.Close()
		}
		l.mu.Lock()
		if l.bridge == lifetime {
			l.bridge = nil
		}
		l.mu.Unlock()
	}
	defer retire()
	control, err := connect(parent, bootstrap, 0)
	if err != nil {
		return
	}
	defer control.Close()
	stopWatch := make(chan struct{})
	watchDone := make(chan struct{})
	go func() {
		defer close(watchDone)
		select {
		case <-parent.Done():
			_ = control.Close()
		case <-stopWatch:
		}
	}()
	defer func() { close(stopWatch); <-watchDone }()
	if control.json(hostcontract.KindSnapshot, l.resourceSnapshot()) != nil {
		return
	}
	var lastPeer uint64
	for {
		kind, body, err := control.read()
		if err != nil {
			return
		}
		if kind == hostcontract.KindClose && len(body) == 0 {
			retire()
			_ = control.write(hostcontract.KindClosed, nil)
			return
		}
		var open hostcontract.Open
		if kind != hostcontract.KindOpen || decode(body, &open) != nil || open.Peer <= lastPeer || open.RemainingMillis == 0 ||
			open.RemainingMillis > hostcontract.MAX_OPEN_MILLIS {
			return
		}
		lastPeer = open.Peer
		l.mu.Lock()
		release, available := l.reserve()
		if !available || l.closed || len(l.visitors) >= cap(l.slots)-1 {
			l.mu.Unlock()
			if available {
				release()
			}
			if control.json(hostcontract.KindRejected, hostcontract.PeerResult{Peer: open.Peer, Code: 1}) != nil {
				return
			}
			continue
		}
		l.visitors[open.Peer] = nil
		l.workers.Add(1)
		peers.Add(1)
		l.mu.Unlock()
		go func() { defer peers.Done(); l.openPeer(ctx, control, bootstrap, open, release) }()
	}
}

// openPeer retains one reservation through preparation, real Conn handoff and final retirement.
func (l *Listener) openPeer(parent context.Context, control *wire, bootstrap hostcontract.Bootstrap, open hostcontract.Open, release func()) {
	defer l.workers.Done()
	var c *Conn
	defer func() {
		if c == nil {
			l.mu.Lock()
			delete(l.visitors, open.Peer)
			l.mu.Unlock()
			release()
			_ = control.json(hostcontract.KindRejected, hostcontract.PeerResult{Peer: open.Peer, Code: 2})
		}
	}()
	deadline := time.Now().Add(time.Duration(open.RemainingMillis) * time.Millisecond)
	ctx, cancel := context.WithDeadline(parent, deadline)
	defer cancel()
	w, err := connect(ctx, bootstrap, open.Peer)
	if err != nil {
		return
	}
	peer, err := newConn(parent, l, w, open, release)
	if err != nil || ctx.Err() != nil {
		_ = w.Close()
		return
	}
	peer.establishDeadline = deadline
	if w.setBase(deadline) != nil {
		peer.stop()
		return
	}
	l.mu.Lock()
	if l.closed || parent.Err() != nil {
		l.mu.Unlock()
		peer.stop()
		return
	}
	for _, previous := range l.visitors {
		if previous != nil && previous.identity.Identity == peer.identity.Identity {
			l.mu.Unlock()
			peer.stop()
			return
		}
	}
	c = peer
	l.visitors[open.Peer] = c
	l.mu.Unlock()
	if control.json(hostcontract.KindOpened, hostcontract.PeerResult{Peer: open.Peer}) != nil {
		c.stop()
	}
	c.run()
}
