package proxy

import (
	"context"
	"encoding/binary"
	"net"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/latencytrace"
	"github.com/sandertv/go-raknet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// traceRoundTripInterval spaces RakNet RTT samples in the trace.
const traceRoundTripInterval = time.Second

// tracingDialer opens upstream UDP sockets that record each datagram's send and receipt.
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
		conn.trace.Record(latencytrace.UDPRead, conn.trace.Now(), int64(n), int64(b[0]), 0, 0)
	}
	return n, err
}

func (conn *tracedDatagrams) Write(b []byte) (int, error) {
	n, err := conn.Conn.Write(b)
	if n > 0 {
		conn.trace.Record(latencytrace.UDPWrite, conn.trace.Now(), int64(n), int64(b[0]), 0, 0)
	}
	return n, err
}

// tracedRakNet records when each RakNet message reaches gophertunnel and when its batch was
// handled, which is when gophertunnel's single reader asks for the next message.
type tracedRakNet struct {
	*raknet.Conn
	trace   *latencytrace.Trace
	arrived int64
	size    int64
}

// traceRakNet wraps conn when tracing is on and conn is a RakNet connection.
func traceRakNet(conn net.Conn) net.Conn {
	trace := latencytrace.Active()
	rakConn, ok := conn.(*raknet.Conn)
	if trace == nil || !ok {
		return conn
	}
	return &tracedRakNet{Conn: rakConn, trace: trace}
}

func (conn *tracedRakNet) ReadPacket() ([]byte, error) {
	if conn.arrived != 0 {
		conn.trace.Record(latencytrace.Message, conn.arrived, conn.trace.Now(), conn.size, 0, 0)
		conn.arrived = 0
	}
	b, err := conn.Conn.ReadPacket()
	if err == nil {
		conn.arrived, conn.size = conn.trace.Now(), int64(len(b))
	}
	return b, err
}

// relayTrace times one relay direction's batches; nil when tracing is off.
type relayTrace struct {
	trace     *latencytrace.Trace
	inbound   bool // upstream to the client
	conn      int64
	seq       int64
	readAt    int64
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
	if !fromDownstream {
		relay.rtt = upstreamLatency(source)
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
		relay.trace.Record(latencytrace.StackLatency, 0, readAt, timestamp, 0, 0)
	}
	if session, ok := destination.(*sessionConn); ok {
		session.traceBatchRead(readAt)
	}
	if relay.rtt != nil && readAt-relay.rttAt >= int64(traceRoundTripInterval) {
		relay.rttAt = readAt
		if rtt := sampleRoundTrip(relay.rtt); rtt > 0 {
			relay.trace.Record(latencytrace.RoundTrip, readAt, int64(rtt), 0, 0, 0)
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
	relay.trace.RecordIDs(latencytrace.Outbound, relay.conn, relay.seq, relay.readAt, now, 0, relay.ids)
	for _, timestamp := range relay.latencies {
		relay.trace.Record(latencytrace.StackLatency, 1, now, timestamp, 0, 0)
	}
}

// upstreamLatency returns the half-RTT estimator beneath the relay's observing wrappers, which
// hide it; nil when the transport has none.
func upstreamLatency(session packetSession) func() time.Duration {
	for {
		switch wrapper := session.(type) {
		case interface{ Latency() time.Duration }:
			return wrapper.Latency
		case *transferObservingSession:
			session = wrapper.upstreamSession
		case *disconnectObservingSession:
			session = wrapper.upstreamSession
		default:
			return nil
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
