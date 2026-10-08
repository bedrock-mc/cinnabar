package proxy

import (
	"context"
	"errors"
	"io"
	"math"
	"testing"
	"testing/synctest"
	"time"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestPacketDelayPositionAdvancesOnlyAfterSuccessfulUpstreamFlush(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		delay := new(PacketDelay)
		session := delay.beginSession()
		_ = delay.SetWithPosition(200, true)
		src, dst := newFakeDownstream(nil), newFakeUpstream(nil)
		src.useBatchReads = true
		ctx, cancel := context.WithCancel(context.Background())
		defer cancel()
		done := make(chan error, 1)
		go func() { done <- pumpPacketsWithDelay(ctx, delay, src, dst, true, session) }()
		first := &packet.PlayerAuthInput{Position: mgl32.Vec3{1, 70, 3}}
		src.batchReads <- batchResult{packets: []packet.Packet{first}}
		synctest.Wait()
		if _, position := delay.PositionSnapshot(); position != nil {
			t.Fatal("held movement advanced witness")
		}
		time.Sleep(200 * time.Millisecond)
		synctest.Wait()
		if id, position := delay.PositionSnapshot(); id != session || position == nil || *position != (ForwardedPosition{1, 70, 3}) {
			t.Fatalf("successful flush witness: %d %#v", id, position)
		}
		second := &packet.MovePlayer{EntityRuntimeID: dst.data.EntityRuntimeID, Position: mgl32.Vec3{4, 71, 6}}
		src.batchReads <- batchResult{packets: []packet.Packet{second}}
		synctest.Wait()
		if _, position := delay.PositionSnapshot(); position == nil || position.X != 1 {
			t.Fatal("second held packet advanced witness")
		}
		failure := errors.New("flush rejected")
		dst.setFlushErr(failure)
		time.Sleep(200 * time.Millisecond)
		if err := <-done; !errors.Is(err, failure) {
			t.Fatal(err)
		}
		if _, position := delay.PositionSnapshot(); position == nil || position.X != 1 {
			t.Fatal("failed flush advanced witness")
		}
		if got := dst.flushedBatches(); len(got) != 1 || got[0][0] != first {
			t.Fatal("forwarded movement was changed")
		}
		_ = src.Close()
	})
}

func TestPacketDelayPositionFiltersOwnFiniteMovement(t *testing.T) {
	raw := func(value packet.Packet) minecraft.RawPacket {
		return minecraft.RawPacket{Decoded: []packet.Packet{value}}
	}
	if ownMovementPosition(raw(&packet.MovePlayer{EntityRuntimeID: 8, Position: mgl32.Vec3{1, 2, 3}}), 9) != nil {
		t.Fatal("observed another player")
	}
	if ownMovementPosition(raw(&packet.MovePlayer{EntityRuntimeID: 9, Mode: packet.MoveModeRotation, Position: mgl32.Vec3{1, 2, 3}}), 9) != nil {
		t.Fatal("rotation-only update moved witness")
	}
	if ownMovementPosition(raw(&packet.PlayerAuthInput{Position: mgl32.Vec3{1, float32(math.NaN()), 3}}), 9) != nil {
		t.Fatal("observed nonfinite position")
	}
	if got := ownMovementPosition(raw(&packet.MovePlayer{EntityRuntimeID: 9, Position: mgl32.Vec3{1, 2, 3}}), 9); got == nil || got.Y != 2 {
		t.Fatal("own network position was changed")
	}
}

func TestPacketDelayPositionClearsWithoutResurrection(t *testing.T) {
	for _, action := range []string{"disable", "hide", "reset", "session", "expiry"} {
		t.Run(action, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				delay := new(PacketDelay)
				session := delay.beginSession()
				_ = delay.SetWithPosition(200, true)
				epoch, _ := delay.positionToken()
				delay.commitPosition(session, epoch, &ForwardedPosition{1, 2, 3})
				if _, position := delay.PositionSnapshot(); position == nil {
					t.Fatal("missing initial witness")
				}
				switch action {
				case "disable":
					_ = delay.SetWithPosition(0, true)
				case "hide":
					_ = delay.SetWithPosition(200, false)
				case "reset":
					delay.Reset()
				case "session":
					delay.beginSession()
				case "expiry":
					time.Sleep(PacketDelayLease)
				}
				if _, position := delay.PositionSnapshot(); position != nil {
					t.Fatal("revoked witness leaked")
				}
				_ = delay.SetWithPosition(200, true)
				if _, position := delay.PositionSnapshot(); position != nil {
					t.Fatal("renewal resurrected old witness")
				}
				delay.commitPosition(session, epoch, &ForwardedPosition{4, 5, 6})
				if _, position := delay.PositionSnapshot(); position != nil {
					t.Fatal("stale in-flight flush resurrected old witness")
				}
			})
		})
	}
}

type positionDecodeSource struct {
	*fakeDownstream
	decoded int
}

func (source *positionDecodeSource) ReadBatchRaw(decode func(uint32) bool) ([]minecraft.RawPacket, error) {
	return source.fakeDownstream.ReadBatchRaw(func(id uint32) bool {
		enabled := decode != nil && decode(id)
		if enabled && isOwnMovement(id) {
			source.decoded++
		}
		return enabled
	})
}

func TestPacketDelayPositionOffDoesNotDecodeMovement(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		delay := new(PacketDelay)
		_ = delay.SetWithPosition(10, false)
		src := &positionDecodeSource{fakeDownstream: newFakeDownstream(nil)}
		src.useBatchReads = true
		dst := newFakeUpstream(nil)
		src.batchReads <- batchResult{packets: []packet.Packet{&packet.PlayerAuthInput{Position: mgl32.Vec3{1, 2, 3}}}}
		src.batchReads <- batchResult{err: io.EOF}
		if err := pumpPacketsWithDelay(context.Background(), delay, src, dst, true); !errors.Is(err, io.EOF) {
			t.Fatal(err)
		}
		if src.decoded != 0 {
			t.Fatal("decoded movement while witness disabled")
		}
		if _, position := delay.PositionSnapshot(); position != nil {
			t.Fatal("published position while disabled")
		}
		_ = src.Close()
	})
}
