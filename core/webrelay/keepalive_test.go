package webrelay

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/netip"
	"sync"
	"testing"
	"testing/synctest"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

// droppedControlConn accepts writes into a lost transport without delivering them to the peer.
type droppedControlConn struct {
	net.Conn
	mu      sync.Mutex
	dropped bool
	pings   chan controlEvent
}

// Write models successful local buffering after the browser's network silently drops.
func (conn *droppedControlConn) Write(data []byte) (int, error) {
	conn.mu.Lock()
	dropped := conn.dropped
	conn.mu.Unlock()
	if !dropped {
		return conn.Conn.Write(data)
	}
	var event controlEvent
	if err := json.Unmarshal(data[4:], &event); err != nil {
		return 0, err
	}
	if event.Type == bridgecontract.RelayEventPing {
		conn.pings <- event
	}
	return len(data), nil
}

func TestControlRecoversAfterTwoLocallyBufferedPings(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		client := netip.MustParseAddr("192.0.2.1")
		controller, detach, err := relay.openControl(client, "", time.Now())
		if err != nil {
			t.Fatal(err)
		}
		defer func() { detach(time.Now()) }()
		server, browser := net.Pipe()
		defer server.Close()
		defer browser.Close()
		conn := &droppedControlConn{Conn: server, pings: make(chan controlEvent, 2)}
		done := make(chan struct{})
		go func() { defer close(done); relay.runControl(relay.ctx, conn, client, controller) }()
		for range 2 {
			if err := readJSON(browser, new(controlEvent)); err != nil {
				t.Fatal(err)
			}
		}
		var received controlEvent
		if err := readJSON(browser, &received); err != nil || received.Type != bridgecontract.RelayEventPing {
			t.Fatal("missing received keepalive", err)
		}
		<-time.After(time.Second) // Advance the virtual clock before answering the first ping.
		if err := writeJSON(context.Background(), browser, controlRequest{Op: bridgecontract.RelayOpPong, Resume: received.Resume}); err != nil {
			t.Fatal(err)
		}
		synctest.Wait()
		conn.mu.Lock()
		conn.dropped = true
		conn.mu.Unlock()
		first, second := <-conn.pings, <-conn.pings
		if first.Resume != second.Resume {
			t.Fatal("unanswered pings rotated the pending replacement twice")
		}
		<-done
		detach(time.Now())
		resumed, finish, err := relay.openControl(client, received.Resume, time.Now())
		if err != nil || resumed.session != controller.session {
			t.Fatal("successful local writes discarded the acknowledged browser bearer", err)
		}
		finish(time.Now())
	})
}

func TestControlRequestsCannotReplacePongLiveness(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		client := netip.MustParseAddr("192.0.2.1")
		controller, detach, err := relay.openControl(client, "", time.Now())
		if err != nil {
			t.Fatal(err)
		}
		defer func() { detach(time.Now()) }()
		server, browser := net.Pipe()
		defer server.Close()
		defer browser.Close()
		ctx, cancel := context.WithCancel(relay.ctx)
		defer cancel()
		done := make(chan struct{})
		go func() { defer close(done); relay.runControl(ctx, server, client, controller) }()
		for range 3 { // Welcome, account and the first ping.
			if err := readJSON(browser, new(controlEvent)); err != nil {
				t.Fatal(err)
			}
		}
		if err := writeJSON(context.Background(), browser, controlRequest{Op: bridgecontract.RelayOpSignOut}); err != nil {
			t.Fatal(err)
		}
		if err := readJSON(browser, new(controlEvent)); err != nil {
			t.Fatal(err)
		}
		select {
		case <-done:
		case <-time.After(keepAliveInterval + time.Second):
			cancel()
			<-done
			t.Fatal("a non-pong request extended a silent controller's lifetime")
		}
	})
}

func TestControlMessageFloodClosesTheController(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		client := netip.MustParseAddr("192.0.2.1")
		controller, detach, err := relay.openControl(client, "", time.Now())
		if err != nil {
			t.Fatal(err)
		}
		defer func() { detach(time.Now()) }()
		server, browser := net.Pipe()
		defer server.Close()
		defer browser.Close()
		done := make(chan struct{})
		go func() { defer close(done); relay.runControl(relay.ctx, server, client, controller) }()
		for range 2 {
			if err := readJSON(browser, new(controlEvent)); err != nil {
				t.Fatal(err)
			}
		}
		for range controlRequestBurst + 1 {
			if err := writeJSON(context.Background(), browser, controlRequest{Op: "unknown"}); err != nil {
				t.Fatal(err)
			}
		}
		select {
		case <-done:
		case <-time.After(time.Second):
			t.Fatal("a burst of control frames was not refused")
		}
	})
}

// cancelledDuringWriteConn cancels its owner, and lets the cancellation deadline land, as a write arms its own deadline.
type cancelledDuringWriteConn struct {
	net.Conn
	cancel context.CancelFunc
}

func (conn *cancelledDuringWriteConn) SetWriteDeadline(deadline time.Time) error {
	conn.cancel()
	synctest.Wait()
	return conn.Conn.SetWriteDeadline(deadline)
}

func TestCancellationRacingAWriteDeadlineStillStopsTheWrite(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		server, browser := net.Pipe()
		defer server.Close()
		defer browser.Close()
		ctx, cancel := context.WithCancel(t.Context())
		defer context.AfterFunc(ctx, func() { _ = server.SetDeadline(time.Now()) })()
		start := time.Now()
		err := writeJSON(ctx, &cancelledDuringWriteConn{Conn: server, cancel: cancel}, controlEvent{})
		if !errors.Is(err, context.Canceled) || time.Since(start) != 0 {
			t.Fatal("a per-write deadline outlived the controller's cancellation", err, time.Since(start))
		}
	})
}
