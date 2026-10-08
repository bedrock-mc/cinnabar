package proxy

import (
	"errors"
	"io"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

type countedFlushSink struct {
	*fakeUpstream
	flushes  atomic.Int32
	flushed  chan struct{}
	failure  error
	writeErr error
}

// WritePacketRaw can fail independently of the destination's flush.
func (s *countedFlushSink) WritePacketRaw(data []byte) error {
	if s.writeErr != nil {
		return s.writeErr
	}
	return s.fakeUpstream.WritePacketRaw(data)
}

// Flush counts even empty attempts and signals after transport submission.
func (s *countedFlushSink) Flush() error {
	s.flushes.Add(1)
	err := s.fakeUpstream.Flush()
	if s.flushed != nil {
		s.flushed <- struct{}{}
	}
	return errors.Join(err, s.failure)
}

// controlledReader uses manual ticks so assertions depend on events, not scheduler timing.
func controlledReader(t *testing.T, source *fakeDownstream, sink packetSession, upstream bool) (*packetReader, chan time.Time) {
	t.Helper()
	source.useBatchReads = true
	reader := newPacketReader(source, sink, upstream, time.Hour, nil)
	reader.idle.Stop()
	ticks := make(chan time.Time, 1)
	reader.idle.C = ticks
	t.Cleanup(func() { reader.Close(); _ = source.Close() })
	return reader, ticks
}

func TestRelayIdleWriteLeavesOnFirstTick(t *testing.T) {
	src := newFakeDownstream(nil)
	sink := &countedFlushSink{fakeUpstream: newFakeUpstream(nil), flushed: make(chan struct{}, 1)}
	reader, ticks := controlledReader(t, src, sink, false)
	out := &packet.NetworkStackLatency{Timestamp: 9}
	if err := sink.WritePacket(out); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { _, err := reader.Read(); done <- err }()
	ticks <- time.Now()
	select {
	case <-sink.flushed:
	case <-time.After(time.Second):
		t.Fatal("first idle tick did not flush")
	}
	if got := sink.flushedBatches(); len(got) != 1 || got[0][0] != out || sink.flushes.Load() != 1 {
		t.Fatal("first tick did not deliver the buffered write exactly once")
	}
	src.batchReads <- batchResult{err: io.EOF}
	if err := <-done; !errors.Is(err, io.EOF) {
		t.Fatal(err)
	}
}

func TestRelayFailureAttribution(t *testing.T) {
	for _, upstream := range []bool{false, true} {
		for _, operation := range []string{"read", "write", "idle", "boundary"} {
			t.Run(operation+map[bool]string{true: "-upstream", false: "-downstream"}[upstream], func(t *testing.T) {
				failure := &minecraft.DisconnectPacketError{Message: "injected " + operation}
				src := newFakeDownstream(nil)
				sink := &countedFlushSink{fakeUpstream: newFakeUpstream(nil), failure: failure}
				reader, ticks := controlledReader(t, src, sink, upstream)
				var err error
				switch operation {
				case "read":
					src.batchReads <- batchResult{err: failure}
					_, err = reader.Read()
				case "idle":
					ticks <- time.Now()
					_, err = reader.Read()
				case "boundary":
					err = reader.Flush()
				case "write":
					src = newFakeDownstream(nil)
					src.useBatchReads = true
					defer src.Close()
					sink.failure = nil
					sink.writeErr = failure
					src.batchReads <- batchResult{packets: stamps(1)[0]}
					err = pumpPackets(src, sink, !upstream)
				}
				var attributed *upstreamRelayDisconnect
				wantUpstream := !upstream
				if operation == "read" {
					wantUpstream = upstream
				}
				if !errors.Is(err, failure) || errors.As(err, &attributed) != wantUpstream {
					t.Fatalf("error = %v; upstream attribution wanted %t", err, wantUpstream)
				}
			})
		}
	}
}
