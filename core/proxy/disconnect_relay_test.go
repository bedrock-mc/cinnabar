package proxy

import (
	"context"
	"errors"
	"fmt"
	"net"
	"reflect"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestRelayPreservesUpstreamDisconnectBeforeClosing(t *testing.T) {
	for _, hidden := range []bool{false, true} {
		t.Run(fmt.Sprintf("hidden=%t", hidden), func(t *testing.T) {
			down := newFakeDownstream(nil)
			up := newFakeUpstream(nil)
			reason := &minecraft.DisconnectPacketError{
				Reason: 7, HideDisconnectionScreen: hidden,
				Message: "original server message", FilteredMessage: "filtered server message",
			}
			before := &packet.NetworkStackLatency{Timestamp: 42}
			up.reads <- packetResult{packet: before}
			up.reads <- packetResult{err: fmt.Errorf("receive: %w", reason)}
			err := relayPackets(context.Background(), down, up)
			if !errors.Is(err, reason) {
				t.Fatalf("relay error = %v, want original disconnect", err)
			}
			want := []packet.Packet{before, reason.Packet()}
			if got := down.written(); !reflect.DeepEqual(got, want) {
				t.Fatalf("forwarded packets = %#v, want %#v", got, want)
			}
			batches := down.flushedBatches()
			if len(batches) != 2 || !reflect.DeepEqual(batches[1], want[1:]) {
				t.Fatalf("disconnect was not flushed before shutdown: %#v", batches)
			}
			if !down.isClosed() || !up.isClosed() {
				t.Fatal("relay did not close both sessions")
			}
		})
	}
}

func TestRelayDoesNotReflectDownstreamDisconnectUpstream(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	reason := &minecraft.DisconnectPacketError{Message: "local disconnect"}
	down.reads <- packetResult{err: reason}
	if err := relayPackets(context.Background(), down, up); !errors.Is(err, reason) {
		t.Fatalf("relay error = %v, want original disconnect", err)
	}
	if len(up.written()) != 0 || len(down.written()) != 0 {
		t.Fatal("reflected a downstream-only disconnect")
	}
}

func TestRelayDisconnectFlushFailurePreservesBothErrors(t *testing.T) {
	down := newFakeDownstream(nil)
	up := newFakeUpstream(nil)
	reason := &minecraft.DisconnectPacketError{Message: "server closing"}
	flushErr := errors.New("local transport flush failed")
	down.flushErr = flushErr
	up.reads <- packetResult{err: reason}
	err := relayPackets(context.Background(), down, up)
	if !errors.Is(err, reason) || !errors.Is(err, flushErr) {
		t.Fatalf("relay error = %v, want disconnect and flush failure", err)
	}
	if !down.isClosed() || !up.isClosed() {
		t.Fatal("failed disconnect delivery did not close both sessions")
	}
}

type reverseFirstDisconnectSession struct {
	*fakeUpstream
	reason error
}

func (s *reverseFirstDisconnectSession) ReadBatch() ([]packet.Packet, error) {
	// Force the reverse write result to win; the reader becomes runnable only
	// once the coordinator tears down the upstream session.
	<-s.closed
	return nil, s.reason
}

func (s *reverseFirstDisconnectSession) WritePacketImmediate(...packet.Packet) error {
	return s.reason
}

func TestRelayPreservesDisconnectWhenReverseWriterFinishesFirst(t *testing.T) {
	down := newFakeDownstream(nil)
	reason := &minecraft.DisconnectPacketError{Reason: 7, Message: "server stopped"}
	up := &reverseFirstDisconnectSession{fakeUpstream: newFakeUpstream(nil), reason: reason}
	down.reads <- packetResult{packet: &packet.NetworkStackLatency{Timestamp: 1}}
	err := relayPackets(context.Background(), down, up)
	if !errors.Is(err, reason) {
		t.Fatalf("relay error = %v, want original server disconnect", err)
	}
	if got := down.written(); !reflect.DeepEqual(got, []packet.Packet{reason.Packet()}) {
		t.Fatalf("disconnect must be delivered exactly once before teardown: %#v", got)
	}
}

type blockedDisconnectDestination struct {
	*fakeDownstream
	started chan struct{}
}

func (s *blockedDisconnectDestination) WritePacketImmediate(...packet.Packet) error {
	close(s.started)
	<-s.closed
	return net.ErrClosed
}

func TestRelayCancellationUnblocksDisconnectDelivery(t *testing.T) {
	down := &blockedDisconnectDestination{fakeDownstream: newFakeDownstream(nil), started: make(chan struct{})}
	up := newFakeUpstream(nil)
	up.reads <- packetResult{err: &minecraft.DisconnectPacketError{Message: "server stopped"}}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- relayPackets(ctx, down, up) }()
	select {
	case <-down.started:
	case <-time.After(time.Second):
		t.Fatal("disconnect delivery did not start")
	}
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("relay error = %v, want cancellation", err)
		}
	case <-time.After(time.Second):
		t.Fatal("cancellation left disconnect delivery blocked")
	}
	if !down.isClosed() || !up.isClosed() {
		t.Fatal("canceled relay did not close both sessions")
	}
}
