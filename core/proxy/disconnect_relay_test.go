package proxy

import (
	"context"
	"errors"
	"fmt"
	"net"
	"reflect"
	"slices"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// Successful flushes and downstream failures must not allocate an upstream error inspection target.
func TestRelayErrorPassthroughDoesNotAllocate(t *testing.T) {
	downstream := errors.New("downstream write failed")
	for _, test := range []struct {
		name     string
		err      error
		upstream bool
	}{
		{"upstream success", nil, true},
		{"downstream success", nil, false},
		{"downstream failure", downstream, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			if got := attributeRelayError(test.err, test.upstream); got != test.err {
				t.Fatalf("error = %v, want original %v", got, test.err)
			}
			allocations := testing.AllocsPerRun(100, func() {
				if attributeRelayError(test.err, test.upstream) != test.err {
					panic("relay error changed")
				}
			})
			if allocations != 0 {
				t.Fatalf("passthrough allocations = %v, want zero", allocations)
			}
		})
	}
}

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
			err := relayWithSessions(context.Background(), down, up)
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
	if err := relayWithSessions(context.Background(), down, up); !errors.Is(err, reason) {
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
	err := relayWithSessions(context.Background(), down, up)
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

func (s *reverseFirstDisconnectSession) ReadBatchRaw(func(uint32) bool) ([]minecraft.RawPacket, error) {
	// Force the reverse write result to win; the reader becomes runnable only
	// once the coordinator tears down the upstream session.
	<-s.closed
	return nil, s.reason
}

func (s *reverseFirstDisconnectSession) WritePacketRaw([]byte) error {
	return s.reason
}

func TestRelayPreservesDisconnectWhenReverseWriterFinishesFirst(t *testing.T) {
	down := newFakeDownstream(nil)
	reason := &minecraft.DisconnectPacketError{Reason: 7, Message: "server stopped"}
	up := &reverseFirstDisconnectSession{fakeUpstream: newFakeUpstream(nil), reason: reason}
	down.reads <- packetResult{packet: &packet.NetworkStackLatency{Timestamp: 1}}
	err := relayWithSessions(context.Background(), down, up)
	if !errors.Is(err, reason) {
		t.Fatalf("relay error = %v, want original server disconnect", err)
	}
	if got := down.written(); !reflect.DeepEqual(got, []packet.Packet{reason.Packet()}) {
		t.Fatalf("disconnect must be delivered exactly once before teardown: %#v", got)
	}
}

func TestRelayRetainsDistinctErrorsFromBothPumps(t *testing.T) {
	down := newFakeDownstream(nil)
	downErr := errors.New("downstream read failed")
	upErr := errors.New("upstream read failed after teardown")
	up := &reverseFirstDisconnectSession{fakeUpstream: newFakeUpstream(nil), reason: upErr}
	down.reads <- packetResult{err: downErr}
	err := relayWithSessions(context.Background(), down, up)
	if !errors.Is(err, downErr) || !errors.Is(err, upErr) {
		t.Fatalf("relay error = %v, want both independent pump failures", err)
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
	go func() { done <- relayWithSessions(ctx, down, up) }()
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

type recordingDisconnecter struct{ got []packet.Disconnect }

func (r *recordingDisconnecter) DisconnectPacket(pk packet.Disconnect) error {
	r.got = append(r.got, pk)
	return nil
}

func TestRelayPreLoginDisconnectForwardsServerReason(t *testing.T) {
	reason := &minecraft.DisconnectPacketError{Reason: 3, Message: "you are banned", FilteredMessage: "filtered"}
	var rec recordingDisconnecter
	relayPreLoginDisconnect(&rec, fmt.Errorf("dial upstream: %w", errors.Join(errors.New("cleanup"), reason)))
	if len(rec.got) != 1 || !reflect.DeepEqual(rec.got[0], *reason.Packet()) {
		t.Fatalf("forwarded = %#v, want %#v", rec.got, *reason.Packet())
	}
}

// A failed join reads as vanilla's lang key; a cancelled one gets no packet.
func TestRelayPreLoginDisconnectWordsJoinFailuresAsVanilla(t *testing.T) {
	var rec recordingDisconnecter
	relayPreLoginDisconnect(&rec, errors.New("dial raknet: i/o timeout"))
	relayPreLoginDisconnect(&rec, &realmJoinError{err: errors.New("remote peer notified connection failure (code: 37)")})
	relayPreLoginDisconnect(&rec, fmt.Errorf("dial: %w", errResourcePackTransferTooLarge))
	relayPreLoginDisconnect(&rec, &preparationCancellationError{cause: context.Canceled})
	relayPreLoginDisconnect(&rec, nil)
	var got []string
	for _, pk := range rec.got {
		got = append(got, pk.Message)
	}
	want := []string{"disconnectionScreen.cantConnect", "disconnectionScreen.cantConnectToRealm", "disconnectionScreen.resourcePack"}
	if !slices.Equal(got, want) {
		t.Fatalf("messages = %q, want %q", got, want)
	}
}

func TestNetworkForAddressUsesRakNetForTransferTargets(t *testing.T) {
	target := &resolvedUpstreamTarget{address: "Host:1", network: scopedNetherNetNetwork{}}
	if _, ok := networkForAddress(target, "host:1").(scopedNetherNetNetwork); !ok {
		t.Fatal("resolved address lost its transport")
	}
	if _, ok := networkForAddress(target, "other.example:19132").(minecraft.RakNet); !ok {
		t.Fatal("transfer target must dial over RakNet")
	}
}
