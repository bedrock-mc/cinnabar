package proxy

import (
	"context"
	"errors"
	"net"
	"reflect"
	"sync"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// closedWriterUpstream keeps received batches queued while upstream flushes report closure.
type closedWriterUpstream struct {
	*fakeUpstream
	failed chan struct{}
	once   sync.Once
}

// Flush exposes the upstream close before its queued input is released by the test.
func (s *closedWriterUpstream) Flush() error {
	s.once.Do(func() { close(s.failed) })
	return net.ErrClosed
}

// TestRelayDrainsClosedUpstream preserves both batch layouts despite the reverse flush failure.
func TestRelayDrainsClosedUpstream(t *testing.T) {
	for _, sameBatch := range []bool{false, true} {
		t.Run(map[bool]string{false: "separate", true: "same"}[sameBatch], func(t *testing.T) {
			down := newFakeDownstream(nil)
			up := &closedWriterUpstream{fakeUpstream: newFakeUpstream(nil), failed: make(chan struct{})}
			up.useBatchReads = true
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			done := make(chan error, 1)
			go func() { done <- relayWithSessions(ctx, down, up) }()
			<-up.failed
			want := []packet.Packet{relayFixtureStartup()[0], &packet.Transfer{Address: "next.example.test", Port: 19133}}
			if sameBatch {
				up.batchReads <- batchResult{packets: want}
			} else {
				for _, value := range want {
					up.batchReads <- batchResult{packets: []packet.Packet{value}}
				}
			}
			up.batchReads <- batchResult{err: net.ErrClosed}
			if err := <-done; err != nil {
				t.Fatal(err)
			}
			var delivered []packet.Packet
			for _, batch := range down.flushedBatches() {
				delivered = append(delivered, batch...)
			}
			if !reflect.DeepEqual(delivered, want) {
				t.Fatalf("flushed %#v, want %#v", delivered, want)
			}
		})
	}
}

// TestRelayCancellationUnblocksUpstreamDrain bounds a stalled read after the reverse writer closes.
func TestRelayCancellationUnblocksUpstreamDrain(t *testing.T) {
	down := newFakeDownstream(nil)
	up := &closedWriterUpstream{fakeUpstream: newFakeUpstream(nil), failed: make(chan struct{})}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- relayWithSessions(ctx, down, up) }()
	<-up.failed
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("relay error = %v, want cancellation", err)
		}
	case <-time.After(time.Second):
		t.Fatal("upstream drain ignored cancellation")
	}
	if !down.isClosed() || !up.isClosed() {
		t.Fatal("cancellation left a session open")
	}
}
