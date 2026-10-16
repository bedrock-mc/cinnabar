package proxy

import (
	"context"
	"encoding/binary"
	"hash/fnv"
	"net"
	"sync"
	"sync/atomic"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/latencytrace"
	"github.com/sandertv/go-raknet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// traceRoundTripInterval spaces RakNet RTT samples in the trace.
const traceRoundTripInterval = time.Second

// tracingDialer opens upstream UDP sockets that record each datagram and the RakNet frames in it.
type tracingDialer struct{ trace *latencytrace.Trace }

func (dialer tracingDialer) DialContext(ctx context.Context, network, address string) (net.Conn, error) {
	conn, err := (&net.Dialer{}).DialContext(ctx, network, address)
	if err != nil {
		return nil, err
	}
	return &tracedDatagrams{Conn: conn, trace: dialer.trace}, nil
}

type tracedDatagrams struct {
	net.Conn
	trace *latencytrace.Trace
}

func (conn *tracedDatagrams) Read(b []byte) (int, error) {
	n, err := conn.Conn.Read(b)
	if n > 0 {
		traceDatagram(conn.trace, latencytrace.UDPRead, 0, b[:n])
	}
	return n, err
}

func (conn *tracedDatagrams) Write(b []byte) (int, error) {
	n, err := conn.Conn.Write(b)
	if n > 0 {
		traceDatagram(conn.trace, latencytrace.UDPWrite, 1, b[:n])
	}
	return n, err
}

// traceDatagram records a datagram and, for a connected data datagram, each RakNet frame in it.
func traceDatagram(trace *latencytrace.Trace, kind byte, direction int64, datagram []byte) {
	now := trace.Now()
	sequence := int64(-1)
	const valid, ack, nack = 0x80, 0x40, 0x20
	data := datagram[0]&valid != 0 && datagram[0]&(ack|nack) == 0 && len(datagram) >= 4
	if data {
		sequence = int64(uint32(datagram[1]) | uint32(datagram[2])<<8 | uint32(datagram[3])<<16)
	}
	trace.Record(kind, now, int64(len(datagram)), int64(datagram[0]), sequence)
	if !data {
		return
	}
	for _, frame := range rakNetFrames(datagram[4:]) {
		trace.Record(latencytrace.Frame, direction, sequence, int64(frame.key), frame.splitID, frame.splitIndex, frame.splitCount)
	}
}

// rakNetFrame is one encapsulated frame's identity; key is set where the frame starts a message.
type rakNetFrame struct {
	key                             uint64
	splitID, splitIndex, splitCount int64
}

// rakNetFrames reads the frame headers of a data datagram's body, stopping at a malformed one.
func rakNetFrames(body []byte) (frames []rakNetFrame) {
	for len(body) >= 3 {
		flags := body[0]
		reliability := flags >> 5
		length := (int(binary.BigEndian.Uint16(body[1:3])) + 7) / 8
		offset := 3
		if reliability == 2 || reliability == 3 || reliability == 4 || reliability == 6 || reliability == 7 {
			offset += 3 // message index
		}
		if reliability == 1 || reliability == 4 {
			offset += 3 // sequence index
		}
		if reliability == 1 || reliability == 3 || reliability == 4 || reliability == 7 {
			offset += 4 // order index and channel
		}
		frame := rakNetFrame{splitID: -1}
		if flags&0x10 != 0 {
			if len(body) < offset+10 {
				return frames
			}
			frame.splitCount = int64(binary.BigEndian.Uint32(body[offset:]))
			frame.splitID = int64(binary.BigEndian.Uint16(body[offset+4:]))
			frame.splitIndex = int64(binary.BigEndian.Uint32(body[offset+6:]))
			offset += 10
		}
		if len(body) < offset+length {
			return frames
		}
		if frame.splitIndex == 0 {
			frame.key = messageKey(body[offset : offset+length])
		}
		frames = append(frames, frame)
		body = body[offset+length:]
	}
	return frames
}

// messageKey digests a RakNet message's first 16 bytes, which a message's first frame also
// starts with; game messages are encrypted, so the digest identifies without exposing content.
func messageKey(message []byte) uint64 {
	digest := fnv.New64a()
	_, _ = digest.Write(message[:min(len(message), 16)])
	return digest.Sum64() >> 1 // a non-negative trace value
}

// upstreamTrace links one upstream connection's RakNet messages to the batches the relay reads.
// After StartGame, gophertunnel queues the packets it accepted from each message as one batch,
// in order. Startup batches mix packets deferred from several login messages, so they are traced
// with an unknown origin and every message through StartGame is dropped once they are read.
type upstreamTrace struct {
	accepted     atomic.Int64 // packets accepted from the message being handled
	sawStartGame atomic.Bool  // the message being handled holds StartGame

	mu        sync.Mutex
	queued    []tracedMessage // handled messages whose batch has not been fully read, oldest first
	current   tracedMessage   // the message being handled
	startupAt bool            // startup was read before the StartGame message finished
	lastOut   uint64          // key of the last message written upstream
}

type tracedMessage struct {
	key       uint64
	startGame bool
	taken     int64 // packets already read from its batch
	packets   int64 // packets accepted, once known
}

// unknownOrigin is the message key of a batch whose source message is not tracked.
const unknownOrigin = -1

type upstreamTraceKey struct{}

// upstreamTraces finds a dialled upstream's trace from the session the relay reads.
var upstreamTraces sync.Map // packetSession -> *upstreamTrace

// traceUpstream prepares one upstream dial's trace when tracing is on: the dialer counts the
// packets it accepts, and the returned context hands the trace to the RakNet transport.
func traceUpstream(ctx context.Context, dialer minecraft.Dialer) (context.Context, minecraft.Dialer, *upstreamTrace) {
	if latencytrace.Active() == nil {
		return ctx, dialer, nil
	}
	upstream := new(upstreamTrace)
	accept := dialer.AcceptPacketHeader
	dialer.AcceptPacketHeader = func(header packet.Header) bool {
		accepted := accept == nil || accept(header)
		if accepted {
			upstream.accepted.Add(1)
			if header.PacketID == packet.IDStartGame {
				upstream.sawStartGame.Store(true)
			}
		}
		return accepted
	}
	return context.WithValue(ctx, upstreamTraceKey{}, upstream), dialer, upstream
}

// register lets the relay find this trace from the dialled session until the session closes.
func (upstream *upstreamTrace) register(session upstreamSession) {
	if upstream == nil || session == nil {
		return
	}
	upstreamTraces.Store(session, upstream)
	if closing, ok := session.(interface{ Context() context.Context }); ok {
		context.AfterFunc(closing.Context(), func() { upstreamTraces.Delete(session) })
	}
}

// lookupUpstreamTrace returns the trace of the dialled session beneath session's observers.
func lookupUpstreamTrace(session packetSession) *upstreamTrace {
	base := unwrapUpstream(session)
	if base == nil {
		return nil
	}
	if upstream, ok := upstreamTraces.Load(base); ok {
		return upstream.(*upstreamTrace)
	}
	return nil
}

// begin starts tracking the message gophertunnel is about to handle.
func (upstream *upstreamTrace) begin(key uint64) {
	upstream.mu.Lock()
	upstream.current = tracedMessage{key: key}
	upstream.mu.Unlock()
}

// finish closes the message being handled, returning how many packets were accepted from it.
func (upstream *upstreamTrace) finish() int64 {
	packets := upstream.accepted.Swap(0)
	startGame := upstream.sawStartGame.Swap(false)
	upstream.mu.Lock()
	defer upstream.mu.Unlock()
	message := upstream.current
	message.packets, message.startGame = packets, startGame
	switch {
	case upstream.startupAt:
		// Startup was read before this message finished: drop it, and everything through
		// StartGame, from the play-phase queue.
		upstream.startupAt = !startGame
	case message.packets > message.taken:
		upstream.queued = append(upstream.queued, message)
	}
	upstream.current = tracedMessage{}
	return packets
}

// startupRead drops every message through StartGame once the session startup has read the batch
// holding it; those batches mixed deferred login packets.
func (upstream *upstreamTrace) startupRead() {
	if upstream == nil {
		return
	}
	upstream.mu.Lock()
	defer upstream.mu.Unlock()
	for index, message := range upstream.queued {
		if message.startGame {
			upstream.queued = upstream.queued[index+1:]
			return
		}
	}
	upstream.queued = upstream.queued[:0]
	upstream.startupAt = true
}

// batchMessage attributes a batch of n packets read from the upstream to its message's key.
func (upstream *upstreamTrace) batchMessage(n int) uint64 {
	if upstream == nil {
		return 0
	}
	upstream.mu.Lock()
	defer upstream.mu.Unlock()
	if len(upstream.queued) != 0 {
		head := &upstream.queued[0]
		key := head.key
		if head.taken += int64(n); head.taken >= head.packets {
			upstream.queued = upstream.queued[1:]
		}
		return key
	}
	// The batch was queued before the receive goroutine finished its message.
	upstream.current.taken += int64(n)
	return upstream.current.key
}

// tracedRakNet records when each RakNet message reaches gophertunnel and when its batch was
// handled, which is when gophertunnel's single reader asks for the next message.
type tracedRakNet struct {
	*raknet.Conn
	trace    *latencytrace.Trace
	upstream *upstreamTrace
	arrived  int64
	size     int64
	key      uint64
}

// traceRakNet wraps conn when tracing is on and conn is a RakNet connection.
func traceRakNet(ctx context.Context, conn net.Conn) net.Conn {
	trace := latencytrace.Active()
	rakConn, ok := conn.(*raknet.Conn)
	if trace == nil || !ok {
		return conn
	}
	upstream, _ := ctx.Value(upstreamTraceKey{}).(*upstreamTrace)
	if upstream == nil {
		upstream = new(upstreamTrace) // dials without a dialer hook still record their messages
	}
	return &tracedRakNet{Conn: rakConn, trace: trace, upstream: upstream}
}

func (conn *tracedRakNet) ReadPacket() ([]byte, error) {
	if conn.arrived != 0 {
		packets := conn.upstream.finish()
		conn.trace.Record(latencytrace.Message, conn.arrived, conn.trace.Now(), conn.size, int64(conn.key), packets)
		conn.arrived = 0
	}
	b, err := conn.Conn.ReadPacket()
	if err == nil {
		conn.arrived, conn.size, conn.key = conn.trace.Now(), int64(len(b)), messageKey(b)
		conn.upstream.begin(conn.key)
	}
	return b, err
}

// Write records each message gophertunnel hands to RakNet.
func (conn *tracedRakNet) Write(b []byte) (int, error) {
	key := messageKey(b)
	n, err := conn.Conn.Write(b)
	conn.trace.Record(latencytrace.Sent, conn.trace.Now(), int64(key), int64(len(b)))
	conn.upstream.mu.Lock()
	conn.upstream.lastOut = key
	conn.upstream.mu.Unlock()
	return n, err
}

// traceStartupBatch records a batch the session startup read, with an unknown origin, and once
// the batch holding StartGame is read, drops the messages it consumed from the play-phase queue.
func traceStartupBatch(upstream packetSession, batch []minecraft.RawPacket, holdsStartGame bool) {
	trace := latencytrace.Active()
	if trace == nil {
		return
	}
	trace.Record(latencytrace.StartupBatch, trace.Now(), unknownOrigin, int64(len(batch)))
	if holdsStartGame {
		lookupUpstreamTrace(upstream).startupRead()
	}
}

// relayTrace times one relay direction's batches; nil when tracing is off.
type relayTrace struct {
	trace     *latencytrace.Trace
	inbound   bool // upstream to the client
	conn      int64
	seq       int64
	readAt    int64
	key       uint64 // the inbound batch's message, or the outbound batch's once written
	upstream  *upstreamTrace
	ids       []uint32
	latencies []int64 // NetworkStackLatency timestamps in the current batch
	rtt       func() time.Duration
	rttAt     int64
}

func newRelayTrace(source, destination packetSession, fromDownstream bool) *relayTrace {
	trace := latencytrace.Active()
	if trace == nil {
		return nil
	}
	relay := &relayTrace{trace: trace, inbound: !fromDownstream}
	if session, ok := source.(*sessionConn); ok && fromDownstream {
		relay.conn = session.traceConn
	}
	if fromDownstream {
		relay.upstream = lookupUpstreamTrace(destination)
	} else {
		relay.rtt = upstreamLatency(source)
		relay.upstream = lookupUpstreamTrace(source)
	}
	return relay
}

// read notes a batch taken from the source at readAt, before it is forwarded to destination.
func (relay *relayTrace) read(batch []minecraft.RawPacket, readAt int64, destination packetSession) {
	if relay == nil {
		return
	}
	relay.readAt, relay.ids, relay.latencies = readAt, relay.ids[:0], relay.latencies[:0]
	for _, raw := range batch {
		relay.ids = append(relay.ids, raw.ID)
		if timestamp, ok := stackLatencyTimestamp(raw); ok {
			relay.latencies = append(relay.latencies, timestamp)
		}
	}
	if !relay.inbound {
		return
	}
	for _, timestamp := range relay.latencies {
		relay.trace.Record(latencytrace.StackLatency, 0, readAt, timestamp)
	}
	key := relay.upstream.batchMessage(len(batch))
	if session, ok := destination.(*sessionConn); ok {
		session.traceBatchRead(readAt, key)
	}
	if relay.rtt != nil && readAt-relay.rttAt >= int64(traceRoundTripInterval) {
		relay.rttAt = readAt
		if rtt := sampleRoundTrip(relay.rtt); rtt > 0 {
			relay.trace.Record(latencytrace.RoundTrip, readAt, int64(rtt))
		}
	}
}

// forwarded records an upstream-bound batch once the destination flush returned.
func (relay *relayTrace) forwarded() {
	if relay == nil || relay.inbound || len(relay.ids) == 0 {
		return
	}
	now := relay.trace.Now()
	relay.seq++
	var key uint64
	if relay.upstream != nil {
		relay.upstream.mu.Lock()
		key, relay.upstream.lastOut = relay.upstream.lastOut, 0
		relay.upstream.mu.Unlock()
	}
	relay.trace.RecordIDs(latencytrace.Outbound, relay.ids, relay.conn, relay.seq, relay.readAt, now, int64(key))
	for _, timestamp := range relay.latencies {
		relay.trace.Record(latencytrace.StackLatency, 1, now, timestamp)
	}
}

// upstreamLatency returns the half-RTT estimator beneath the relay's observing wrappers, which
// hide it; nil when the transport has none.
func upstreamLatency(session packetSession) func() time.Duration {
	if estimator, ok := unwrapUpstream(session).(interface{ Latency() time.Duration }); ok {
		return estimator.Latency
	}
	return nil
}

// unwrapUpstream returns the dialled session beneath the relay's observing wrappers.
func unwrapUpstream(session packetSession) packetSession {
	for {
		switch wrapper := session.(type) {
		case *transferObservingSession:
			session = wrapper.upstreamSession
		case *disconnectObservingSession:
			session = wrapper.upstreamSession
		default:
			return session
		}
	}
}

// sampleRoundTrip doubles a transport's half-RTT estimate; adapters without one report zero.
func sampleRoundTrip(latency func() time.Duration) (rtt time.Duration) {
	defer func() {
		if recover() != nil {
			rtt = 0
		}
	}()
	return 2 * latency()
}

// stackLatencyTimestamp reads a NetworkStackLatency packet's timestamp field.
func stackLatencyTimestamp(raw minecraft.RawPacket) (int64, bool) {
	if raw.ID != packet.IDNetworkStackLatency {
		return 0, false
	}
	_, n := readSessionVaruint32(raw.Data)
	if n <= 0 || len(raw.Data) < n+8 {
		return 0, false
	}
	return int64(binary.LittleEndian.Uint64(raw.Data[n:])), true
}
