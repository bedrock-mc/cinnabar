package proxy

import (
	"bytes"
	"errors"
	"io"
	"net"
	"sync/atomic"
	"testing"
	"testing/synctest"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestSessionReaderBuffersBurstAndPreservesEOFAfterBatches(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		session, peer := newTestSessionConn(t)
		const burst = 8
		written := make(chan error, 1)
		go func() {
			for i := range burst {
				if _, err := peer.Write(sessionTestBatch(&packet.NetworkStackLatency{Timestamp: int64(i)})); err != nil {
					written <- err
					return
				}
			}
			written <- peer.Close()
		}()
		synctest.Wait()
		select {
		case err := <-written:
			if err != nil {
				t.Fatal(err)
			}
		default:
			t.Fatal("client burst stalled while batch consumer was paused")
		}
		for i := range burst {
			assertSessionBatch(t, session, encodeTestPacket(&packet.NetworkStackLatency{Timestamp: int64(i)}))
		}
		if _, err := session.ReadBatchRaw(nil); !errors.Is(err, io.EOF) {
			t.Fatalf("after buffered batches: %v, want EOF", err)
		}
	})
}

func TestSessionReaderBackpressureResumesWithoutPacketLoss(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		session, peer := newTestSessionConn(t)
		burst := sessionReadAheadFrames + 3
		var completed atomic.Int32
		written := make(chan error, 1)
		go func() {
			for i := range burst {
				if _, err := peer.Write(sessionTestBatch(&packet.NetworkStackLatency{Timestamp: int64(i)})); err != nil {
					written <- err
					return
				}
				completed.Add(1)
			}
			written <- nil
		}()
		synctest.Wait()
		if got := int(completed.Load()); got == 0 || got >= burst {
			t.Fatalf("completed %d of %d writes before consumer resumed; want bounded read-ahead", got, burst)
		}
		for i := range burst {
			assertSessionBatch(t, session, encodeTestPacket(&packet.NetworkStackLatency{Timestamp: int64(i)}))
		}
		if err := <-written; err != nil {
			t.Fatal(err)
		}
	})
}

func TestSessionReaderBoundsQueuedBytesAndResumes(t *testing.T) {
	// Two individually legal frames exceed the queued byte budget together.
	large := sessionTestBatch(&packet.ScriptMessage{Data: make([]byte, streamnet.MaxFrameLen/2)})
	small := sessionTestBatch(&packet.NetworkStackLatency{Timestamp: 9})
	synctest.Test(t, func(t *testing.T) {
		session, peer := newTestSessionConn(t)
		var completed atomic.Int32
		written := make(chan error, 1)
		go func() {
			for _, frame := range [][]byte{large, large, small} {
				if _, err := peer.Write(frame); err != nil {
					written <- err
					return
				}
				completed.Add(1)
			}
			written <- nil
		}()
		synctest.Wait()
		if got := completed.Load(); got != 2 {
			t.Fatalf("completed %d writes, want one queued large frame and one held by the reader", got)
		}
		largePackets, err := splitSessionBatch(large[1:])
		if err != nil {
			t.Fatal(err)
		}
		assertSessionBatch(t, session, largePackets[0])
		synctest.Wait()
		select {
		case err := <-written:
			if err != nil {
				t.Fatal(err)
			}
		default:
			t.Fatal("dequeue did not release the byte budget")
		}
		assertSessionBatch(t, session, largePackets[0])
		assertSessionBatch(t, session, encodeTestPacket(&packet.NetworkStackLatency{Timestamp: 9}))
	})
}

func TestSessionReaderCloseReleasesBlockedWriter(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		session, peer := newTestSessionConn(t)
		written := make(chan error, 1)
		go func() {
			for range sessionReadAheadFrames + 3 {
				if _, err := peer.Write(sessionTestBatch(&packet.NetworkStackLatency{})); err != nil {
					written <- err
					return
				}
			}
			written <- nil
		}()
		synctest.Wait()
		if err := session.Close(); err != nil {
			t.Fatal(err)
		}
		if err := <-written; err == nil {
			t.Fatal("all writes completed despite a full queue and closed session")
		}
		<-session.readDone
		if _, err := session.ReadBatchRaw(nil); !errors.Is(err, net.ErrClosed) {
			t.Fatalf("read after Close = %v, want closed", err)
		}
	})
}

// assertSessionBatch checks exact packet bytes and cardinality at the consumer boundary.
func assertSessionBatch(t *testing.T, session *sessionConn, want []byte) {
	t.Helper()
	got, err := session.ReadBatchRaw(nil)
	if err != nil {
		t.Fatal(err)
	}
	if len(got) != 1 {
		t.Fatalf("forwarded %d packets, want one", len(got))
	}
	if !bytes.Equal(got[0].Data, want) {
		t.Fatal("packet bytes changed or batches reordered")
	}
}
