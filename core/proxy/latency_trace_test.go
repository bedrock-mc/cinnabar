package proxy

import (
	"bytes"
	"fmt"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/latencytrace"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"

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

// The handoff carries the core's trace ID only while tracing, keeping untraced handoffs unchanged.
func TestHandoffCarriesTheTraceConnection(t *testing.T) {
	traced, err := encodeSessionHandoff(sessionHandoff{TraceConn: 81604378625}, nil)
	if err != nil {
		t.Fatal(err)
	}
	untraced, err := encodeSessionHandoff(sessionHandoff{}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(traced, []byte(`"trace_conn":81604378625`)) || bytes.Contains(untraced, []byte("trace_conn")) {
		t.Fatalf("traced %q, untraced %q", traced, untraced)
	}
}

// Only a bridge frame the client socket accepted is traced as written to it.
func TestFailedBridgeWritesAreNotTraced(t *testing.T) {
	dir := t.TempDir()
	trace, err := latencytrace.Open(dir, "bridge")
	if err != nil {
		t.Fatal(err)
	}
	local, peer := net.Pipe()
	go func() { _, _ = io.Copy(io.Discard, peer) }()
	session := newSessionConn(streamnet.NewFramedConn(local))
	defer session.Close()
	session.trace, session.traceConn = trace, 7
	write := func() error {
		if err := session.WritePacketRaw([]byte{byte(packet.IDText), 0}); err != nil {
			return err
		}
		return session.Flush()
	}
	if err := write(); err != nil {
		t.Fatal(err)
	}
	_ = peer.Close()
	if write() == nil {
		t.Fatal("the closed peer must fail the write")
	}
	trace.Close()
	data, err := os.ReadFile(filepath.Join(dir, fmt.Sprintf("bridge-%d.trace", os.Getpid())))
	if err != nil {
		t.Fatal(err)
	}
	if frames := strings.Count(string(data), "\nI 7 "); frames != 1 {
		t.Fatalf("trace = %q, want only the written frame", data)
	}
}
