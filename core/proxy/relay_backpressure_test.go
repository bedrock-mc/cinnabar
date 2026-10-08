package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"reflect"
	"slices"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// countingSource counts batch reads on the wrapped session.
type countingSource struct {
	*fakeDownstream
	reads atomic.Int32
}

func (s *countingSource) ReadBatchRaw(decode func(uint32) bool) ([]minecraft.RawPacket, error) {
	s.reads.Add(1)
	return s.fakeDownstream.ReadBatchRaw(decode)
}

// gatedSink blocks writes until gate closes or the session is torn down.
type gatedSink struct {
	*fakeUpstream
	gate    chan struct{}
	entered chan struct{}
}

func newGatedSink() *gatedSink {
	return &gatedSink{fakeUpstream: newFakeUpstream(nil), gate: make(chan struct{}), entered: make(chan struct{}, 64)}
}

func (s *gatedSink) WritePacketRaw(data []byte) error {
	s.entered <- struct{}{}
	select {
	case <-s.gate:
	case <-s.closed:
		return net.ErrClosed
	}
	return s.fakeUpstream.WritePacketRaw(data)
}

func stamps(n int) [][]packet.Packet {
	batches := make([][]packet.Packet, n)
	for i := range batches {
		batches[i] = []packet.Packet{&packet.NetworkStackLatency{Timestamp: int64(i)}, &packet.NetworkStackLatency{Timestamp: int64(i) + 100}}
	}
	return batches
}

func TestRelaySlowReaderBoundsReadAheadAndStaysLossless(t *testing.T) {
	src := &countingSource{fakeDownstream: newFakeDownstream(nil)}
	src.useBatchReads = true
	sink := newGatedSink()
	want := stamps(5)
	for _, b := range want {
		src.batchReads <- batchResult{packets: b}
	}
	src.batchReads <- batchResult{err: io.EOF}

	done := make(chan error, 1)
	go func() { done <- pumpPackets(src, sink, true) }()
	<-sink.entered
	time.Sleep(50 * time.Millisecond)
	// The reader keeps reading while it forwards so it can serve flushes, holding at most one batch ahead.
	if got := src.reads.Load(); got != 2 {
		t.Fatalf("source batches read while sink stalled = %d, want 2", got)
	}
	close(sink.gate)
	if err := <-done; !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	got := sink.flushedBatches()
	if len(got) != len(want) {
		t.Fatalf("delivered %d batches, want %d", len(got), len(want))
	}
	for i := range want {
		if !slices.Equal(got[i], want[i]) {
			t.Fatalf("batch %d reordered or altered: %#v", i, got[i])
		}
	}
}

func TestRelayCancellationReleasesStalledWriter(t *testing.T) {
	down := newFakeDownstream(nil)
	down.useBatchReads = true
	sink := newGatedSink()
	down.batchReads <- batchResult{packets: stamps(1)[0]}
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan error, 1)
	go func() { done <- relayWithSessions(ctx, down, sink) }()
	<-sink.entered
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("relayWithSessions() error = %v, want cancellation", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("relay did not shut down with a stalled writer")
	}
}

func TestRelayForwardsPartialBatchBeforeMidBatchDecodeClose(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	up.useBatchReads = true
	delivered := stamps(1)[0]
	closeErr := errors.New("decode closed connection")
	up.batchReads <- batchResult{packets: delivered}
	up.batchReads <- batchResult{err: closeErr}

	err := relayWithSessions(context.Background(), down, up)
	if !errors.Is(err, closeErr) {
		t.Fatalf("relay error = %v, want decode close error", err)
	}
	batches := down.flushedBatches()
	if len(batches) != 1 || !slices.Equal(batches[0], delivered) {
		t.Fatalf("packets before the offending one were not delivered as one batch: %#v", batches)
	}
	if !down.isClosed() || !up.isClosed() {
		t.Fatal("relay left a session open after decode close")
	}
}

func TestRelaySkipsEmptyBatchesWithoutFlushing(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	down.useBatchReads = true
	want := stamps(1)[0]
	down.batchReads <- batchResult{packets: nil}
	down.batchReads <- batchResult{packets: want}
	down.batchReads <- batchResult{err: io.EOF}
	if err := pumpPackets(down, up, true); !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	if got := up.flushedBatches(); len(got) != 1 || !slices.Equal(got[0], want) {
		t.Fatalf("batches = %#v, want one batch %#v", got, want)
	}
}

func TestRelayKeepsBatchBoundaryBeforeUpstreamDisconnect(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	up.useBatchReads = true
	before := stamps(1)[0]
	reason := &minecraft.DisconnectPacketError{Message: "server message", FilteredMessage: "filtered"}
	up.batchReads <- batchResult{packets: before}
	up.batchReads <- batchResult{err: reason}
	if err := relayWithSessions(context.Background(), down, up); !errors.Is(err, reason) {
		t.Fatalf("relay error = %v, want disconnect", err)
	}
	batches := down.flushedBatches()
	if len(batches) != 2 || !slices.Equal(batches[0], before) || len(batches[1]) != 1 || !reflect.DeepEqual(batches[1][0], reason.Packet()) {
		t.Fatalf("batches = %#v, want [pre-disconnect batch][disconnect]", batches)
	}
}

// eventSink records writes and flushes in order; a write of slow stalls first.
type eventSink struct {
	*fakeUpstream
	mu     sync.Mutex
	events []string
	slow   packet.Packet
	stall  time.Duration
}

func (s *eventSink) WritePacketRaw(data []byte) error {
	value := packetFromRaw(data)
	if value == s.slow {
		time.Sleep(s.stall)
	}
	s.record(fmt.Sprintf("write %d", value.(*packet.NetworkStackLatency).Timestamp))
	return s.fakeUpstream.WritePacket(value)
}

func (s *eventSink) Flush() error {
	s.record("flush")
	return s.fakeUpstream.Flush()
}

func (s *eventSink) record(event string) {
	s.mu.Lock()
	s.events = append(s.events, event)
	s.mu.Unlock()
}

func (s *eventSink) recorded() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return slices.Clone(s.events)
}

// A batch whose forwarding stalls past the idle flush still leaves as one batch, flushed once after its last packet.
func TestRelayKeepsAStalledBatchWhole(t *testing.T) {
	src := newFakeDownstream(nil)
	src.useBatchReads = true
	batch := stamps(1)[0]
	batch = append(batch, &packet.NetworkStackLatency{Timestamp: 7})
	sink := &eventSink{fakeUpstream: newFakeUpstream(nil), slow: batch[1], stall: 3 * relayIdleFlush}
	src.batchReads <- batchResult{packets: batch}
	src.batchReads <- batchResult{err: io.EOF}
	if err := pumpPackets(src, sink, true); !errors.Is(err, io.EOF) {
		t.Fatalf("pumpPackets() error = %v, want EOF", err)
	}
	if got := sink.flushedBatches(); len(got) != 1 || !slices.Equal(got[0], batch) {
		t.Fatalf("batches = %v, want the source batch whole", batchSizes(got))
	}
	events := sink.recorded()
	first := slices.Index(events, "write 0")
	if want := []string{"write 0", "write 100", "write 7", "flush"}; first < 0 || !slices.Equal(events[first:first+4], want) {
		t.Fatalf("events = %v, want the batch's writes then one flush", events)
	}
}
