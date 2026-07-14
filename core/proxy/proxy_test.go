package proxy

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"runtime/pprof"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

type dialerTestDownstream struct {
	identity login.IdentityData
	client   login.ClientData
	protocol minecraft.Protocol
}

func (d dialerTestDownstream) IdentityData() login.IdentityData { return d.identity }
func (d dialerTestDownstream) ClientData() login.ClientData     { return d.client }
func (d dialerTestDownstream) Proto() minecraft.Protocol        { return d.protocol }

func TestNewUpstreamDialerOfflinePreservesIdentity(t *testing.T) {
	downstream := dialerTestDownstream{
		identity: login.IdentityData{
			Identity:    "offline-identity",
			DisplayName: "Offline Player",
			XUID:        "must-not-be-copied",
			TitleID:     "must-not-be-copied",
		},
		client:   login.ClientData{DeviceModel: "client-data-sentinel"},
		protocol: minecraft.DefaultProtocol,
	}

	dialer := newUpstreamDialer(downstream, nil)
	if !dialer.EnableBatchReading {
		t.Fatal("batch reading is disabled in offline mode")
	}
	if dialer.TokenSource != nil {
		t.Fatal("TokenSource is non-nil in offline mode")
	}
	if dialer.IdentityData.Identity != downstream.identity.Identity || dialer.IdentityData.DisplayName != downstream.identity.DisplayName {
		t.Fatalf("IdentityData = %#v, want copied offline identity/display name", dialer.IdentityData)
	}
	if dialer.IdentityData.XUID != "" || dialer.IdentityData.TitleID != "" {
		t.Fatalf("IdentityData copied authenticated fields: %#v", dialer.IdentityData)
	}
	if dialer.ClientData.DeviceModel != downstream.client.DeviceModel {
		t.Fatalf("ClientData.DeviceModel = %q, want %q", dialer.ClientData.DeviceModel, downstream.client.DeviceModel)
	}
	if dialer.Protocol != downstream.protocol {
		t.Fatal("Protocol was not preserved")
	}
}

func TestNewUpstreamDialerAuthenticatedUsesTokenAndOmitsOfflineIdentity(t *testing.T) {
	downstream := dialerTestDownstream{
		identity: login.IdentityData{Identity: "offline-identity", DisplayName: "Offline Player"},
		client:   login.ClientData{DeviceModel: "client-data-sentinel"},
		protocol: minecraft.DefaultProtocol,
	}
	source := oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "sentinel"})

	dialer := newUpstreamDialer(downstream, source)
	if !dialer.EnableBatchReading {
		t.Fatal("batch reading is disabled in authenticated mode")
	}
	if dialer.TokenSource != source {
		t.Fatal("TokenSource was not preserved")
	}
	if dialer.IdentityData != (login.IdentityData{}) {
		t.Fatalf("IdentityData = %#v, want zero value in authenticated mode", dialer.IdentityData)
	}
	if dialer.ClientData.DeviceModel != downstream.client.DeviceModel {
		t.Fatalf("ClientData.DeviceModel = %q, want %q", dialer.ClientData.DeviceModel, downstream.client.DeviceModel)
	}
	if dialer.Protocol != downstream.protocol {
		t.Fatal("Protocol was not preserved")
	}
}

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

func TestRelayDoesNotForwardDownstreamSpawnLoadingScreens(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	wantFirst := &packet.NetworkStackLatency{Timestamp: 7}
	wantLaterLoading := &packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeStart}
	wantLast := &packet.NetworkStackLatency{Timestamp: 8}
	down.useBatchReads = true
	down.batchReads <- batchResult{packets: []packet.Packet{
		&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeStart},
		&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeEnd},
	}}
	down.batchReads <- batchResult{packets: []packet.Packet{wantFirst}}
	down.batchReads <- batchResult{packets: []packet.Packet{wantLaterLoading}}
	down.batchReads <- batchResult{packets: []packet.Packet{wantLast}}
	down.batchReads <- batchResult{err: io.EOF}

	err := pumpPackets(down, up, true)
	if !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	got := up.written()
	want := []packet.Packet{wantFirst, wantLaterLoading, wantLast}
	if len(got) != len(want) {
		t.Fatalf("forwarded packets = %#v, want %#v", got, want)
	}
	for index := range want {
		if got[index] != want[index] {
			t.Fatalf("forwarded packet %d = %#v, want %#v", index, got[index], want[index])
		}
	}
}

func TestRelayPreservesNonAdjacentLoadingScreens(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	want := []packet.Packet{
		&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeStart},
		&packet.NetworkStackLatency{Timestamp: 7},
		&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeEnd},
	}
	for _, value := range want {
		down.reads <- packetResult{packet: value}
	}
	down.reads <- packetResult{err: io.EOF}

	err := pumpPackets(down, up, true)
	if !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	got := up.written()
	if len(got) != len(want) {
		t.Fatalf("forwarded packets = %#v, want %#v", got, want)
	}
	for index := range want {
		if got[index] != want[index] {
			t.Fatalf("forwarded packet %d out of order", index)
		}
	}
}

func TestRelayNeverFiltersUpstreamLoadingScreens(t *testing.T) {
	up := newFakeUpstream(nil)
	down := newFakeDownstream(nil)
	want := []packet.Packet{
		&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeStart},
		&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeEnd},
	}
	for _, value := range want {
		up.reads <- packetResult{packet: value}
	}
	up.reads <- packetResult{err: io.EOF}

	err := pumpPackets(up, down, false)
	if !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	got := down.written()
	if len(got) != len(want) || got[0] != want[0] || got[1] != want[1] {
		t.Fatalf("forwarded packets = %#v, want %#v", got, want)
	}
}

func TestRelayPreservesUpstreamWireBatchBoundaries(t *testing.T) {
	up := newFakeUpstream(nil)
	down := newFakeDownstream(nil)
	up.useBatchReads = true
	first := []packet.Packet{
		&packet.NetworkStackLatency{Timestamp: 1},
		&packet.NetworkStackLatency{Timestamp: 2},
	}
	second := []packet.Packet{&packet.NetworkStackLatency{Timestamp: 3}}
	up.batchReads <- batchResult{packets: first}
	up.batchReads <- batchResult{packets: second}
	up.batchReads <- batchResult{err: io.EOF}

	if err := pumpPackets(up, down, false); !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	if err := down.Flush(); err != nil {
		t.Fatalf("flush remaining packets: %v", err)
	}
	batches := down.flushedBatches()
	if got, want := batchSizes(batches), []int{2, 1}; !slices.Equal(got, want) {
		t.Fatalf("batch sizes = %v, want %v", got, want)
	}
	want := append(append([]packet.Packet(nil), first...), second...)
	got := append(append([]packet.Packet(nil), batches[0]...), batches[1]...)
	for index := range want {
		if got[index] != want[index] {
			t.Fatalf("flattened packet %d was reordered", index)
		}
	}
}

func TestRelayPreservesDownstreamWireBatchBoundaries(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	down.useBatchReads = true
	first := []packet.Packet{
		&packet.NetworkStackLatency{Timestamp: 1},
		&packet.NetworkStackLatency{Timestamp: 2},
	}
	second := []packet.Packet{&packet.NetworkStackLatency{Timestamp: 3}}
	down.batchReads <- batchResult{packets: first}
	down.batchReads <- batchResult{packets: second}
	down.batchReads <- batchResult{err: io.EOF}

	if err := pumpPackets(down, up, true); !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	if got, want := batchSizes(up.flushedBatches()), []int{2, 1}; !slices.Equal(got, want) {
		t.Fatalf("batch sizes = %v, want %v", got, want)
	}
}

func TestRelayDoesNotMergeLoadingScreenStartAcrossWireBoundary(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	down.useBatchReads = true
	start := &packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeStart}
	normal := &packet.NetworkStackLatency{Timestamp: 1}
	down.batchReads <- batchResult{packets: []packet.Packet{start}}
	down.batchReads <- batchResult{packets: []packet.Packet{normal}}
	down.batchReads <- batchResult{err: io.EOF}

	if err := pumpPackets(down, up, true); !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	batches := up.flushedBatches()
	if got, want := batchSizes(batches), []int{1, 1}; !slices.Equal(got, want) {
		t.Fatalf("batch sizes = %v, want %v", got, want)
	}
	if batches[0][0] != start || batches[1][0] != normal {
		t.Fatal("loading-screen boundary packets were reordered")
	}
}

func TestRelayCapsUpstreamToDownstreamBatches(t *testing.T) {
	const packetLimit = 1600
	up := newFakeUpstream(nil)
	down := newFakeDownstream(nil)
	handshake := &packet.NetworkStackLatency{Timestamp: -1}
	if err := down.WritePacket(handshake); err != nil {
		t.Fatalf("prequeue handshake packet: %v", err)
	}

	relayed := make([]packet.Packet, packetLimit*2+1)
	for index := range relayed {
		relayed[index] = &packet.NetworkStackLatency{Timestamp: int64(index)}
	}
	up.useBatchReads = true
	up.batchReads <- batchResult{packets: relayed}
	up.batchReads <- batchResult{err: io.EOF}

	if err := pumpPackets(up, down, false); !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	if err := down.Flush(); err != nil {
		t.Fatalf("flush remaining packets: %v", err)
	}

	batches := down.flushedBatches()
	wantSizes := []int{1, packetLimit, packetLimit, 1}
	if len(batches) != len(wantSizes) {
		t.Fatalf("batch count = %d, want %d; sizes = %v", len(batches), len(wantSizes), batchSizes(batches))
	}
	for index, batch := range batches {
		if len(batch) != wantSizes[index] {
			t.Fatalf("batch %d size = %d, want %d", index, len(batch), wantSizes[index])
		}
		if len(batch) > packetLimit {
			t.Fatalf("batch %d size = %d, exceeds %d", index, len(batch), packetLimit)
		}
	}

	wantPackets := append([]packet.Packet{handshake}, relayed...)
	gotPackets := make([]packet.Packet, 0, len(wantPackets))
	for _, batch := range batches {
		gotPackets = append(gotPackets, batch...)
	}
	if len(gotPackets) != len(wantPackets) {
		t.Fatalf("flattened packet count = %d, want %d", len(gotPackets), len(wantPackets))
	}
	for index := range wantPackets {
		if gotPackets[index] != wantPackets[index] {
			t.Fatalf("flattened packet %d = %T %p, want %T %p", index, gotPackets[index], gotPackets[index], wantPackets[index], wantPackets[index])
		}
	}
}

func TestRelayPropagatesUpstreamBatchBoundaryFlushError(t *testing.T) {
	const packetLimit = 1600
	wantErr := errors.New("batch boundary flush failed")
	up := newFakeUpstream(nil)
	down := newFakeDownstream(nil)
	down.flushErr = wantErr
	batch := make([]packet.Packet, packetLimit)
	for index := range batch {
		batch[index] = &packet.NetworkStackLatency{Timestamp: int64(index)}
	}
	up.useBatchReads = true
	up.batchReads <- batchResult{packets: batch}

	err := pumpPackets(up, down, false)
	if !errors.Is(err, wantErr) {
		t.Fatalf("pumpPackets() error = %v, want %v", err, wantErr)
	}
	if got := len(down.written()); got != packetLimit {
		t.Fatalf("written packet count = %d, want %d", got, packetLimit)
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
	acceptDone := make(chan error, 1)
	acceptDone <- nil
	err := stopServer(func() {}, errorCloser{err: wantErr}, &sessions, acceptDone)
	if !errors.Is(err, wantErr) {
		t.Fatalf("stopServer() error = %v, want cleanup error", err)
	}
}

func TestBackpressuredAcceptHandoffAbortsBeforePanickingClose(t *testing.T) {
	server, client := net.Pipe()
	defer client.Close()
	conn := &handoffTestConn{Conn: server}
	listener := &singleAcceptListener{conn: conn, returned: make(chan struct{})}
	accepted := make(chan acceptResult)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- runAcceptLoop(ctx, listener, accepted) }()
	<-listener.returned
	cancel()

	select {
	case err := <-done:
		if err == nil || !strings.Contains(err.Error(), "panic while closing accepted connection") {
			t.Fatalf("runAcceptLoop() error = %v, want recovered Close panic", err)
		}
	case <-time.After(time.Second):
		t.Fatal("backpressured handoff cleanup blocked")
	}
	if got := conn.events(); len(got) != 2 || got[0] != "abort" || got[1] != "close" {
		t.Fatalf("handoff lifecycle = %v, want abort before close", got)
	}
}

func TestServeCancellationClosesRawPreLoginConnection(t *testing.T) {
	dir := t.TempDir()
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() {
		done <- Serve(ctx, Config{SocketDir: dir, Upstream: "127.0.0.1:1"})
	}()

	var networkName, address string
	deadline := time.Now().Add(2 * time.Second)
	for time.Now().Before(deadline) {
		var err error
		networkName, address, err = streamnet.Resolve(dir)
		if err == nil {
			break
		}
		select {
		case serveErr := <-done:
			t.Fatalf("Serve() stopped before publishing endpoint: %v", serveErr)
		default:
		}
		time.Sleep(5 * time.Millisecond)
	}
	if networkName == "" {
		cancel()
		t.Fatal("proxy endpoint was not published")
	}
	client, err := net.DialTimeout(networkName, address, time.Second)
	if err != nil {
		cancel()
		t.Fatalf("dial raw proxy endpoint: %v", err)
	}
	defer client.Close()
	waitForGoroutineStack(t, "minecraft.(*Listener).handleConn", true, time.Second)
	cancel()
	select {
	case err := <-done:
		if err != nil && !errors.Is(err, context.Canceled) {
			t.Fatalf("Serve() error = %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("Serve() remained blocked by raw pre-login connection")
	}

	if err := client.SetReadDeadline(time.Now().Add(2 * time.Second)); err != nil {
		t.Fatal(err)
	}
	readErr := make(chan error, 1)
	go func() {
		_, err := client.Read(make([]byte, 1))
		readErr <- err
	}()
	select {
	case err := <-readErr:
		if err == nil {
			t.Fatal("raw client remained readable after proxy shutdown")
		}
		var netErr net.Error
		if errors.As(err, &netErr) && netErr.Timeout() {
			t.Fatalf("raw client closed only by deadline: %v", err)
		}
	case <-time.After(500 * time.Millisecond):
		t.Fatal("raw client remained open after proxy shutdown")
	}
	waitForGoroutineStack(t, "minecraft.(*Listener).handleConn", false, time.Second)

	successor, err := streamnet.New(dir).Listen("")
	if err != nil {
		t.Fatalf("endpoint lease leaked after proxy shutdown: %v", err)
	}
	_ = successor.Close()
}

func TestServeReportsListenerReadyAfterEndpointPublication(t *testing.T) {
	dir := t.TempDir()
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() {
		done <- Serve(ctx, Config{SocketDir: dir, Upstream: "127.0.0.1:19132", Logger: logger})
	}()

	deadline := time.Now().Add(2 * time.Second)
	for !strings.Contains(output.String(), "msg=\"listener ready; waiting for local Rust client\"") && time.Now().Before(deadline) {
		select {
		case err := <-done:
			t.Fatalf("Serve() stopped before reporting readiness: %v", err)
		default:
		}
		time.Sleep(time.Millisecond)
	}
	network, endpoint, err := streamnet.Resolve(dir)
	if err != nil {
		cancel()
		t.Fatalf("listener was reported ready before endpoint publication: %v\n%s", err, output.String())
	}
	if got := output.String(); !strings.Contains(got, "msg=\"listener ready; waiting for local Rust client\" socket_dir="+dir+" network="+network+" endpoint="+endpoint) {
		cancel()
		t.Fatalf("listener readiness output = %q, want published %s endpoint %q for socket directory %q", got, network, endpoint, dir)
	}

	cancel()
	select {
	case err := <-done:
		if err != nil && !errors.Is(err, context.Canceled) {
			t.Fatalf("Serve() error = %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("Serve() did not stop after cancellation")
	}
}

func TestReportListenerReadyFallsBackToSocketDirectory(t *testing.T) {
	dir := t.TempDir()
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	reportListenerReady(logger, dir)

	got := output.String()
	if !strings.Contains(got, "msg=\"listener ready; waiting for local Rust client\" socket_dir="+dir) {
		t.Fatalf("fallback listener readiness output = %q, want socket directory %q", got, dir)
	}
	if strings.Contains(got, " network=") || strings.Contains(got, " endpoint=") {
		t.Fatalf("fallback listener readiness claimed an unresolved endpoint: %q", got)
	}
}

func waitForGoroutineStack(t *testing.T, substring string, want bool, timeout time.Duration) {
	t.Helper()
	deadline := time.Now().Add(timeout)
	for {
		var stacks bytes.Buffer
		if err := pprof.Lookup("goroutine").WriteTo(&stacks, 1); err != nil {
			t.Fatalf("read goroutine profile: %v", err)
		}
		present := bytes.Contains(stacks.Bytes(), []byte(substring))
		if present == want {
			return
		}
		if !time.Now().Before(deadline) {
			t.Fatalf("goroutine stack %q presence = %v, want %v\n%s", substring, present, want, stacks.String())
		}
		time.Sleep(5 * time.Millisecond)
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

func TestDialFollowingTransfersRedialsBeforeReturningSession(t *testing.T) {
	var addresses []string
	want := newFakeUpstream(nil)
	got, err := dialFollowingTransfers(context.Background(), "zeqa.net:19132", func(_ context.Context, address string) (upstreamSession, error) {
		addresses = append(addresses, address)
		switch len(addresses) {
		case 1:
			return nil, &minecraft.TransferError{Address: "na.zeqa.net", Port: 19133, ReloadWorld: true}
		case 2:
			return want, nil
		default:
			t.Fatalf("unexpected dial %d to %q", len(addresses), address)
			return nil, nil
		}
	})
	if err != nil {
		t.Fatalf("dialFollowingTransfers() error = %v", err)
	}
	if got != want {
		t.Fatalf("dialFollowingTransfers() session = %p, want %p", got, want)
	}
	if joined := strings.Join(addresses, ","); joined != "zeqa.net:19132,na.zeqa.net:19133" {
		t.Fatalf("dial addresses = %q", joined)
	}
}

func TestConnectUpstreamReportsOrderedConnectionState(t *testing.T) {
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	want := newFakeUpstream(nil)
	got, err := connectUpstream(
		context.Background(),
		"zeqa.net:19132",
		"microsoft",
		logger,
		func(context.Context, string) (upstreamSession, error) { return want, nil },
	)
	if err != nil {
		t.Fatalf("connectUpstream() error = %v", err)
	}
	if got != want {
		t.Fatalf("connectUpstream() session = %p, want %p", got, want)
	}
	assertProxyTextInOrder(t, output.String(),
		"msg=\"upstream connection starting\" target=zeqa.net:19132 authentication=microsoft",
		"msg=\"upstream connected\" target=zeqa.net:19132 authentication=microsoft",
	)
}

func TestReportLocalClientAcceptedIncludesSocketDirectory(t *testing.T) {
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	reportLocalClientAccepted(logger, "run/socket")
	if got := output.String(); !strings.Contains(got, "msg=\"local client accepted\" socket_dir=run/socket") {
		t.Fatalf("local client output = %q", got)
	}
}

func TestConnectUpstreamReportsConnectionFailure(t *testing.T) {
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	wantErr := errors.New("dial refused")
	_, err := connectUpstream(
		context.Background(),
		"localhost:19132",
		"offline",
		logger,
		func(context.Context, string) (upstreamSession, error) { return nil, wantErr },
	)
	if !errors.Is(err, wantErr) {
		t.Fatalf("connectUpstream() error = %v, want %v", err, wantErr)
	}
	assertProxyTextInOrder(t, output.String(),
		"msg=\"upstream connection starting\" target=localhost:19132 authentication=offline",
		"level=ERROR msg=\"upstream connection failed\" target=localhost:19132 authentication=offline error=\"dial refused\"",
	)
}

func TestDialFollowingTransfersRejectsCyclesAndInvalidDestinations(t *testing.T) {
	tests := []struct {
		name     string
		transfer minecraft.TransferError
		want     string
	}{
		{name: "cycle", transfer: minecraft.TransferError{Address: "zeqa.net", Port: 19132}, want: "cycle"},
		{name: "empty host", transfer: minecraft.TransferError{Port: 19132}, want: "empty address"},
		{name: "empty bracketed host", transfer: minecraft.TransferError{Address: "[]", Port: 19132}, want: "empty address"},
		{name: "zero port", transfer: minecraft.TransferError{Address: "na.zeqa.net"}, want: "zero port"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			_, err := dialFollowingTransfers(context.Background(), "zeqa.net:19132", func(context.Context, string) (upstreamSession, error) {
				return nil, &test.transfer
			})
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("dialFollowingTransfers() error = %v, want substring %q", err, test.want)
			}
		})
	}
}

func TestDialFollowingTransfersBoundsRedirectChain(t *testing.T) {
	dials := 0
	_, err := dialFollowingTransfers(context.Background(), "entry.example:19132", func(context.Context, string) (upstreamSession, error) {
		dials++
		return nil, &minecraft.TransferError{Address: fmt.Sprintf("hop-%d.example", dials), Port: 19132}
	})
	if err == nil || !strings.Contains(err.Error(), "too many transfers") {
		t.Fatalf("dialFollowingTransfers() error = %v, want bounded-transfer failure", err)
	}
	if dials != maxInitialTransferHops+1 {
		t.Fatalf("dial attempts = %d, want %d", dials, maxInitialTransferHops+1)
	}
}

func assertNoWrites(t *testing.T, session *fakeUpstream) {
	t.Helper()
	time.Sleep(30 * time.Millisecond)
	if got := len(session.written()); got != 0 {
		t.Fatalf("forwarded %d packets before spawn barrier", got)
	}
}

func assertProxyTextInOrder(t *testing.T, text string, parts ...string) {
	t.Helper()
	position := 0
	for _, part := range parts {
		next := strings.Index(text[position:], part)
		if next < 0 {
			t.Fatalf("output missing %q after byte %d:\n%s", part, position, text)
		}
		position += next + len(part)
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

type batchResult struct {
	packets []packet.Packet
	err     error
}

type fakeSession struct {
	reads                   chan packetResult
	batchReads              chan batchResult
	useBatchReads           bool
	closed                  chan struct{}
	abortOnce               sync.Once
	closeOnce               sync.Once
	unblockOnce             sync.Once
	writesMu                sync.Mutex
	writes                  []packet.Packet
	batchesMu               sync.Mutex
	pendingBatch            []packet.Packet
	batches                 [][]packet.Packet
	flushErr                error
	lifecycleMu             sync.Mutex
	lifecycle               []string
	closePanic              bool
	closePanicBeforeUnblock bool
}

func newFakeSession() fakeSession {
	return fakeSession{
		reads:      make(chan packetResult, 16),
		batchReads: make(chan batchResult, 16),
		closed:     make(chan struct{}),
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

func (s *fakeSession) ReadBatch() ([]packet.Packet, error) {
	if !s.useBatchReads {
		value, err := s.ReadPacket()
		if err != nil {
			return nil, err
		}
		return []packet.Packet{value}, nil
	}
	select {
	case <-s.closed:
		return nil, net.ErrClosed
	case result := <-s.batchReads:
		return result.packets, result.err
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
	s.batchesMu.Lock()
	s.pendingBatch = append(s.pendingBatch, p)
	s.batchesMu.Unlock()
	return nil
}

func (s *fakeSession) WritePacketImmediate(packets ...packet.Packet) error {
	for _, value := range packets {
		if err := s.WritePacket(value); err != nil {
			return err
		}
	}
	return s.Flush()
}

func (s *fakeSession) Flush() error {
	s.batchesMu.Lock()
	defer s.batchesMu.Unlock()
	if len(s.pendingBatch) == 0 {
		return nil
	}
	if s.flushErr != nil {
		return s.flushErr
	}
	s.batches = append(s.batches, append([]packet.Packet(nil), s.pendingBatch...))
	s.pendingBatch = s.pendingBatch[:0]
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

func (s *fakeSession) flushedBatches() [][]packet.Packet {
	s.batchesMu.Lock()
	defer s.batchesMu.Unlock()
	batches := make([][]packet.Packet, len(s.batches))
	for index := range s.batches {
		batches[index] = append([]packet.Packet(nil), s.batches[index]...)
	}
	return batches
}

func batchSizes(batches [][]packet.Packet) []int {
	sizes := make([]int, len(batches))
	for index := range batches {
		sizes[index] = len(batches[index])
	}
	return sizes
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

type singleAcceptListener struct {
	conn     net.Conn
	returned chan struct{}
}

func (listener *singleAcceptListener) Accept() (net.Conn, error) {
	close(listener.returned)
	return listener.conn, nil
}

type handoffTestConn struct {
	net.Conn
	mu        sync.Mutex
	lifecycle []string
}

func (conn *handoffTestConn) Abort() error {
	conn.record("abort")
	return conn.Conn.Close()
}

func (conn *handoffTestConn) Close() error {
	conn.record("close")
	panic("close after abort")
}

func (conn *handoffTestConn) record(event string) {
	conn.mu.Lock()
	conn.lifecycle = append(conn.lifecycle, event)
	conn.mu.Unlock()
}

func (conn *handoffTestConn) events() []string {
	conn.mu.Lock()
	defer conn.mu.Unlock()
	return append([]string(nil), conn.lifecycle...)
}

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
