package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"testing"
	"testing/synctest"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// encodedPositionUpstream decodes received bytes independently of mutable test packet objects.
type encodedPositionUpstream struct{ *fakeUpstream }

// WritePacketRaw records the values actually encoded in the forwarded bytes.
func (upstream *encodedPositionUpstream) WritePacketRaw(data []byte) error {
	value := decodeSessionPacket(testPacketPool, data, 0, false)
	if value == nil {
		return errors.New("invalid encoded test packet")
	}
	return upstream.fakeUpstream.WritePacket(value)
}

// discardingPositionUpstream matches the transport's removal of a batch on flush failure.
type discardingPositionUpstream struct {
	*fakeUpstream
	failure error
}

// Flush discards the buffered batch when transport submission fails.
func (upstream *discardingPositionUpstream) Flush() error {
	if upstream.failure != nil {
		upstream.batchesMu.Lock()
		upstream.pendingBatch = nil
		upstream.batchesMu.Unlock()
		return upstream.failure
	}
	return upstream.fakeUpstream.Flush()
}

// WritePacketImmediate submits the immediate packets through the same discarding flush.
func (upstream *discardingPositionUpstream) WritePacketImmediate(values ...packet.Packet) error {
	for _, value := range values {
		if err := upstream.WritePacket(value); err != nil {
			return err
		}
	}
	return upstream.Flush()
}

func TestPacketDelayPositionPreservesEncodedForwardedMovement(t *testing.T) {
	for _, inject := range []bool{false, true} {
		t.Run(fmt.Sprintf("inject=%t", inject), func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				delay := new(PacketDelay)
				id := delay.beginSession()
				_ = delay.SetWithPosition(200, true)
				up := &encodedPositionUpstream{newFakeUpstream(nil)}
				observed := observeUpstreamPositions(up, delay, 9)
				intercepted := &InterceptedSession{server: observed}
				src := newFakeDownstream(nil)
				defer src.Close()
				src.useBatchReads = true
				src.batchReads <- batchResult{packets: []packet.Packet{&packet.PlayerAuthInput{Position: mgl32.Vec3{1, 70, 3}}}}
				src.batchReads <- batchResult{err: io.EOF}
				intercept := &scriptedIntercept{
					selects: map[uint32]bool{packet.IDPlayerAuthInput: true},
					handle: func(_ Direction, value packet.Packet) (Verdict, error) {
						value.(*packet.PlayerAuthInput).Position = mgl32.Vec3{4, 71, 6}
						if inject {
							return Forward, intercepted.WriteToServer(value)
						}
						return Forward, nil
					},
				}
				if err := pumpPacketsWithDelay(context.Background(), delay, intercept, src, observed, true, id); !errors.Is(err, io.EOF) {
					t.Fatal(err)
				}
				written := up.written()
				if got := written[len(written)-1].(*packet.PlayerAuthInput).Position; got != (mgl32.Vec3{1, 70, 3}) {
					t.Fatalf("encoded final movement = %v", got)
				}
				if _, got := delay.PositionSnapshot(); got == nil || *got != (ForwardedPosition{1, 70, 3}) {
					t.Fatalf("position did not match encoded output: %#v", got)
				}
			})
		})
	}
}

func TestPacketDelayPositionDiscardsFailedFlushObservations(t *testing.T) {
	for _, immediate := range []bool{false, true} {
		t.Run(fmt.Sprintf("immediate=%t", immediate), func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				delay := new(PacketDelay)
				id := delay.beginSession()
				_ = delay.SetWithPosition(200, true)
				up := &discardingPositionUpstream{fakeUpstream: newFakeUpstream(nil)}
				writer := trackPositionWrites(up, delay, id, 9)
				if err := writer.WritePacket(&packet.PlayerAuthInput{Position: mgl32.Vec3{1, 70, 3}}); err != nil {
					t.Fatal(err)
				}
				up.failure = errors.New("flush rejected")
				flush := writer.Flush
				if immediate {
					flush = func() error { return writer.WritePacketImmediate(&packet.SetTime{}) }
				}
				if err := flush(); !errors.Is(err, up.failure) {
					t.Fatalf("failed flush = %v", err)
				}
				up.failure = nil
				if err := writer.Flush(); err != nil {
					t.Fatal(err)
				}
				if _, position := delay.PositionSnapshot(); position != nil {
					t.Fatal("an empty successful flush published a discarded movement")
				}
				if len(up.flushedBatches()) != 0 {
					t.Fatal("the failed transport retained its discarded batch")
				}
			})
		})
	}
}

func TestPacketDelayPositionPublishesIdleInterceptorWrites(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		delay := new(PacketDelay)
		id := delay.beginSession()
		_ = delay.SetWithPosition(200, true)
		up := newFakeUpstream(nil)
		observed := observeUpstreamPositions(up, delay, 9)
		intercepted := &InterceptedSession{server: observed}
		src := newFakeDownstream(nil)
		defer src.Close()
		src.useBatchReads = true
		done := make(chan error, 1)
		go func() { done <- pumpPacketsWithDelay(context.Background(), delay, nil, src, observed, true, id) }()
		synctest.Wait()
		if err := intercepted.WriteToServer(&packet.PlayerAuthInput{Position: mgl32.Vec3{1, 70, 3}}); err != nil {
			t.Fatal(err)
		}
		if _, position := delay.PositionSnapshot(); position != nil {
			t.Fatal("an unflushed injection advanced the position")
		}
		src.batchReads <- batchResult{err: io.EOF}
		if err := <-done; !errors.Is(err, io.EOF) {
			t.Fatal(err)
		}
		if _, position := delay.PositionSnapshot(); position == nil || *position != (ForwardedPosition{1, 70, 3}) {
			t.Fatalf("idle flush position = %#v", position)
		}
		if batches := up.flushedBatches(); len(batches) != 1 || len(batches[0]) != 1 {
			t.Fatalf("idle flush batches = %#v", batches)
		}
	})
}

func TestPacketDelayPositionFencesQueuedInterceptorWrites(t *testing.T) {
	for _, action := range []string{"lease", "session", "failed flush"} {
		t.Run(action, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				delay := new(PacketDelay)
				id := delay.beginSession()
				_ = delay.SetWithPosition(200, true)
				up := &discardingPositionUpstream{fakeUpstream: newFakeUpstream(nil)}
				observed := observeUpstreamPositions(up, delay, 9)
				trackPositionWrites(observed, delay, id, 9)
				intercepted := &InterceptedSession{server: observed}
				if err := intercepted.WriteToServer(&packet.PlayerAuthInput{Position: mgl32.Vec3{1, 70, 3}}); err != nil {
					t.Fatal(err)
				}
				failure := errors.New("flush rejected")
				switch action {
				case "lease":
					_ = delay.SetWithPosition(200, false)
					_ = delay.SetWithPosition(200, true)
				case "session":
					delay.beginSession()
				case "failed flush":
					up.failure = failure
				}
				err := intercepted.FlushServer()
				if action == "failed flush" && !errors.Is(err, failure) || action != "failed flush" && err != nil {
					t.Fatalf("flush = %v", err)
				}
				if _, position := delay.PositionSnapshot(); position != nil {
					t.Fatal("a stale or unflushed injection advanced the position")
				}
				if action == "failed flush" {
					up.failure = nil
					if err := intercepted.FlushServer(); err != nil {
						t.Fatal(err)
					}
					if _, position := delay.PositionSnapshot(); position != nil {
						t.Fatal("an empty retry published a discarded movement")
					}
				}
			})
		})
	}
}
