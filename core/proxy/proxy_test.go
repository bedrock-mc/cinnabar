package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestSpawnBarrierPreventsEarlyRelay(t *testing.T) {
	downReady := make(chan struct{})
	upReady := make(chan struct{})
	down := newFakeDownstream(func(ctx context.Context, _ minecraft.GameData) error {
		select {
		case <-downReady:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	})
	up := newFakeUpstream(func(ctx context.Context) error {
		select {
		case <-upReady:
			return nil
		case <-ctx.Done():
			return ctx.Err()
		}
	})
	p := &packet.NetworkStackLatency{Timestamp: 7}
	down.reads <- packetResult{packet: p}

	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- serveConnections(ctx, down, up) }()

	assertNoWrites(t, up)
	close(downReady)
	assertNoWrites(t, up)
	close(upReady)
	waitForWrites(t, up, 1)
	if got := up.written()[0]; got != p {
		t.Fatalf("forwarded packet = %T %p, want %T %p", got, got, p, p)
	}
	cancel()
	if err := <-done; err != nil && !errors.Is(err, context.Canceled) {
		t.Fatalf("serveConnections() error = %v", err)
	}
}

func TestSpawnBarrierFailureCancelsOther(t *testing.T) {
	wantErr := errors.New("downstream spawn failed")
	otherCancelled := make(chan struct{})
	down := newFakeDownstream(func(context.Context, minecraft.GameData) error { return wantErr })
	up := newFakeUpstream(func(ctx context.Context) error {
		<-ctx.Done()
		close(otherCancelled)
		return ctx.Err()
	})

	err := serveConnections(context.Background(), down, up)
	if !errors.Is(err, wantErr) {
		t.Fatalf("serveConnections() error = %v, want %v", err, wantErr)
	}
	select {
	case <-otherCancelled:
	default:
		t.Fatal("other spawn operation was not cancelled")
	}
}

func TestSpawnBarrierReturnsRuntimeIDMismatch(t *testing.T) {
	wantErr := errors.New("runtime entity ID mismatch")
	down := newFakeDownstream(func(context.Context, minecraft.GameData) error { return wantErr })
	up := newFakeUpstream(func(ctx context.Context) error {
		<-ctx.Done()
		return ctx.Err()
	})

	err := serveConnections(context.Background(), down, up)
	if !errors.Is(err, wantErr) {
		t.Fatalf("serveConnections() error = %v, want runtime mismatch", err)
	}
}

func TestRelayFIFO(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	want := []packet.Packet{
		&packet.NetworkStackLatency{Timestamp: 1},
		&packet.NetworkStackLatency{Timestamp: 2},
		&packet.NetworkStackLatency{Timestamp: 3},
	}
	for _, p := range want {
		down.reads <- packetResult{packet: p}
	}
	down.reads <- packetResult{err: io.EOF}

	err := serveConnections(context.Background(), down, up)
	if err != nil {
		t.Fatalf("serveConnections() error = %v", err)
	}
	got := up.written()
	if len(got) != len(want) {
		t.Fatalf("forwarded %d packets, want %d", len(got), len(want))
	}
	for i := range want {
		if got[i] != want[i] {
			t.Fatalf("forwarded packet %d out of order", i)
		}
	}
}

func TestRelayDisconnectClosesBothSides(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	down.reads <- packetResult{err: io.EOF}

	if err := serveConnections(context.Background(), down, up); err != nil {
		t.Fatalf("serveConnections() error = %v", err)
	}
	if !down.isClosed() || !up.isClosed() {
		t.Fatalf("closed states = downstream:%v upstream:%v, want both true", down.isClosed(), up.isClosed())
	}
}

func TestRelayClosePanicIsReturned(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	down.closePanic = true
	down.reads <- packetResult{err: io.EOF}

	err := serveConnections(context.Background(), down, up)
	if err == nil || !strings.Contains(err.Error(), "panic while closing session") {
		t.Fatalf("serveConnections() error = %v, want recovered close panic", err)
	}
}

func TestDialFailureClosePanicIsReturned(t *testing.T) {
	down := newFakeDownstream(nil)
	down.closePanic = true
	wantErr := errors.New("dial failed")

	err := finishDialFailure(down, wantErr)
	if !errors.Is(err, wantErr) || !strings.Contains(err.Error(), "panic while closing session") {
		t.Fatalf("finishDialFailure() error = %v, want dial error plus recovered close panic", err)
	}
}

func TestRelayCancellationAbortsBeforePanickingClose(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	down.closePanicBeforeUnblock = true
	up.closePanicBeforeUnblock = true

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- serveConnections(ctx, down, up) }()
	cancel()

	select {
	case err := <-done:
		if err == nil || !strings.Contains(err.Error(), "panic while closing session") {
			t.Fatalf("serveConnections() error = %v, want recovered close panic", err)
		}
	case <-time.After(time.Second):
		t.Fatal("cancellation remained blocked by panicking Close")
	}
	for name, session := range map[string]*fakeSession{"downstream": &down.fakeSession, "upstream": &up.fakeSession} {
		if got := session.lifecycleEvents(); len(got) < 2 || got[0] != "abort" || got[1] != "close" {
			t.Fatalf("%s lifecycle = %v, want abort before close", name, got)
		}
	}
}

func TestIsOrdinaryCloseRequiresEveryJoinedLeaf(t *testing.T) {
	if isOrdinaryClose(errors.Join(errors.New("decode failed"), net.ErrClosed)) {
		t.Fatal("mixed joined error classified as ordinary")
	}
	if !isOrdinaryClose(errors.Join(fmt.Errorf("wrapped: %w", io.EOF), context.Canceled, net.ErrClosed)) {
		t.Fatal("all-ordinary joined error classified as non-ordinary")
	}
}

func TestIsOrdinaryCloseRecognizesClassifiedTerminalTransportError(t *testing.T) {
	framed := streamnet.NewFramedConn(&terminalWriteConn{err: io.ErrClosedPipe})
	_, err := framed.Write([]byte{0xfe})
	if !isOrdinaryClose(err) {
		t.Fatalf("classified terminal transport error considered non-ordinary: %v", err)
	}
	if isOrdinaryClose(errors.Join(errors.New("decode failed"), err)) {
		t.Fatal("mixed application and classified terminal errors considered ordinary")
	}
}

func TestStopServerPropagatesListenerCleanupError(t *testing.T) {
	wantErr := errors.New("endpoint identity changed")
	var sessions sync.WaitGroup
	err := stopServer(func() {}, errorCloser{err: wantErr}, &sessions)
	if !errors.Is(err, wantErr) {
		t.Fatalf("stopServer() error = %v, want cleanup error", err)
	}
}

func TestDialCancellationReturnsWithoutWaitingForDialer(t *testing.T) {
	down := newFakeDownstream(nil)
	started := make(chan struct{})
	release := make(chan struct{})
	dial := func(context.Context) (upstreamSession, error) {
		close(started)
		<-release
		return newFakeUpstream(nil), nil
	}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- dialAndServe(ctx, down, dial) }()
	<-started
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("dialAndServe() error = %v, want context cancellation", err)
		}
	case <-time.After(time.Second):
		t.Fatal("dialAndServe waited for a dialer that ignored cancellation")
	}
	close(release)
	if got := down.lifecycleEvents(); len(got) < 2 || got[0] != "abort" || got[1] != "close" {
		t.Fatalf("downstream lifecycle = %v, want abort before close", got)
	}
}

func assertNoWrites(t *testing.T, session *fakeUpstream) {
	t.Helper()
	time.Sleep(30 * time.Millisecond)
	if got := len(session.written()); got != 0 {
		t.Fatalf("forwarded %d packets before spawn barrier", got)
	}
}

func waitForWrites(t *testing.T, session *fakeUpstream, count int) {
	t.Helper()
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		if len(session.written()) >= count {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatalf("forwarded %d packets, want at least %d", len(session.written()), count)
}

type packetResult struct {
	packet packet.Packet
	err    error
}

type fakeSession struct {
	reads                   chan packetResult
	closed                  chan struct{}
	abortOnce               sync.Once
	closeOnce               sync.Once
	unblockOnce             sync.Once
	writesMu                sync.Mutex
	writes                  []packet.Packet
	lifecycleMu             sync.Mutex
	lifecycle               []string
	closePanic              bool
	closePanicBeforeUnblock bool
}

func newFakeSession() fakeSession {
	return fakeSession{
		reads:  make(chan packetResult, 16),
		closed: make(chan struct{}),
	}
}

func (s *fakeSession) ReadPacket() (packet.Packet, error) {
	select {
	case <-s.closed:
		return nil, net.ErrClosed
	case result := <-s.reads:
		return result.packet, result.err
	}
}

func (s *fakeSession) WritePacket(p packet.Packet) error {
	select {
	case <-s.closed:
		return net.ErrClosed
	default:
	}
	s.writesMu.Lock()
	s.writes = append(s.writes, p)
	s.writesMu.Unlock()
	return nil
}

func (s *fakeSession) Close() error {
	s.recordLifecycle("close")
	if s.closePanicBeforeUnblock {
		panic("close failed before unblock")
	}
	s.closeOnce.Do(func() { s.unblockOnce.Do(func() { close(s.closed) }) })
	if s.closePanic {
		panic("close failed")
	}
	return nil
}

func (s *fakeSession) Abort() error {
	s.recordLifecycle("abort")
	s.abortOnce.Do(func() { s.unblockOnce.Do(func() { close(s.closed) }) })
	return nil
}

func (s *fakeSession) recordLifecycle(event string) {
	s.lifecycleMu.Lock()
	s.lifecycle = append(s.lifecycle, event)
	s.lifecycleMu.Unlock()
}

func (s *fakeSession) lifecycleEvents() []string {
	s.lifecycleMu.Lock()
	defer s.lifecycleMu.Unlock()
	return append([]string(nil), s.lifecycle...)
}

func (s *fakeSession) written() []packet.Packet {
	s.writesMu.Lock()
	defer s.writesMu.Unlock()
	return append([]packet.Packet(nil), s.writes...)
}

func (s *fakeSession) isClosed() bool {
	select {
	case <-s.closed:
		return true
	default:
		return false
	}
}

type fakeDownstream struct {
	fakeSession
	start func(context.Context, minecraft.GameData) error
}

func newFakeDownstream(start func(context.Context, minecraft.GameData) error) *fakeDownstream {
	if start == nil {
		start = func(context.Context, minecraft.GameData) error { return nil }
	}
	return &fakeDownstream{fakeSession: newFakeSession(), start: start}
}

func (s *fakeDownstream) StartGameContext(ctx context.Context, data minecraft.GameData) error {
	return s.start(ctx, data)
}

type fakeUpstream struct {
	fakeSession
	spawn func(context.Context) error
	data  minecraft.GameData
}

func newFakeUpstream(spawn func(context.Context) error) *fakeUpstream {
	if spawn == nil {
		spawn = func(context.Context) error { return nil }
	}
	return &fakeUpstream{fakeSession: newFakeSession(), spawn: spawn, data: minecraft.GameData{EntityRuntimeID: 9}}
}

func (s *fakeUpstream) DoSpawnContext(ctx context.Context) error { return s.spawn(ctx) }
func (s *fakeUpstream) GameData() minecraft.GameData             { return s.data }

type errorCloser struct{ err error }

func (c errorCloser) Close() error { return c.err }

type terminalWriteConn struct{ err error }

func (c *terminalWriteConn) Read([]byte) (int, error)         { return 0, io.EOF }
func (c *terminalWriteConn) Write([]byte) (int, error)        { return 0, c.err }
func (c *terminalWriteConn) Close() error                     { return nil }
func (c *terminalWriteConn) LocalAddr() net.Addr              { return proxyTestAddr("local") }
func (c *terminalWriteConn) RemoteAddr() net.Addr             { return proxyTestAddr("remote") }
func (c *terminalWriteConn) SetDeadline(time.Time) error      { return nil }
func (c *terminalWriteConn) SetReadDeadline(time.Time) error  { return nil }
func (c *terminalWriteConn) SetWriteDeadline(time.Time) error { return nil }

type proxyTestAddr string

func (a proxyTestAddr) Network() string { return "test" }
func (a proxyTestAddr) String() string  { return string(a) }
