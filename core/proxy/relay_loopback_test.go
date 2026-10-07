package proxy

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"math"
	"math/rand/v2"
	"net"
	"path/filepath"
	"reflect"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// relayLoopback is a scripted upstream server and a client joined through the production relay.
type relayLoopback struct {
	server, client *minecraft.Conn
	transfers      *TransferState
	disconnects    chan DisconnectInfo
}

// relayFunc serves one accepted session; production is servePreparedConnection.
type relayFunc func(ctx context.Context, downstream downstreamSession, prepared *preparedConnection) error

// socketNetworks joins both legs over the production local socket transport.
func socketNetworks(tb testing.TB) (upstream, local minecraft.Network) {
	return streamnet.New(filepath.Join(tb.TempDir(), "upstream")), streamnet.New(filepath.Join(tb.TempDir(), "local"))
}

// memoryNetworks joins both legs in memory, leaving only codec and relay work to measure.
func memoryNetworks(testing.TB) (upstream, local minecraft.Network) {
	return newMemoryNetwork(), newMemoryNetwork()
}

func startRelayLoopback(tb testing.TB, networks func(testing.TB) (minecraft.Network, minecraft.Network), relay relayFunc) *relayLoopback {
	tb.Helper()
	ctx, cancel := context.WithCancel(context.Background())
	tb.Cleanup(cancel)
	quiet := slog.New(slog.DiscardHandler)
	upstreamNetwork, localNetwork := networks(tb)
	upstreamListener, err := minecraft.ListenConfig{
		AuthenticationDisabled: true,
		AllowUnknownPackets:    true,
		EnableBatchReading:     true,
		FlushRate:              -1,
		// Negotiated but never applied: the relay, not DEFLATE, is under test.
		CompressionThreshold:    math.MaxInt32,
		DisablePacketEncryption: true,
		ErrorLog:                quiet,
	}.ListenNetwork(upstreamNetwork, "")
	if err != nil {
		tb.Fatal(err)
	}
	tb.Cleanup(func() { _ = upstreamListener.Close() })
	accepted := make(chan *minecraft.Conn, 1)
	go func() {
		conn, err := upstreamListener.Accept()
		if err != nil {
			return
		}
		server := conn.(*minecraft.Conn)
		_ = server.WritePacketImmediate(relayFixtureStartup()...)
		accepted <- server
	}()

	loopback := &relayLoopback{transfers: new(TransferState), disconnects: make(chan DisconnectInfo, 1)}
	connections := newPreparedConnections("unused.invalid:19132", nil, quiet)
	connections.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{network: upstreamNetwork}, nil
	}
	connections.dialTarget = func(ctx context.Context, target *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		return dialer.DialContextNetwork(ctx, target.network, "")
	}
	config := localListenConfig(connections.prepare)
	config.ErrorLog = quiet
	listener, err := config.ListenNetwork(localNetwork, "")
	if err != nil {
		tb.Fatal(err)
	}
	tb.Cleanup(func() { _ = listener.Close() })
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			return
		}
		downstream := conn.(*minecraft.Conn)
		prepared, err := takePreparedAfterAccept(connections, downstream)
		if err != nil || prepared == nil {
			_ = downstream.Close()
			return
		}
		// The wrappers Serve installs after the handoff.
		prepared.upstream = observeDisconnects(observeTransfers(prepared.upstream, loopback.transfers, quiet), func(info DisconnectInfo) {
			loopback.disconnects <- info
		})
		_ = relay(ctx, downstream, prepared)
	}()

	dialCtx, dialCancel := context.WithTimeout(ctx, 10*time.Second)
	defer dialCancel()
	client, err := minecraft.Dialer{
		IdentityData:       login.IdentityData{DisplayName: "RustClient"},
		Protocol:           minecraft.DefaultProtocol,
		Handoff:            minecraft.HandoffAtStartGame,
		EnableBatchReading: true,
		FlushRate:          -1,
		ErrorLog:           quiet,
	}.DialContextNetwork(dialCtx, localNetwork, "")
	if err != nil {
		tb.Fatalf("client dial: %v", err)
	}
	tb.Cleanup(func() { _ = client.Close() })
	select {
	case loopback.server = <-accepted:
	case <-dialCtx.Done():
		tb.Fatal("upstream never accepted the relay")
	}
	tb.Cleanup(func() { _ = loopback.server.Close() })
	// Drain the startup so tests see only their own batches.
	for received := 0; received < len(relayFixtureStartup()); {
		batch, err := client.ReadBatchRaw(nil)
		if err != nil {
			tb.Fatalf("read startup: %v", err)
		}
		received += len(batch)
	}
	loopback.client = client
	return loopback
}

// sendRaw writes frames as one batch.
func sendRaw(tb testing.TB, conn *minecraft.Conn, frames ...[]byte) {
	tb.Helper()
	for _, frame := range frames {
		if err := conn.WritePacketRaw(frame); err != nil {
			tb.Fatal(err)
		}
	}
	if err := conn.Flush(); err != nil {
		tb.Fatal(err)
	}
}

func readRawBatch(tb testing.TB, conn *minecraft.Conn) [][]byte {
	tb.Helper()
	batch, err := conn.ReadBatchRaw(nil)
	if err != nil {
		tb.Fatalf("read batch: %v", err)
	}
	frames := make([][]byte, len(batch))
	for index, raw := range batch {
		frames[index] = raw.Data
	}
	return frames
}

func assertFrames(tb testing.TB, got, want [][]byte) {
	tb.Helper()
	if len(got) != len(want) {
		tb.Fatalf("batch has %d packets, want %d", len(got), len(want))
	}
	for index := range want {
		if !bytes.Equal(got[index], want[index]) {
			tb.Fatalf("packet %d changed in relay: %d bytes %x..., want %d bytes %x...", index, len(got[index]), head(got[index]), len(want[index]), head(want[index]))
		}
	}
}

func head(frame []byte) []byte { return frame[:min(len(frame), 16)] }

// withSubclients re-encodes frame's header with sender and target subclient IDs.
func withSubclients(frame []byte, sender, target byte) []byte {
	buf := bytes.NewBuffer(frame)
	var header packet.Header
	_ = header.Read(buf)
	header.SenderSubClient, header.TargetSubClient = sender, target
	out := new(bytes.Buffer)
	_ = header.Write(out)
	out.Write(buf.Bytes())
	return out.Bytes()
}

func chunkFrame(x int32, payload []byte) []byte {
	return encodeTestPacket(&packet.LevelChunk{Position: protocol.ChunkPos{x, -x}, SubChunkCount: 24, RawPayload: payload})
}

func chunkPayload(size int) []byte {
	payload := make([]byte, size)
	random := rand.NewChaCha8([32]byte{7})
	_, _ = random.Read(payload)
	return payload
}

// Packets the proxy does not inspect cross both legs byte for byte, headers included.
func TestRelayLoopbackForwardsUntouchedPacketsByteForByte(t *testing.T) {
	loopback := startRelayLoopback(t, socketNetworks, servePreparedConnection)
	inbound := [][]byte{
		chunkFrame(3, chunkPayload(16<<10)),
		encodeTestPacket(&packet.Unknown{PacketID: 1000, Payload: []byte{0, 1, 2, 0xff}}),
		withSubclients(encodeTestPacket(&packet.SetTime{Time: 6000}), 1, 2),
		encodeTestPacket(&packet.Text{TextType: packet.TextTypeChat, SourceName: "Server", XUID: "9", Message: "kept"}),
	}
	sendRaw(t, loopback.server, inbound...)
	assertFrames(t, readRawBatch(t, loopback.client), inbound)

	outbound := [][]byte{
		encodeTestPacket(&packet.NetworkStackLatency{Timestamp: 11, NeedsResponse: true}),
		encodeTestPacket(&packet.Unknown{PacketID: 1001, Payload: []byte{9, 9}}),
		encodeTestPacket(&packet.Text{TextType: packet.TextTypeRaw, SourceName: "client", Message: "not chat"}),
	}
	sendRaw(t, loopback.client, outbound...)
	assertFrames(t, readRawBatch(t, loopback.server), outbound)
}

// Mixed batches keep their order and boundaries while chat is rewritten and transfers are recorded.
func TestRelayLoopbackHandlesInspectedPacketsInOrder(t *testing.T) {
	loopback := startRelayLoopback(t, socketNetworks, servePreparedConnection)
	payload := chunkPayload(4 << 10)
	first := [][]byte{
		chunkFrame(0, payload),
		encodeTestPacket(&packet.Transfer{Address: "next.example.test", Port: 19133}),
		encodeTestPacket(&packet.SetTime{Time: 1}),
	}
	second := [][]byte{encodeTestPacket(&packet.SetTime{Time: 2}), chunkFrame(1, payload)}
	sendRaw(t, loopback.server, first...)
	sendRaw(t, loopback.server, second...)
	assertFrames(t, readRawBatch(t, loopback.client), first)
	assertFrames(t, readRawBatch(t, loopback.client), second)
	if target, ok := loopback.transfers.Pending(); !ok || target != "next.example.test:19133" {
		t.Fatalf("pending transfer = %q, %t; want the relayed Transfer", target, ok)
	}

	before := encodeTestPacket(&packet.NetworkStackLatency{Timestamp: 1})
	after := encodeTestPacket(&packet.Unknown{PacketID: 1002, Payload: []byte{3}})
	chat := encodeTestPacket(&packet.Text{TextType: packet.TextTypeChat, SourceName: "spoofed", XUID: "1", Message: "hello"})
	sendRaw(t, loopback.client, before, chat, after)
	got := readRawBatch(t, loopback.server)
	if len(got) != 3 || !bytes.Equal(got[0], before) || !bytes.Equal(got[2], after) {
		t.Fatalf("outbound batch = %d packets, want the untouched neighbours in place", len(got))
	}
	rewritten := packetFromRaw(got[1])
	want := &packet.Text{TextType: packet.TextTypeChat, SourceName: "RustClient", Message: "hello"}
	if !reflect.DeepEqual(rewritten, want) {
		t.Fatalf("chat reached upstream as %#v, want %#v", rewritten, want)
	}
}

// A server disconnect arrives after the batch before it, then ends the client session with its reason.
func TestRelayLoopbackDeliversDisconnectAfterPrecedingBatch(t *testing.T) {
	loopback := startRelayLoopback(t, socketNetworks, servePreparedConnection)
	before := encodeTestPacket(&packet.SetTime{Time: 3})
	sendRaw(t, loopback.server, before)
	if err := loopback.server.Disconnect("server closing"); err != nil {
		t.Fatal(err)
	}
	assertFrames(t, readRawBatch(t, loopback.client), [][]byte{before})
	_, err := loopback.client.ReadBatchRaw(nil)
	var disconnect *minecraft.DisconnectPacketError
	if !errors.As(err, &disconnect) || disconnect.Message != "server closing" {
		t.Fatalf("client read error = %v, want the server's disconnect", err)
	}
	select {
	case info := <-loopback.disconnects:
		if info.Message != "server closing" {
			t.Fatalf("reported disconnect %q", info.Message)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("disconnect was not reported")
	}
}

// decodedRelay is the relay before raw forwarding: every packet decoded and re-encoded.
func decodedRelay(ctx context.Context, downstream downstreamSession, prepared *preparedConnection) error {
	session := prepared.upstream
	for {
		if observing, ok := session.(*disconnectObservingSession); ok {
			session = observing.upstreamSession
		} else if observing, ok := session.(*transferObservingSession); ok {
			session = observing.upstreamSession
		} else {
			break
		}
	}
	upstream := session.(*minecraft.Conn)
	pump := func(source, destination *minecraft.Conn) {
		for {
			batch, err := source.ReadBatch()
			if err != nil {
				return
			}
			for _, value := range batch {
				_ = destination.WritePacket(value)
			}
			_ = destination.Flush()
		}
	}
	go pump(downstream.(*minecraft.Conn), upstream)
	pump(upstream, downstream.(*minecraft.Conn))
	<-ctx.Done()
	return nil
}

// Run: go test ./proxy -run '^$' -bench RelayLoopbackChunks -benchmem -cpu 1,12
// Chunk batches from upstream to client through both real Conns, against the decoding relay; at -cpu 1
// ns/op is the whole process's CPU per batch, of which only the relay differs.
func BenchmarkRelayLoopbackChunks(b *testing.B) {
	const chunksPerBatch, chunkBytes = 16, 16 << 10
	payload := chunkPayload(chunkBytes)
	frames := make([][]byte, chunksPerBatch)
	for index := range frames {
		frames[index] = chunkFrame(int32(index), payload)
	}
	for _, variant := range []struct {
		name  string
		relay relayFunc
	}{{"relay=raw", servePreparedConnection}, {"relay=decoded", decodedRelay}} {
		b.Run(variant.name, func(b *testing.B) {
			loopback := startRelayLoopback(b, memoryNetworks, variant.relay)
			b.SetBytes(chunksPerBatch * chunkBytes)
			b.ReportAllocs()
			b.ResetTimer()
			go func() {
				for range b.N {
					for _, frame := range frames {
						_ = loopback.server.WritePacketRaw(frame)
					}
					if loopback.server.Flush() != nil {
						return
					}
				}
			}()
			for received := 0; received < b.N*chunksPerBatch; {
				batch, err := loopback.client.ReadBatchRaw(nil)
				if err != nil {
					b.Fatal(err)
				}
				received += len(batch)
			}
		})
	}
}

// memoryNetwork is a framed in-memory transport: each Write arrives as one ReadPacket.
type memoryNetwork struct{ accept chan net.Conn }

func newMemoryNetwork() *memoryNetwork { return &memoryNetwork{accept: make(chan net.Conn)} }

func (n *memoryNetwork) DialContext(ctx context.Context, _ string) (net.Conn, error) {
	client, server := newMemoryConnPair()
	select {
	case n.accept <- server:
		return client, nil
	case <-ctx.Done():
		return nil, ctx.Err()
	}
}

func (*memoryNetwork) PingContext(context.Context, string) ([]byte, error) { return nil, nil }

func (n *memoryNetwork) Listen(string) (minecraft.NetworkListener, error) {
	return &memoryListener{network: n, closed: make(chan struct{})}, nil
}

type memoryListener struct {
	network   *memoryNetwork
	closed    chan struct{}
	closeOnce sync.Once
}

func (l *memoryListener) Accept() (net.Conn, error) {
	select {
	case conn := <-l.network.accept:
		return conn, nil
	case <-l.closed:
		return nil, net.ErrClosed
	}
}

func (l *memoryListener) Close() error {
	l.closeOnce.Do(func() { close(l.closed) })
	return nil
}

func (*memoryListener) Addr() net.Addr  { return memoryAddr{} }
func (*memoryListener) ID() int64       { return 1 }
func (*memoryListener) PongData([]byte) {}

type memoryAddr struct{}

func (memoryAddr) Network() string { return "memory" }
func (memoryAddr) String() string  { return "memory" }

// memoryConn is one end of a pair; a closed pair fails both ends.
type memoryConn struct {
	in, out chan []byte
	closed  chan struct{}
	once    *sync.Once
}

func newMemoryConnPair() (*memoryConn, *memoryConn) {
	ab, ba, closed, once := make(chan []byte, 4), make(chan []byte, 4), make(chan struct{}), new(sync.Once)
	return &memoryConn{in: ba, out: ab, closed: closed, once: once}, &memoryConn{in: ab, out: ba, closed: closed, once: once}
}

func (c *memoryConn) ReadPacket() ([]byte, error) {
	select {
	case frame := <-c.in:
		return frame, nil
	case <-c.closed:
		return nil, net.ErrClosed
	}
}

func (c *memoryConn) Read(b []byte) (int, error) {
	frame, err := c.ReadPacket()
	return copy(b, frame), err
}

func (c *memoryConn) Write(b []byte) (int, error) {
	select {
	case c.out <- bytes.Clone(b):
		return len(b), nil
	case <-c.closed:
		return 0, net.ErrClosed
	}
}

func (c *memoryConn) Close() error {
	c.once.Do(func() { close(c.closed) })
	return nil
}

func (*memoryConn) LocalAddr() net.Addr              { return memoryAddr{} }
func (*memoryConn) RemoteAddr() net.Addr             { return memoryAddr{} }
func (*memoryConn) SetDeadline(time.Time) error      { return nil }
func (*memoryConn) SetReadDeadline(time.Time) error  { return nil }
func (*memoryConn) SetWriteDeadline(time.Time) error { return nil }
