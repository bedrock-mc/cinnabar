package proxy

import (
	"bytes"
	"testing"

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
