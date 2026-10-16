package proxy

import (
	"bytes"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// NetworkStackLatency probes and replies are matched by their timestamp field.
func TestStackLatencyTimestampReadsTheProbe(t *testing.T) {
	buf := new(bytes.Buffer)
	(&packet.Header{PacketID: packet.IDNetworkStackLatency}).Write(buf)
	(&packet.NetworkStackLatency{Timestamp: 1234567, NeedsResponse: true}).Marshal(minecraft.DefaultProtocol.NewWriter(buf, 0))
	raw := minecraft.RawPacket{ID: packet.IDNetworkStackLatency, Data: buf.Bytes()}
	if timestamp, ok := stackLatencyTimestamp(raw); !ok || timestamp != 1234567 {
		t.Fatalf("timestamp = %d, %v; want 1234567, true", timestamp, ok)
	}
	if _, ok := stackLatencyTimestamp(minecraft.RawPacket{ID: packet.IDText, Data: buf.Bytes()}); ok {
		t.Fatal("another packet must not read as a probe")
	}
}

type latencyUpstream struct{ upstreamSession }

func (latencyUpstream) Latency() time.Duration { return 7 * time.Millisecond }

// The relay samples RTT through the transfer and disconnect observers that wrap a session upstream.
func TestUpstreamLatencySeesThroughObservers(t *testing.T) {
	wrapped := observeDisconnects(observeTransfers(latencyUpstream{}, new(TransferState), nil), func(DisconnectInfo) {})
	latency := upstreamLatency(wrapped)
	if latency == nil || latency() != 7*time.Millisecond {
		t.Fatal("the wrapped upstream's estimator must be found")
	}
	if upstreamLatency(observeTransfers(struct{ upstreamSession }{}, new(TransferState), nil)) != nil {
		t.Fatal("an upstream without an estimator has none")
	}
}
