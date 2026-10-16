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

// A datagram's frames carry the key of the message they start and their split identity.
func TestRakNetFramesKeyTheMessagesTheyStart(t *testing.T) {
	message := []byte{0xfe, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17}
	frame := func(flags byte, extra []byte, content []byte) []byte {
		out := []byte{flags, byte(len(content) * 8 >> 8), byte(len(content) * 8)}
		out = append(out, extra...)
		return append(out, content...)
	}
	ordered := []byte{1, 0, 0, 2, 0, 0, 0} // message index, order index, channel
	split := []byte{0, 0, 0, 2, 0, 7, 0, 0, 0, 0}
	splitLater := []byte{0, 0, 0, 2, 0, 7, 0, 0, 0, 1}
	body := frame(3<<5, ordered, message)
	body = append(body, frame(3<<5|0x10, append(ordered, split...), message[:10])...)
	body = append(body, frame(3<<5|0x10, append(ordered, splitLater...), message[10:])...)
	frames := rakNetFrames(body)
	want := []rakNetFrame{
		{key: messageKey(message), splitID: -1},
		{key: messageKey(message[:10]), splitID: 7, splitIndex: 0, splitCount: 2},
		{key: 0, splitID: 7, splitIndex: 1, splitCount: 2},
	}
	if len(frames) != len(want) {
		t.Fatalf("frames = %+v, want %+v", frames, want)
	}
	for i := range want {
		if frames[i] != want[i] {
			t.Fatalf("frame %d = %+v, want %+v", i, frames[i], want[i])
		}
	}
	if messageKey(message) != messageKey(append(message[:16:16], 99)) {
		t.Fatal("a split message's first fragment must key like the reassembled message")
	}
}

// Play-phase batches map to the messages that produced them: messages through StartGame leave
// the queue once startup is read, messages without accepted packets queue nothing, a batch read
// before its message closed takes that message, and one message can be read as two batches.
func TestUpstreamTraceAttributesBatchesToTheirMessages(t *testing.T) {
	upstream := new(upstreamTrace)
	message := func(key uint64, packets int64, startGame bool) {
		upstream.begin(key)
		upstream.accepted.Add(packets)
		upstream.sawStartGame.Store(startGame)
	}
	message(1, 2, false) // login, handled internally
	upstream.finish()
	message(2, 1, false) // DimensionData, deferred to the startup batch
	upstream.finish()
	message(3, 3, true)
	upstream.finish()
	upstream.startupRead()
	message(4, 0, false) // only subclient packets
	upstream.finish()
	message(5, 2, false)
	if key := upstream.batchMessage(2); key != 5 {
		t.Fatalf("early batch key = %d, want the message still being handled", key)
	}
	upstream.finish()
	message(6, 4, false)
	upstream.finish()
	for _, n := range []int{1, 3} {
		if key := upstream.batchMessage(n); key != 6 {
			t.Fatalf("split batch key = %d, want 6", key)
		}
	}
	if len(upstream.queued) != 0 {
		t.Fatalf("queued = %+v, want every batch read", upstream.queued)
	}
}

// Startup read before the StartGame message finished still drops that message and its elders.
func TestUpstreamTraceDropsStartGameFinishedAfterStartup(t *testing.T) {
	upstream := new(upstreamTrace)
	upstream.begin(1)
	upstream.accepted.Add(2)
	upstream.finish()
	upstream.begin(2)
	upstream.accepted.Add(3)
	upstream.sawStartGame.Store(true)
	upstream.startupRead()
	upstream.finish()
	upstream.begin(3)
	upstream.accepted.Add(1)
	upstream.finish()
	if key := upstream.batchMessage(1); key != 3 || len(upstream.queued) != 0 {
		t.Fatalf("first relay batch key = %d, queued %+v; want message 3 alone", key, upstream.queued)
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
