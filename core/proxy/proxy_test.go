package proxy

import (
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"math"
	"net"
	"path/filepath"
	"runtime/pprof"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"
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

// The UpstreamClientCache opt-in maps onto the native Dialer option and is off by default.
func TestNewUpstreamDialerDefaultsUpstreamClientCacheOff(t *testing.T) {
	optedIn := newUpstreamDialerForAdmission(
		dialerTestDownstream{protocol: minecraft.DefaultProtocol},
		nil,
		nil,
		nil,
		true,
	)
	if !optedIn.EnableClientCache {
		t.Fatal("EnableClientCache = false under the opt-in")
	}

	dialer := newUpstreamDialer(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil)
	if dialer.EnableClientCache {
		t.Fatal("EnableClientCache = true before downstream ClientCacheStatus is available")
	}
}

func TestNewUpstreamDialerDeclinesResourcePackAcquisitionWithoutBudget(t *testing.T) {
	dialer := newUpstreamDialer(dialerTestDownstream{protocol: minecraft.DefaultProtocol}, nil)
	if dialer.DownloadResourcePack == nil {
		t.Fatal("DownloadResourcePack is nil, want explicit admission callback")
	}
	for _, total := range []int{1, 8} {
		for index := range total {
			if dialer.DownloadResourcePack(uuid.New(), "1.0.0", index, total) {
				t.Fatalf("pack %d/%d accepted without an acquisition budget", index, total)
			}
		}
	}
	want := boundedResourcePackDownload()
	if dialer.ResourcePackDownload != want {
		t.Fatalf("ResourcePackDownload = %#v, want explicit bounds %#v", dialer.ResourcePackDownload, want)
	}
}

func TestProtocol2193RustFastTransferFixtureDecodesAsVanillaPlayerRequest(t *testing.T) {
	// Body bytes are shared with crates/protocol/tests/chat_send.rs. Decoding
	// them here prevents a self-round-trip from hiding a Rust/Go bridge
	// disagreement in CommandOrigin or UUID byte order.
	body := []byte{
		0x0d, '/', 't', 'r', 'a', 'n', 's', 'f', 'e', 'r', ' ', 's', 'm', '3',
		0x06, 'p', 'l', 'a', 'y', 'e', 'r',
		0x77, 0x66, 0x55, 0x44, 0x33, 0x22, 0x11, 0x00,
		0xff, 0xee, 0xdd, 0xcc, 0xbb, 0xaa, 0x99, 0x88,
		0x00,
		0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
		0x00,
		0x06, 'l', 'a', 't', 'e', 's', 't',
	}
	request := new(packet.CommandRequest)
	request.Marshal(minecraft.DefaultProtocol.NewReader(bytes.NewBuffer(body), 0, true))

	if request.CommandLine != "/transfer sm3" {
		t.Fatalf("CommandLine = %q, want /transfer sm3", request.CommandLine)
	}
	if request.CommandOrigin.Origin != protocol.CommandOriginPlayer {
		t.Fatalf("origin = %d, want player", request.CommandOrigin.Origin)
	}
	if request.CommandOrigin.UUID.String() != "00112233-4455-6677-8899-aabbccddeeff" {
		t.Fatalf("origin UUID = %s", request.CommandOrigin.UUID)
	}
	if request.CommandOrigin.RequestID != "" || request.CommandOrigin.PlayerUniqueID != 0 {
		t.Fatalf("origin extras = %#v, want empty request ID and entity ID 0", request.CommandOrigin)
	}
	if request.Internal || request.Version != "latest" {
		t.Fatalf("internal/version = %t/%q, want false/latest", request.Internal, request.Version)
	}
}

// TestCacheBoundaryScriptedUpstreamObservesDefaultDisabledStatus is the
// scripted-network ratchet for the default: without the opt-in, the fake
// upstream server observes Enabled=false exactly as before this option
// existed.
func TestCacheBoundaryScriptedUpstreamObservesDefaultDisabledStatus(t *testing.T) {
	network := newCacheStatusScriptedNetwork(func(conn net.Conn) error {
		decoder := packet.NewDecoder(conn)
		encoder := packet.NewEncoder(conn)
		if _, err := decoder.Decode(); err != nil {
			return fmt.Errorf("read RequestNetworkSettings: %w", err)
		}
		if err := encodeCacheStatusScriptedPackets(encoder, &packet.NetworkSettings{
			CompressionThreshold: math.MaxUint16,
			CompressionAlgorithm: packet.CompressionAlgorithmFlate,
		}); err != nil {
			return fmt.Errorf("write NetworkSettings: %w", err)
		}
		decoder.EnableCompression(packet.FlateCompression, math.MaxInt)
		encoder.EnableCompression(packet.FlateCompression, math.MaxUint16)
		if _, err := decoder.Decode(); err != nil {
			return fmt.Errorf("read Login: %w", err)
		}
		if err := encodeCacheStatusScriptedPackets(
			encoder,
			&packet.PlayStatus{Status: packet.PlayStatusLoginSuccess},
		); err != nil {
			return fmt.Errorf("write login success: %w", err)
		}
		batch, err := decoder.Decode()
		if err != nil {
			return fmt.Errorf("read ClientCacheStatus: %w", err)
		}
		if len(batch) != 1 {
			return fmt.Errorf("login response packet count = %d, want 1", len(batch))
		}
		buffer := bytes.NewBuffer(batch[0])
		header := new(packet.Header)
		if err := header.Read(buffer); err != nil {
			return fmt.Errorf("read ClientCacheStatus header: %w", err)
		}
		if header.PacketID != packet.IDClientCacheStatus {
			return fmt.Errorf("login response packet ID = %d, want %d", header.PacketID, packet.IDClientCacheStatus)
		}
		status := new(packet.ClientCacheStatus)
		status.Marshal(minecraft.DefaultProtocol.NewReader(buffer, 0, true))
		if status.Enabled {
			return errors.New("upstream ClientCacheStatus enabled cache before downstream capability was available")
		}
		return nil
	})
	dialer := newUpstreamDialer(
		dialerTestDownstream{protocol: minecraft.DefaultProtocol},
		nil,
	)
	dialer.FlushRate = -1
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	conn, err := dialer.DialContextNetwork(ctx, network, "cache-boundary.invalid:19132")
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil {
		t.Fatal("scripted server closed before full login but dial succeeded")
	}
	if scriptErr := <-network.done; scriptErr != nil {
		t.Fatalf("scripted cache status server: %v (dial error: %v)", scriptErr, err)
	}
}

// TestCacheBoundaryScriptedUpstreamObservesEnabledStatusWhenOptedIn drives the
// same scripted login with UpstreamClientCache enabled and requires the fake
// upstream server to observe the enabled ClientCacheStatus byte on the wire,
// plus honest effective-value telemetry.
func TestCacheBoundaryScriptedUpstreamObservesEnabledStatusWhenOptedIn(t *testing.T) {
	network := newCacheStatusScriptedNetwork(func(conn net.Conn) error {
		decoder := packet.NewDecoder(conn)
		encoder := packet.NewEncoder(conn)
		if _, err := decoder.Decode(); err != nil {
			return fmt.Errorf("read RequestNetworkSettings: %w", err)
		}
		if err := encodeCacheStatusScriptedPackets(encoder, &packet.NetworkSettings{
			CompressionThreshold: math.MaxUint16,
			CompressionAlgorithm: packet.CompressionAlgorithmFlate,
		}); err != nil {
			return fmt.Errorf("write NetworkSettings: %w", err)
		}
		decoder.EnableCompression(packet.FlateCompression, math.MaxInt)
		encoder.EnableCompression(packet.FlateCompression, math.MaxUint16)
		if _, err := decoder.Decode(); err != nil {
			return fmt.Errorf("read Login: %w", err)
		}
		if err := encodeCacheStatusScriptedPackets(
			encoder,
			&packet.PlayStatus{Status: packet.PlayStatusLoginSuccess},
		); err != nil {
			return fmt.Errorf("write login success: %w", err)
		}
		batch, err := decoder.Decode()
		if err != nil {
			return fmt.Errorf("read ClientCacheStatus: %w", err)
		}
		if len(batch) != 1 {
			return fmt.Errorf("login response packet count = %d, want 1", len(batch))
		}
		buffer := bytes.NewBuffer(batch[0])
		header := new(packet.Header)
		if err := header.Read(buffer); err != nil {
			return fmt.Errorf("read ClientCacheStatus header: %w", err)
		}
		if header.PacketID != packet.IDClientCacheStatus {
			return fmt.Errorf("login response packet ID = %d, want %d", header.PacketID, packet.IDClientCacheStatus)
		}
		status := new(packet.ClientCacheStatus)
		status.Marshal(minecraft.DefaultProtocol.NewReader(buffer, 0, true))
		if !status.Enabled {
			return errors.New("opt-in upstream ClientCacheStatus did not reach the scripted server enabled")
		}
		return nil
	})
	dialer := newUpstreamDialerForAdmission(
		dialerTestDownstream{protocol: minecraft.DefaultProtocol},
		nil,
		nil,
		nil,
		true,
	)
	dialer.FlushRate = -1
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	conn, err := dialer.DialContextNetwork(ctx, network, "cache-boundary.invalid:19132")
	if conn != nil {
		_ = conn.Close()
	}
	if err == nil {
		t.Fatal("scripted server closed before full login but dial succeeded")
	}
	if scriptErr := <-network.done; scriptErr != nil {
		t.Fatalf("scripted enabled cache status server: %v (dial error: %v)", scriptErr, err)
	}
}

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

	err := servePreparedConnection(context.Background(), down, &preparedConnection{upstream: up})
	if err != nil {
		t.Fatalf("servePreparedConnection() error = %v", err)
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

// A source batch is written once in order; splitting at the packet limit belongs to the library encoder.
func TestRelayWritesEachUpstreamBatchOnceInOrder(t *testing.T) {
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
	wantSizes := []int{1, packetLimit*2 + 1}
	if len(batches) != len(wantSizes) {
		t.Fatalf("batch count = %d, want %d; sizes = %v", len(batches), len(wantSizes), batchSizes(batches))
	}
	for index, batch := range batches {
		if len(batch) != wantSizes[index] {
			t.Fatalf("batch %d size = %d, want %d", index, len(batch), wantSizes[index])
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

	if err := servePreparedConnection(context.Background(), down, &preparedConnection{upstream: up}); err != nil {
		t.Fatalf("servePreparedConnection() error = %v", err)
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

	err := servePreparedConnection(context.Background(), down, &preparedConnection{upstream: up})
	if err == nil || !strings.Contains(err.Error(), "panic while closing session") {
		t.Fatalf("servePreparedConnection() error = %v, want recovered close panic", err)
	}
}

func TestRelayCancellationAbortsBeforePanickingClose(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	down.closePanicBeforeUnblock = true
	up.closePanicBeforeUnblock = true

	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- servePreparedConnection(ctx, down, &preparedConnection{upstream: up}) }()
	cancel()

	select {
	case err := <-done:
		if err == nil || !strings.Contains(err.Error(), "panic while closing session") {
			t.Fatalf("servePreparedConnection() error = %v, want recovered close panic", err)
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
	if streamnet.IsClosed(errors.Join(errors.New("decode failed"), net.ErrClosed)) {
		t.Fatal("mixed joined error classified as ordinary")
	}
	if !streamnet.IsClosed(errors.Join(fmt.Errorf("wrapped: %w", io.EOF), context.Canceled, net.ErrClosed)) {
		t.Fatal("all-ordinary joined error classified as non-ordinary")
	}
}

func TestIsOrdinaryCloseRecognizesClassifiedTerminalTransportError(t *testing.T) {
	framed := streamnet.NewFramedConn(&terminalWriteConn{err: io.ErrClosedPipe})
	_, err := framed.Write([]byte{0xfe})
	if !streamnet.IsClosed(err) {
		t.Fatalf("classified terminal transport error considered non-ordinary: %v", err)
	}
	if streamnet.IsClosed(errors.Join(errors.New("decode failed"), err)) {
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
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() {
		done <- Serve(ctx, Config{SocketDir: dir, Upstream: "127.0.0.1:1", Logger: logger})
	}()

	readyCtx, stopWaiting := context.WithTimeout(context.Background(), 2*time.Second)
	defer stopWaiting()
	if !output.waitFor(readyCtx, "msg=\"listener ready; waiting for local Rust client\"") {
		cancel()
		select {
		case serveErr := <-done:
			t.Fatalf("Serve() stopped before reporting listener readiness: %v", serveErr)
		default:
			t.Fatalf("proxy listener was not ready:\n%s", output.String())
		}
	}
	networkName, address, err := streamnet.Resolve(dir)
	if err != nil {
		cancel()
		t.Fatalf("resolve ready proxy endpoint: %v", err)
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

	readyCtx, stopWaiting := context.WithTimeout(context.Background(), 2*time.Second)
	defer stopWaiting()
	if !output.waitFor(readyCtx, "msg=\"listener ready; waiting for local Rust client\"") {
		select {
		case err := <-done:
			t.Fatalf("Serve() stopped before reporting readiness: %v", err)
		default:
			t.Fatalf("Serve() did not report readiness:\n%s", output.String())
		}
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

func TestReportLocalClientAcceptedIncludesCapabilities(t *testing.T) {
	var output lockedBuffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	reportLocalClientAccepted(logger, "run/socket", true)
	if got := output.String(); !strings.Contains(got, "msg=\"local client accepted\" socket_dir=run/socket client_blob_cache=true") {
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

// Chat must carry the authenticated upstream identity even through the observing wrappers.
func TestRelayRewritesChatIdentityThroughSessionWrappers(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	up.identity = login.IdentityData{DisplayName: "Canonical", XUID: "2535"}
	wrapped := observeDisconnects(observeTransfers(up, new(TransferState), nil), func(DisconnectInfo) {})
	down.useBatchReads = true
	down.batchReads <- batchResult{packets: []packet.Packet{&packet.Text{TextType: packet.TextTypeChat, SourceName: "offline", XUID: "1", Message: "hi"}}}
	down.batchReads <- batchResult{err: io.EOF}

	if err := pumpPackets(down, wrapped, true); !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	batches := up.flushedBatches()
	text, ok := batches[0][0].(*packet.Text)
	if !ok || text.SourceName != "Canonical" || text.XUID != "2535" {
		t.Fatalf("forwarded chat = %#v", batches[0][0])
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
	spawn    func(context.Context) error
	data     minecraft.GameData
	packs    []*resource.Pack
	required bool
	identity login.IdentityData
}

func newFakeUpstream(spawn func(context.Context) error) *fakeUpstream {
	if spawn == nil {
		spawn = func(context.Context) error { return nil }
	}
	return &fakeUpstream{fakeSession: newFakeSession(), spawn: spawn, data: minecraft.GameData{EntityRuntimeID: 9}}
}

func (s *fakeUpstream) DoSpawnContext(ctx context.Context) error { return s.spawn(ctx) }
func (s *fakeUpstream) GameData() minecraft.GameData             { return s.data }
func (s *fakeUpstream) ResourcePacks() []*resource.Pack          { return slices.Clone(s.packs) }
func (s *fakeUpstream) TexturePacksRequired() bool               { return s.required }
func (s *fakeUpstream) IdentityData() login.IdentityData         { return s.identity }

type errorCloser struct{ err error }

func (c errorCloser) Close() error { return c.err }

type cacheStatusScriptedNetwork struct {
	script func(net.Conn) error
	done   chan error
}

func newCacheStatusScriptedNetwork(script func(net.Conn) error) *cacheStatusScriptedNetwork {
	return &cacheStatusScriptedNetwork{script: script, done: make(chan error, 1)}
}

func (network *cacheStatusScriptedNetwork) DialContext(context.Context, string) (net.Conn, error) {
	client, server := net.Pipe()
	go func() {
		defer server.Close()
		network.done <- network.script(server)
	}()
	return client, nil
}

func (*cacheStatusScriptedNetwork) PingContext(context.Context, string) ([]byte, error) {
	return nil, errors.New("not implemented")
}

func (*cacheStatusScriptedNetwork) Listen(string) (minecraft.NetworkListener, error) {
	return nil, errors.New("not implemented")
}

func encodeCacheStatusScriptedPackets(encoder *packet.Encoder, packets ...packet.Packet) error {
	encoded := make([][]byte, 0, len(packets))
	for _, value := range packets {
		buffer := new(bytes.Buffer)
		if err := (&packet.Header{PacketID: value.ID()}).Write(buffer); err != nil {
			return err
		}
		value.Marshal(minecraft.DefaultProtocol.NewWriter(buffer, 0))
		encoded = append(encoded, buffer.Bytes())
	}
	return encoder.Encode(encoded)
}

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

// relayFixtureStartup is a recorded-session-shaped startup carrying StartGame fields GameData drops.
func relayFixtureStartup() []packet.Packet {
	return []packet.Packet{
		&packet.StartGame{
			WorldName: "Fixture", LevelID: "fixture-level", ServerID: "server-id", WorldID: "world-id",
			ScenarioID: "scenario", OwnerID: "owner", EntityUniqueID: 42, EntityRuntimeID: 42,
			BaseGameVersion: "1.26.50", GameVersion: "1.26.50", Trial: true, EducationFeaturesEnabled: true,
			TemplateContentIdentity: "template", EnchantmentSeed: 1234, MultiPlayerCorrelationID: "correlation",
		},
		&packet.ItemRegistry{Items: []protocol.ItemEntry{{Name: "minecraft:shield", RuntimeID: 355}}},
		&packet.CreativeContent{},
		&packet.ChunkRadiusUpdated{ChunkRadius: 7},
		&packet.PlayStatus{Status: packet.PlayStatusPlayerSpawn},
	}
}

// The client receives the upstream startup byte for byte, owns the spawn sequence alone (one chunk radius
// request with its own radius, one loading-screen pair, one initialisation), and the core adds nothing.
func TestRelayForwardsTheUpstreamStartupLosslessly(t *testing.T) {
	var mu sync.Mutex
	upstreamSent := map[uint32][]byte{}
	var upstreamReceived []packet.Packet
	upstreamNetwork := streamnet.New(filepath.Join(t.TempDir(), "upstream"))
	upstreamListener, err := minecraft.ListenConfig{
		AuthenticationDisabled: true,
		ErrorLog:               slog.New(slog.DiscardHandler),
		PacketFunc: func(header packet.Header, payload []byte, src, _ net.Addr) {
			if header.PacketID == packet.IDStartGame || header.PacketID == packet.IDItemRegistry || header.PacketID == packet.IDCreativeContent {
				mu.Lock()
				if _, seen := upstreamSent[header.PacketID]; !seen {
					upstreamSent[header.PacketID] = bytes.Clone(payload)
				}
				mu.Unlock()
			}
		},
	}.ListenNetwork(upstreamNetwork, "")
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = upstreamListener.Close() })
	go func() {
		accepted, err := upstreamListener.Accept()
		if err != nil {
			return
		}
		conn := accepted.(*minecraft.Conn)
		_ = conn.WritePacketImmediate(relayFixtureStartup()...)
		for {
			pk, err := conn.ReadPacket()
			if err != nil {
				return
			}
			mu.Lock()
			upstreamReceived = append(upstreamReceived, pk)
			mu.Unlock()
		}
	}()

	connections := newPreparedConnections("unused.invalid:19132", nil, slog.New(slog.DiscardHandler))
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{network: upstreamNetwork}, nil
	}
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		return dialer.DialContextNetwork(ctx, target.network, "")
	}
	listener, network := newAdmissionTestListener(t, connections.prepare)
	ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
	defer cancel()
	go func() {
		accepted, err := listener.Accept()
		if err != nil {
			return
		}
		downstream := accepted.(*minecraft.Conn)
		prepared, err := takePreparedAfterAccept(connections, downstream)
		if err != nil || prepared == nil {
			return
		}
		_ = servePreparedConnection(ctx, downstream, prepared)
	}()

	clientReceived := map[uint32][]byte{}
	client, err := minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "RustClient"},
		Protocol:     minecraft.DefaultProtocol,
		RelayStartup: true,
		PacketFunc: func(header packet.Header, payload []byte, src, _ net.Addr) {
			mu.Lock()
			if _, seen := clientReceived[header.PacketID]; !seen {
				clientReceived[header.PacketID] = bytes.Clone(payload)
			}
			mu.Unlock()
		},
	}.DialContextNetwork(ctx, network, "")
	if err != nil {
		t.Fatalf("client dial: %v", err)
	}
	defer client.Close()
	for range relayFixtureStartup() {
		if _, err := client.ReadPacket(); err != nil {
			t.Fatalf("read startup: %v", err)
		}
	}
	_ = client.WritePacket(&packet.RequestChunkRadius{ChunkRadius: 7, MaxChunkRadius: 7})
	_ = client.WritePacket(&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeStart})
	_ = client.WritePacket(&packet.ServerBoundLoadingScreen{Type: packet.LoadingScreenTypeEnd})
	_ = client.WritePacket(&packet.SetLocalPlayerAsInitialised{EntityRuntimeID: 42})
	_ = client.Flush()

	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		mu.Lock()
		n := len(upstreamReceived)
		mu.Unlock()
		if n >= 4 {
			break
		}
		time.Sleep(10 * time.Millisecond)
	}
	time.Sleep(200 * time.Millisecond) // let any stray duplicate arrive
	mu.Lock()
	defer mu.Unlock()
	for _, id := range []uint32{packet.IDStartGame, packet.IDItemRegistry, packet.IDCreativeContent} {
		if sent, got := upstreamSent[id], clientReceived[id]; sent == nil || !bytes.Equal(sent, got) {
			t.Fatalf("packet %d reached the client changed (%d upstream bytes, %d client bytes)", id, len(sent), len(got))
		}
	}
	var radii []int32
	loading, initialised := 0, 0
	for _, pk := range upstreamReceived {
		switch pk := pk.(type) {
		case *packet.RequestChunkRadius:
			radii = append(radii, pk.ChunkRadius)
		case *packet.ServerBoundLoadingScreen:
			loading++
		case *packet.SetLocalPlayerAsInitialised:
			initialised++
		}
	}
	if !slices.Equal(radii, []int32{7}) || loading != 2 || initialised != 1 {
		t.Fatalf("upstream saw radii=%v loading=%d initialised=%d, want only the client's single sequence", radii, loading, initialised)
	}
}

// The private listener negotiates no compression, so local batches are neither compressed nor decompressed.
func TestLocalListenerNegotiatesNoCompression(t *testing.T) {
	network := streamnet.New(filepath.Join(t.TempDir(), "local"))
	config := localListenConfig(nil)
	config.ErrorLog = slog.New(slog.DiscardHandler)
	listener, err := config.ListenNetwork(network, "")
	if err != nil {
		t.Fatal(err)
	}
	defer listener.Close()
	go func() {
		if conn, err := listener.Accept(); err == nil {
			_ = conn.(*minecraft.Conn).StartGame(minecraft.GameData{EntityRuntimeID: 1})
		}
	}()
	settings := make(chan uint16, 1)
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	conn, err := minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "Local"},
		PacketFunc: func(header packet.Header, payload []byte, _, _ net.Addr) {
			if header.PacketID == packet.IDNetworkSettings {
				var pk packet.NetworkSettings
				pk.Marshal(minecraft.DefaultProtocol.NewReader(bytes.NewBuffer(payload), 0, false))
				settings <- pk.CompressionAlgorithm
			}
		},
	}.DialContextNetwork(ctx, network, "")
	if err != nil {
		t.Fatalf("dial local listener: %v", err)
	}
	_ = conn.Close()
	if got := <-settings; got != packet.CompressionAlgorithmNone {
		t.Fatalf("local NetworkSettings compression = %#x, want none (%#x)", got, packet.CompressionAlgorithmNone)
	}
}

// BenchmarkLocalLegCompression measures the per-MB encode and decode work each side of the local leg does.
func BenchmarkLocalLegCompression(b *testing.B) {
	payload := make([]byte, 1<<20)
	for index := range payload {
		payload[index] = byte(index*31) ^ byte(index>>9) // skin- and chunk-like: structured, partly compressible
	}
	for _, test := range []struct {
		name        string
		compression packet.Compression
	}{{"deflate", packet.FlateCompression}, {"none", packet.NopCompression}} {
		b.Run(test.name, func(b *testing.B) {
			var wire bytes.Buffer
			encoder, decoder := packet.NewEncoder(&wire), packet.NewDecoder(&wire)
			encoder.EnableCompression(test.compression, 256)
			decoder.EnableCompression(test.compression, math.MaxInt)
			b.SetBytes(int64(len(payload)))
			b.ReportAllocs()
			for b.Loop() {
				if err := encoder.Encode([][]byte{payload}); err != nil {
					b.Fatal(err)
				}
				if _, err := decoder.Decode(); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}

// newUpstreamDialer exercises the production constructor with default policy.
func newUpstreamDialer(downstream dialerDownstream, tokens oauth2.TokenSource) minecraft.Dialer {
	return newUpstreamDialerForAdmission(downstream, tokens, nil, nil, false)
}

// relayWithSessions provides the relay's close callback for packet-only test doubles.
func relayWithSessions(ctx context.Context, downstream, upstream packetSession) (err error) {
	var closeErr error
	err = relayPackets(ctx, downstream, upstream, func() {
		closeErr = errors.Join(shutdownSession(downstream), shutdownSession(upstream))
	})
	return errors.Join(err, closeErr)
}
