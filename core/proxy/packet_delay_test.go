package proxy

import (
	"context"
	"errors"
	"io"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestPacketDelayFIFOAndPipeline(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		for _, direction := range []bool{true, false} {
			delay := new(PacketDelay)
			if err := delay.Set(200); err != nil {
				t.Fatal(err)
			}
			src, dst := newFakeDownstream(nil), newFakeUpstream(nil)
			src.useBatchReads = true
			start := time.Now()
			done := make(chan error, 1)
			go func() { done <- pumpPacketsWithDelay(context.Background(), delay, src, dst, direction) }()
			for i := range 4 {
				src.batchReads <- batchResult{packets: []packet.Packet{&packet.NetworkStackLatency{Timestamp: int64(i)}}}
			}
			src.batchReads <- batchResult{err: io.EOF}
			synctest.Wait()
			if got := dst.flushedBatches(); len(got) != 0 {
				t.Fatal("delivered before deadline")
			}
			if err := <-done; !errors.Is(err, io.EOF) {
				t.Fatal(err)
			}
			if elapsed := time.Since(start); elapsed != 200*time.Millisecond {
				t.Fatalf("pipeline elapsed %s, want one delay", elapsed)
			}
			got := dst.flushedBatches()
			if len(got) != 4 {
				t.Fatalf("got %d batches", len(got))
			}
			for i, batch := range got {
				if batch[0].(*packet.NetworkStackLatency).Timestamp != int64(i) {
					t.Fatal("reordered")
				}
			}
			_ = src.Close()
		}
	})
}

func TestPacketDelayDisableResetAndExpiryDrain(t *testing.T) {
	for _, action := range []string{"disable", "reenable", "reset", "expiry"} {
		t.Run(action, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				delay := new(PacketDelay)
				_ = delay.Set(MaxPacketDelayMS)
				if action == "expiry" {
					time.Sleep(PacketDelayLease - 100*time.Millisecond)
				}
				src, dst := newFakeDownstream(nil), newFakeUpstream(nil)
				src.useBatchReads = true
				done := make(chan error, 1)
				go func() { done <- pumpPacketsWithDelay(context.Background(), delay, src, dst, true) }()
				src.batchReads <- batchResult{packets: stamps(1)[0]}
				src.batchReads <- batchResult{err: io.EOF}
				synctest.Wait()
				if len(dst.flushedBatches()) != 0 {
					t.Fatal("early delivery")
				}
				start := time.Now()
				switch action {
				case "reenable":
					_ = delay.Set(0)
					_ = delay.Set(MaxPacketDelayMS)
				case "disable":
					_ = delay.Set(0)
				case "reset":
					delay.Reset()
				}
				if err := <-done; !errors.Is(err, io.EOF) {
					t.Fatal(err)
				}
				want := time.Duration(0)
				if action == "expiry" {
					want = 100 * time.Millisecond
				}
				if got := time.Since(start); got != want {
					t.Fatalf("drain elapsed %s, want %s", got, want)
				}
				if len(dst.flushedBatches()) != 1 {
					t.Fatal("lost batch")
				}
				_ = src.Close()
			})
		})
	}
}

func TestPacketDelayLeaseRenewalAndValidation(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		delay := new(PacketDelay)
		if got, _, _, _ := delay.snapshot(time.Now()); got != 0 {
			t.Fatal("nonzero default")
		}
		if err := delay.Set(MaxPacketDelayMS + 1); err == nil {
			t.Fatal("accepted excessive delay")
		}
		_ = delay.Set(250)
		time.Sleep(2 * time.Second)
		_ = delay.Set(250)
		time.Sleep(2 * time.Second)
		if got, _, _, _ := delay.snapshot(time.Now()); got != 250*time.Millisecond {
			t.Fatal("heartbeat did not renew")
		}
		time.Sleep(time.Second)
		if got, _, _, _ := delay.snapshot(time.Now()); got != 0 {
			t.Fatal("expired lease remains active")
		}
		delay.Reset()
		if got, _, _, _ := delay.snapshot(time.Now()); got != 0 {
			t.Fatal("session reset retains delay")
		}
	})
}

func TestPacketDelayCancellationAndBoundedReadAhead(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		delay := new(PacketDelay)
		_ = delay.Set(MaxPacketDelayMS)
		src := &countingSource{fakeDownstream: newFakeDownstream(nil)}
		src.useBatchReads = true
		dst := newFakeUpstream(nil)
		ctx, cancel := context.WithCancel(context.Background())
		done := make(chan error, 1)
		go func() { done <- pumpPacketsWithDelay(ctx, delay, src, dst, true) }()
		go func() {
			for range 100 {
				select {
				case src.batchReads <- batchResult{packets: stamps(1)[0]}:
				case <-ctx.Done():
					return
				}
			}
		}()
		synctest.Wait()
		if n := src.reads.Load(); n != maxDelayedBatches+1 {
			t.Fatalf("read ahead %d, want %d", n, maxDelayedBatches+1)
		}
		cancel()
		if err := <-done; !errors.Is(err, context.Canceled) {
			t.Fatal(err)
		}
		_ = src.Close()
	})
}

func TestPacketDelayChangedDeadlineKeepsFIFO(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		delay := new(PacketDelay)
		_ = delay.Set(400)
		src, dst := newFakeDownstream(nil), newFakeUpstream(nil)
		src.useBatchReads = true
		done := make(chan error, 1)
		start := time.Now()
		go func() { done <- pumpPacketsWithDelay(context.Background(), delay, src, dst, true) }()
		src.batchReads <- batchResult{packets: stamps(1)[0]}
		synctest.Wait()
		time.Sleep(10 * time.Millisecond)
		_ = delay.Set(100)
		src.batchReads <- batchResult{packets: stamps(2)[1]}
		src.batchReads <- batchResult{err: io.EOF}
		if err := <-done; !errors.Is(err, io.EOF) {
			t.Fatal(err)
		}
		if time.Since(start) != 400*time.Millisecond {
			t.Fatal("later batch bypassed FIFO deadline")
		}
		got := dst.flushedBatches()
		if len(got) != 2 || got[0][0].(*packet.NetworkStackLatency).Timestamp != 0 || got[1][0].(*packet.NetworkStackLatency).Timestamp != 1 {
			t.Fatal("changed delay reordered batches")
		}
		_ = src.Close()
	})
}

func TestPacketDelayByteBackpressureAndReleasedPayload(t *testing.T) {
	var queue delayedBatches
	queue.push(batchReadResult{packets: []minecraft.RawPacket{{Data: make([]byte, maxDelayedBytes)}}}, time.Time{})
	if !queue.full() {
		t.Fatal("byte budget did not apply backpressure")
	}
	queue.pop()
	if queue.full() || queue.bytes != 0 || queue.count != 0 || queue.items[0].result.packets != nil {
		t.Fatal("drained queue retained packet bytes")
	}
}

func TestPacketDelayNewRelaySessionResetsLease(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		delay := new(PacketDelay)
		_ = delay.Set(MaxPacketDelayMS)
		down, up := newFakeDownstream(nil), newFakeUpstream(nil)
		down.useBatchReads = true
		down.batchReads <- batchResult{packets: stamps(1)[0]}
		down.batchReads <- batchResult{err: io.EOF}
		start := time.Now()
		if err := relayPackets(context.Background(), down, up, func() { _ = down.Close(); _ = up.Close() }, delay); err != nil {
			t.Fatal(err)
		}
		if !time.Now().Equal(start) {
			t.Fatal("new relay inherited previous session delay")
		}
		if current, _, _, _ := delay.snapshot(time.Now()); current != 0 {
			t.Fatal("new session retained lease")
		}
	})
}

func TestPacketDelayOldRelayExitCannotResetNewSession(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		delay := new(PacketDelay)
		oldDown, oldUp := newFakeDownstream(nil), newFakeUpstream(nil)
		oldDown.useBatchReads = true
		oldDown.batchReads <- batchResult{err: io.EOF}
		stopping, release := make(chan struct{}), make(chan struct{})
		oldDone := make(chan error, 1)
		go func() {
			oldDone <- relayPackets(context.Background(), oldDown, oldUp, func() {
				close(stopping)
				<-release
				_ = oldDown.Close()
				_ = oldUp.Close()
			}, delay)
		}()
		<-stopping
		newDown, newUp := newFakeDownstream(nil), newFakeUpstream(nil)
		newDown.useBatchReads = true
		ctx, cancel := context.WithCancel(context.Background())
		newDone := make(chan error, 1)
		go func() {
			newDone <- relayPackets(ctx, newDown, newUp, func() { _ = newDown.Close(); _ = newUp.Close() }, delay)
		}()
		synctest.Wait()
		_ = delay.Set(400)
		close(release)
		if err := <-oldDone; err != nil {
			t.Error(err)
		}
		current, _, _, _ := delay.snapshot(time.Now())
		cancel()
		if err := <-newDone; !errors.Is(err, context.Canceled) {
			t.Error(err)
		}
		if current != 400*time.Millisecond {
			t.Fatal("old relay reset the newly renewed session")
		}
		if current, _, _, _ := delay.snapshot(time.Now()); current != 0 {
			t.Fatal("current relay did not reset on exit")
		}
	})
}
