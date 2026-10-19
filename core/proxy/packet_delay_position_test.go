package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"math"
	"net"
	"testing"
	"testing/synctest"
	"time"

	"github.com/go-gl/mathgl/mgl32"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
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
		go func() { done <- pumpPacketsWithDelay(ctx, delay, nil, src, dst, true, session) }()
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

// movementBatchSource can model one received packet decoded into several movements.
type movementBatchSource struct {
	*fakeDownstream
	combined bool
}

// ReadBatchRaw optionally combines the decoded movements into one received packet.
func (source *movementBatchSource) ReadBatchRaw(decode func(uint32) bool) ([]minecraft.RawPacket, error) {
	batch, err := source.fakeDownstream.ReadBatchRaw(decode)
	if source.combined && len(batch) > 1 {
		for _, raw := range batch[1:] {
			batch[0].Decoded = append(batch[0].Decoded, raw.Decoded...)
		}
		batch = batch[:1]
	}
	return batch, err
}

func TestPacketDelayPositionTracksOnlyMovementsForwardedByInterceptor(t *testing.T) {
	for _, combined := range []bool{false, true} {
		for _, action := range []string{"drop all", "drop first", "drop last", "modify first", "forward all"} {
			t.Run(fmt.Sprintf("%s/combined=%t", action, combined), func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					delay := new(PacketDelay)
					session := delay.beginSession()
					_ = delay.SetWithPosition(200, true)
					src := &movementBatchSource{fakeDownstream: newFakeDownstream(nil), combined: combined}
					defer src.Close()
					src.useBatchReads = true
					dst := newFakeUpstream(nil)
					dst.data.EntityRuntimeID = 9
					first := &packet.PlayerAuthInput{Position: mgl32.Vec3{1, 70, 3}}
					last := &packet.MovePlayer{EntityRuntimeID: 9, Position: mgl32.Vec3{4, 71, 6}}
					src.batchReads <- batchResult{packets: []packet.Packet{first, last}}
					src.batchReads <- batchResult{err: io.EOF}
					intercept := &scriptedIntercept{
						selects: map[uint32]bool{packet.IDPlayerAuthInput: true, packet.IDMovePlayer: true},
						handle: func(_ Direction, value packet.Packet) (Verdict, error) {
							if action == "forward all" {
								return Forward, nil
							}
							if action == "drop first" {
								if value == first {
									return Drop, nil
								}
								return Forward, nil
							}
							if action == "drop all" || value == last {
								return Drop, nil
							}
							if action == "modify first" {
								first.Position = mgl32.Vec3{7, 72, 9}
								return ForwardModified, nil
							}
							return Forward, nil
						},
					}
					if err := pumpPacketsWithDelay(context.Background(), delay, intercept, src, dst, true, session); !errors.Is(err, io.EOF) {
						t.Fatal(err)
					}
					var want *ForwardedPosition
					wantWrites := 1
					switch action {
					case "drop all":
						wantWrites = 0
					case "drop last":
						want = &ForwardedPosition{1, 70, 3}
					case "drop first":
						want = &ForwardedPosition{4, 71, 6}
					case "modify first":
						want = &ForwardedPosition{7, 72, 9}
					case "forward all":
						want, wantWrites = &ForwardedPosition{4, 71, 6}, 2
					}
					_, got := delay.PositionSnapshot()
					if (got == nil) != (want == nil) || got != nil && *got != *want {
						t.Fatalf("position = %#v, want %#v", got, want)
					}
					if got := len(dst.written()); got != wantWrites {
						t.Fatalf("forwarded %d packets, want %d", got, wantWrites)
					}
				})
			})
		}
	}
}

func TestPacketDelayPositionFiltersOwnFiniteMovement(t *testing.T) {
	decoded := func(value packet.Packet) []packet.Packet {
		return []packet.Packet{value}
	}
	if ownMovementPosition(decoded(&packet.MovePlayer{EntityRuntimeID: 8, Position: mgl32.Vec3{1, 2, 3}}), 9) != nil {
		t.Fatal("observed another player")
	}
	if ownMovementPosition(decoded(&packet.MovePlayer{EntityRuntimeID: 9, Mode: packet.MoveModeRotation, Position: mgl32.Vec3{1, 2, 3}}), 9) != nil {
		t.Fatal("rotation-only update moved witness")
	}
	if ownMovementPosition(decoded(&packet.PlayerAuthInput{Position: mgl32.Vec3{1, float32(math.NaN()), 3}}), 9) != nil {
		t.Fatal("observed nonfinite position")
	}
	if got := ownMovementPosition(decoded(&packet.MovePlayer{EntityRuntimeID: 9, Position: mgl32.Vec3{1, 2, 3}}), 9); got == nil || got.Y != 2 {
		t.Fatal("own network position was changed")
	}
}

func TestPacketDelayPositionIncludesInterceptorInjections(t *testing.T) {
	for _, phase := range []string{"before", "after", "flush"} {
		for _, drop := range []bool{false, true} {
			t.Run(fmt.Sprintf("%s/drop_original=%t", phase, drop), func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					delay := new(PacketDelay)
					id := delay.beginSession()
					_ = delay.SetWithPosition(200, true)
					up := newFakeUpstream(nil)
					up.useBatchReads = true
					up.data.EntityRuntimeID = 9
					up.batchReads <- batchResult{packets: []packet.Packet{&packet.StartGame{EntityRuntimeID: 9}}}
					logger := slog.New(slog.NewTextHandler(io.Discard, nil))
					connections := newPreparedConnections("", nil, logger)
					connections.connectPrepared = func(context.Context, dialerDownstream) (*preparedConnection, error) {
						return &preparedConnection{upstream: up, packStack: &selectedResourcePackStack{}}, nil
					}
					var intercepted *InterceptedSession
					// inject appends a movement through the interceptor's public writer.
					inject := func() error {
						return intercepted.WriteToServer(&packet.MovePlayer{EntityRuntimeID: 9, Position: mgl32.Vec3{4, 71, 6}})
					}
					intercept := &scriptedIntercept{
						selects: map[uint32]bool{packet.IDPlayerAuthInput: true},
						handle: func(Direction, packet.Packet) (Verdict, error) {
							if phase == "before" {
								if err := inject(); err != nil {
									return Forward, err
								}
							}
							if drop {
								return Drop, nil
							}
							return Forward, nil
						},
						end: func(Direction) error {
							if phase == "before" {
								return nil
							}
							if err := inject(); err != nil {
								return err
							}
							if phase == "flush" {
								return intercepted.FlushServer()
							}
							return nil
						},
					}
					server := &sessionServer{prepared: connections, delay: delay, logger: logger,
						interceptor: interceptorFunc(func(_ context.Context, session *InterceptedSession) (SessionInterceptor, error) {
							intercepted = session
							return intercept, nil
						})}
					local, peer := net.Pipe()
					defer peer.Close()
					client := newSessionConn(streamnet.NewFramedConn(local))
					defer client.Close()
					ctx, cancel := context.WithCancelCause(context.Background())
					defer cancel(nil)
					downstream, err := newSessionDownstream(testSessionConnect(t), nil)
					if err != nil {
						t.Fatal(err)
					}
					_, prepared, err := server.prepare(ctx, cancel, client, downstream, nil)
					if err != nil {
						t.Fatal(err)
					}
					defer prepared.close()
					src := newFakeDownstream(nil)
					defer src.Close()
					src.useBatchReads = true
					src.batchReads <- batchResult{packets: []packet.Packet{&packet.PlayerAuthInput{Position: mgl32.Vec3{1, 70, 3}}}}
					src.batchReads <- batchResult{err: io.EOF}
					if err := pumpPacketsWithDelay(ctx, delay, prepared.intercept, src, prepared.upstream, true, id); !errors.Is(err, io.EOF) {
						t.Fatal(err)
					}
					want := ForwardedPosition{4, 71, 6}
					if phase == "before" && !drop {
						want = ForwardedPosition{1, 70, 3}
					}
					if _, got := delay.PositionSnapshot(); got == nil || *got != want {
						t.Fatalf("injected movement position = %#v", got)
					}
				})
			})
		}
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
		if err := pumpPacketsWithDelay(context.Background(), delay, nil, src, dst, true); !errors.Is(err, io.EOF) {
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
