//go:build integration

package hostbridge

import (
	"bytes"
	"context"
	"encoding/hex"
	"encoding/json"
	"log/slog"
	"net"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/session"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// TestCanceledQueuedAttachReleasesCapacity exercises the actual queue discard before a live handoff.
func TestCanceledQueuedAttachReleasesCapacity(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	l, parent, control, bootstrap := retirementBridge(t, ctx)
	staleSocket, stale := retirementPeer(t, l, parent, control, bootstrap, 1)
	defer staleSocket.Close()
	if err := staleSocket.write(hostcontract.KindAttach, nil); err != nil {
		t.Fatal(err)
	}
	if err := staleSocket.write(hostcontract.KindClose, nil); err != nil {
		t.Fatal(err)
	}
	kind, body, err := staleSocket.read()
	if err != nil || kind != hostcontract.KindClosed || len(body) != 0 {
		t.Fatal("queued peer did not complete its actual close")
	}
	select {
	case <-stale.done:
	case <-ctx.Done():
		t.Fatal("queued peer workers did not retire")
	}

	liveSocket, live := retirementPeer(t, l, parent, control, bootstrap, 2)
	defer liveSocket.Close()
	if err := liveSocket.write(hostcontract.KindAttach, nil); err != nil {
		t.Fatal(err)
	}
	type accepted struct {
		conn session.Conn
		err  error
	}
	result := make(chan accepted, 1)
	go func() { conn, err := l.Accept(); result <- accepted{conn, err} }()
	var next accepted
	select {
	case next = <-result:
	case <-ctx.Done():
		_ = l.Close()
		<-result
		t.Fatal("live peer was not accepted after the canceled queue entry")
	}
	l.mu.Lock()
	staleRetained := l.visitors[stale.peer] != nil
	held := len(l.slots)
	l.mu.Unlock()

	// Capture the real owner state, then retire every connection before the negative assertion.
	_ = live.Close()
	_ = stale.Close()
	_ = control.Close()
	_ = l.Close()
	if next.err != nil || next.conn != live {
		t.Fatal("actual Accept did not preserve the live connection")
	}
	if staleRetained || held != 1 {
		t.Fatalf("discarded queued Attach retained capacity: stale=%v held=%d, want only the live peer", staleRetained, held)
	}
}

// TestProcessClosedWaitsForPeerCancellation checks the earliest real retirement prerequisite.
func TestProcessClosedWaitsForPeerCancellation(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	l, parent, control, bootstrap := retirementBridge(t, ctx)
	peer, c := retirementPeer(t, l, parent, control, bootstrap, 1)
	defer peer.Close()
	type acknowledgment struct {
		kind hostcontract.Kind
		body []byte
		err  error
	}
	read := make(chan acknowledgment, 1)
	go func() { kind, body, err := control.read(); read <- acknowledgment{kind, body, err} }()

	// The actual registry lock holds retirement, while context cancellation must stay independent.
	l.mu.Lock()
	writeErr := control.write(hostcontract.KindClose, nil)
	var ack acknowledgment
	var received, beforeCancellation, expired bool
	if writeErr == nil {
		select {
		case ack = <-read:
			received, beforeCancellation = true, true
		case <-c.ctx.Done():
		case <-ctx.Done():
			expired = true
		}
	}
	l.mu.Unlock()
	if writeErr == nil && !received && !expired {
		select {
		case ack = <-read:
			received = true
		case <-ctx.Done():
			expired = true
		}
	}

	// Join actual workers on every branch before evaluating acknowledgment ordering.
	_ = c.Close()
	_ = peer.Close()
	_ = control.Close()
	_ = l.Close()
	if !received {
		ack = <-read
	}
	if writeErr != nil || expired || ack.err != nil || ack.kind != hostcontract.KindClosed || len(ack.body) != 0 {
		t.Fatal("actual process close did not complete")
	}
	if beforeCancellation {
		t.Fatal("process Closed acknowledged before its actual peer context was canceled")
	}
}

// TestProcessClosedAllowsAReplacementBridge preserves the new route after the original socket ends.
func TestProcessClosedAllowsAReplacementBridge(t *testing.T) {
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	l, parent, control, bootstrap := retirementBridge(t, ctx)
	oldSocket, old := retirementPeer(t, l, parent, control, bootstrap, 1)
	defer oldSocket.Close()
	if err := control.write(hostcontract.KindClose, nil); err != nil {
		t.Fatal(err)
	}
	kind, body, err := control.read()
	if err != nil || kind != hostcontract.KindClosed || len(body) != 0 {
		t.Fatal("original bridge did not complete close")
	}
	if err := l.Start(ctx, bootstrap); err != nil {
		t.Fatal("completed close retained the original bridge slot")
	}
	replacement := fixtureAccept(t, parent, bootstrap, 0)
	defer replacement.Close()
	var snapshot hostcontract.Snapshot
	fixtureReadJSON(t, replacement, hostcontract.KindSnapshot, &snapshot)
	if _, _, err := control.read(); err == nil {
		t.Fatal("original control socket did not end")
	}
	newSocket, current := retirementPeer(t, l, parent, replacement, bootstrap, 1)
	defer newSocket.Close()
	duplicate := l.Start(ctx, bootstrap)
	if err := replacement.write(hostcontract.KindClose, nil); err != nil {
		t.Fatal(err)
	}
	kind, body, err = replacement.read()
	_ = old.Close()
	_ = current.Close()
	_ = newSocket.Close()
	_ = oldSocket.Close()
	_ = replacement.Close()
	_ = control.Close()
	_ = l.Close()
	if snapshot.MaxPlayers != hostcontract.MAX_PLAYERS || duplicate == nil {
		t.Fatal("original cleanup did not preserve the sole replacement bridge")
	}
	if err != nil || kind != hostcontract.KindClosed || len(body) != 0 {
		t.Fatal("replacement bridge did not complete its actual close")
	}
}

// retirementBridge captures actual Config resources without starting Dragonfly's Accept loop.
func retirementBridge(t *testing.T, ctx context.Context) (*Listener, net.Listener, *wire, hostcontract.Bootstrap) {
	t.Helper()
	parent, err := net.Listen("tcp4", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = parent.Close() })
	l, err := New(hostcontract.MAX_PLAYERS)
	if err != nil {
		t.Fatal(err)
	}
	srv := (server.Config{MaxPlayers: hostcontract.MAX_PLAYERS, MaxChunkRadius: 1,
		Log: slog.New(slog.DiscardHandler), Listeners: []func(server.Config) (server.Listener, error){l.Factory},
		Generator: func(world.Dimension) world.Generator { return world.NopGenerator{} }}).New()
	t.Cleanup(func() {
		for _, w := range []*world.World{srv.End(), srv.Nether(), srv.World()} {
			if err := w.Close(); err != nil {
				t.Error(err)
			}
		}
	})
	t.Cleanup(func() { _ = l.Close() })
	bootstrap := hostcontract.Bootstrap{Address: parent.Addr().String(),
		Opening:    hex.EncodeToString(bytes.Repeat([]byte{8}, hostcontract.OPENING_BYTES)),
		Capability: hex.EncodeToString(bytes.Repeat([]byte{7}, hostcontract.CAPABILITY_BYTES))}
	if err := l.Start(ctx, bootstrap); err != nil {
		t.Fatal(err)
	}
	control := fixtureAccept(t, parent, bootstrap, 0)
	t.Cleanup(func() { _ = control.Close() })
	var snapshot hostcontract.Snapshot
	fixtureReadJSON(t, control, hostcontract.KindSnapshot, &snapshot)
	return l, parent, control, bootstrap
}

// retirementPeer opens a real process stream and observes its already-registered concrete Conn.
func retirementPeer(t *testing.T, l *Listener, parent net.Listener, control *wire, bootstrap hostcontract.Bootstrap, id uint64) (*wire, *Conn) {
	t.Helper()
	data, err := json.Marshal(login.ClientData{DeviceOS: protocol.DeviceAndroid, GameVersion: protocol.CurrentVersion,
		LanguageCode: "en_GB", SkinID: "fixture", ThirdPartyName: "Fixture"})
	if err != nil {
		t.Fatal(err)
	}
	open := hostcontract.Open{Peer: id, RemainingMillis: 4000,
		Identity: hostcontract.Identity{Xuid: "1234", Uuid: uuid.UUID{6: 0x40, 8: 0x80, 15: byte(id)}.String(), DisplayName: "Fixture"}, ClientData: data}
	if err := control.json(hostcontract.KindOpen, open); err != nil {
		t.Fatal(err)
	}
	peer := fixtureAccept(t, parent, bootstrap, id)
	var opened hostcontract.PeerResult
	fixtureReadJSON(t, control, hostcontract.KindOpened, &opened)
	l.mu.Lock()
	c := l.visitors[id]
	l.mu.Unlock()
	if opened.Peer != id || opened.Code != 0 || c == nil {
		_ = peer.Close()
		t.Fatal("actual pending connection was not registered")
	}
	return peer, c
}
