package proxy

import (
	"encoding/binary"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/latencytrace"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// traceRoundTripInterval spaces RakNet RTT samples in the trace.
const traceRoundTripInterval = time.Second

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
		relay.trace.Record(latencytrace.StackLatency, 0, readAt, timestamp)
	}
	if session, ok := destination.(*sessionConn); ok {
		session.traceBatchRead(readAt)
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
	relay.trace.RecordIDs(latencytrace.Outbound, relay.ids, relay.conn, relay.seq, relay.readAt, now)
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
		case *positionUpstream:
			session = wrapper.packetSession
		case *positionWriter:
			session = wrapper.packetSession
		case *transferObservingSession:
			session = wrapper.upstreamSession
		case *disconnectObservingSession:
			session = wrapper.upstreamSession
		default:
			return session
		}
	}
}

// sampleLatency preserves unknown latency and tolerates adapters without an estimator.
func sampleLatency(latency func() time.Duration) (half time.Duration) {
	if latency == nil {
		return 0
	}
	defer func() {
		if recover() != nil {
			half = 0
		}
	}()
	return latency()
}

// sampleRoundTrip doubles a transport's half-RTT estimate; adapters without one report zero.
func sampleRoundTrip(latency func() time.Duration) time.Duration {
	return 2 * sampleLatency(latency)
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
