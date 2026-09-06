package proxy

import (
	"context"
	"errors"
	"fmt"
	"reflect"
	"testing"

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
	if err := pumpPackets(down, up, true); !errors.Is(err, reason) {
		t.Fatalf("pump error = %v, want original disconnect", err)
	}
	if len(up.written()) != 0 {
		t.Fatal("forwarded a downstream-only disconnect upstream")
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
