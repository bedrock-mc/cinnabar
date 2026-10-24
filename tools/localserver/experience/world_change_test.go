package experience

import (
	"testing"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/google/uuid"
)

// nativePositionHandler connects Dragonfly's teleport hook to the production admission operation.
type nativePositionHandler struct {
	player.NopHandler
	host *Host
}

// HandleTeleport replaces an authoritative teleport with the fenced host operation.
func (h nativePositionHandler) HandleTeleport(ctx *player.Context, pos mgl64.Vec3) {
	if h.host.TeleportPlayer(ctx.Player(), positionOf(pos)) {
		ctx.Cancel()
	}
}

// positionViewer records the same teleport notification consumed by a controlling session.
type positionViewer struct {
	world.NopViewer
	seen      chan struct{}
	positions []mgl64.Vec3
}

// ViewEntity signals that the loader has registered its viewer for the player's chunk.
func (v *positionViewer) ViewEntity(world.Entity) {
	select {
	case v.seen <- struct{}{}:
	default:
	}
}

// ViewEntityTeleport records the position delivered through Dragonfly's viewer contract.
func (v *positionViewer) ViewEntityTeleport(_ world.Entity, pos mgl64.Vec3) {
	v.positions = append(v.positions, pos)
}

// TestSameInstanceTransferSynchronizesPosition observes the client's position notification after same-world admission.
func TestSameInstanceTransferSynchronizesPosition(t *testing.T) {
	f := newRuntimeFixture(t)
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	handle, _ := f.h.worlds.handle("probe:one")
	destination, _ := f.h.ResolveWorld(handle)
	viewer := &positionViewer{seen: make(chan struct{}, 1)}
	var loader *world.Loader
	f.do(destination, func(tx *world.Tx) {
		e, _ := f.actor.Entity(tx)
		e.(*player.Player).SetImmobile()
		loader = world.NewLoader(1, destination, viewer)
		loader.Load(tx, 1)
	})
	defer f.do(destination, func(tx *world.Tx) { loader.Close(tx) })
	select {
	case <-viewer.seen:
	case <-t.Context().Done():
		t.Fatal("player chunk never became visible")
	}
	ev, snap := f.request()
	want := Position{X: 8, Y: 75, Z: 8}
	op := Op{World: &WorldActionOp{Action: WorldOp{Type: "transfer", Player: f.actor.UUID().String(), Instance: handle, Position: want}}}
	if err := f.h.commitLifecycle(t.Context(), f.d, ev, snap, []Op{op}); err != nil {
		t.Fatal(err)
	}
	f.do(destination, func(*world.Tx) {
		if len(viewer.positions) != 1 || viewer.positions[0] != want.vec() {
			t.Fatalf("client position notifications = %v", viewer.positions)
		}
	})
}

// TestNativeWorldChangeFencesClientInput rejects source envelopes after a sampled native swap.
func TestNativeWorldChangeFencesClientInput(t *testing.T) {
	for _, native := range []string{"portal", "respawn"} {
		t.Run(native, func(t *testing.T) {
			m := newLifecycleMatrix(t)
			m.step("handshake")
			m.part.oldInput()
			if m.part.admitted != 1 {
				t.Fatal("source input rejected")
			}
			m.step(native)
			m.part.oldInput()
			if m.part.admitted != 1 {
				t.Fatal("native swap accepted old-world input")
			}
			epoch := m.part.acknowledgeWorld()
			m.part.inputAt(epoch)
			if m.part.admitted != 2 {
				t.Fatal("destination input remained fenced after acknowledgment")
			}
			m.drain()
		})
	}
}

// TestReconnectAfterTransferAcceptsV1 uses the same server after a committed transfer and default-world fallback.
func TestReconnectAfterTransferAcceptsV1(t *testing.T) {
	m := newLifecycleMatrix(t)
	m.step("handshake")
	m.step("commit")
	m.step("disconnect")
	m.step("unload")
	m.step("reconnect")
	m.part.hello.Wire = nil
	m.step("handshake")
	m.part.oldInput()
	if m.part.admitted != 1 {
		t.Fatal("new v1 login inherited the committed transfer")
	}
	m.check()
	m.drain()
}

// TestNativeSwapInvalidatesQueuedInput fences a native move before the next scheduled sample.
func TestNativeSwapInvalidatesQueuedInput(t *testing.T) {
	f := newRuntimeFixture(t)
	part := newLifecyclePart(t, f)
	part.handshake()
	<-f.d.events
	part.oldInput()
	ev := <-f.d.events
	handle, _ := f.h.worlds.handle("probe:one")
	destination, _ := f.h.ResolveWorld(handle)
	f.do(f.source, func(tx *world.Tx) { e, _ := f.actor.Entity(tx); tx.RemoveEntity(e) })
	f.do(destination, func(tx *world.Tx) { tx.AddEntity(f.actor) })
	if _, err := f.h.snapshot(t.Context(), f.d, &ev); err == nil {
		t.Fatal("queued source input acquired destination authority")
	}
	before := part.admitted
	part.oldInput()
	if part.admitted != before {
		t.Fatal("source epoch survived callback-time native detection")
	}
}

// TestSameWorldNativeTeleportFencesInput covers respawn placement without a pointer change.
func TestSameWorldNativeTeleportFencesInput(t *testing.T) {
	m := newLifecycleMatrix(t)
	m.step("handshake")
	m.part.oldInput()
	before := m.f.h.epoch(m.f.actor)
	if err := m.f.actor.Do(func(_ *world.Tx, e world.Entity) { e.(*player.Player).Teleport(mgl64.Vec3{4, 75, 6}) }).Wait(t.Context()); err != nil {
		t.Fatal(err)
	}
	if m.f.h.epoch(m.f.actor) != before+1 {
		t.Fatal("same-world authoritative teleport retained old authority")
	}
	m.part.oldInput()
	if m.part.admitted != 1 {
		t.Fatal("same-world respawn accepted stale input")
	}
	m.part.inputAt(m.part.acknowledgeWorld())
	if m.part.admitted != 2 {
		t.Fatal("destination input remained fenced")
	}
	m.drain()
}

// TestNativeChangeFallsBackV1WithoutBlockingHost preserves native travel when an optional client cannot follow it.
func TestNativeChangeFallsBackV1WithoutBlockingHost(t *testing.T) {
	m := newLifecycleMatrix(t)
	m.part.hello.Wire = nil
	m.step("handshake")
	m.part.oldInput()
	m.step("portal")
	m.part.oldInput()
	if m.part.admitted != 1 {
		t.Fatal("v1 source traffic crossed native travel")
	}
	if !m.f.h.playerCurrent(event{actor: m.f.actor, epoch: m.f.h.epoch(m.f.actor)}) {
		t.Fatal("optional fallback blocked ordinary destination gameplay")
	}
	m.drain()
}

// TestClientSnapshotPreservesPendingMovement keeps callback-time world detection from consuming the next move observation.
func TestClientSnapshotPreservesPendingMovement(t *testing.T) {
	f := newRuntimeFixture(t)
	<-f.d.events
	f.do(f.source, func(tx *world.Tx) { e, _ := f.actor.Entity(tx); e.(*player.Player).Move(mgl64.Vec3{2, 0, 0}, 0, 0) })
	f.request()
	if err := f.h.PollPlayers(t.Context(), 1); err != nil {
		t.Fatal(err)
	}
	select {
	case ev := <-f.d.events:
		if ev.call.Player == nil || ev.call.Player.Event.Type != "move" || ev.call.Player.Event.Before.Position.X != 0 || ev.call.Player.Event.After.Position.X != 2 {
			t.Fatal("callback snapshot replaced the pending movement endpoints")
		}
	default:
		t.Fatal("callback snapshot consumed the pending movement observation")
	}
}

// TestRetiredTransportStillRecordsNativeDeparture preserves final destination history after socket teardown.
func TestRetiredTransportStillRecordsNativeDeparture(t *testing.T) {
	f := newRuntimeFixture(t)
	part := newLifecyclePart(t, f)
	part.handshake()
	<-f.d.events
	handle, _ := f.h.worlds.handle("probe:one")
	destination, _ := f.h.ResolveWorld(handle)
	f.do(f.source, func(tx *world.Tx) { e, _ := f.actor.Entity(tx); tx.RemoveEntity(e) })
	if err := part.conn.Close(); err != nil {
		t.Fatal(err)
	}
	f.do(destination, func(tx *world.Tx) {
		p := tx.AddEntity(f.actor).(*player.Player)
		f.h.PlayerDisconnected(p)
		tx.RemoveEntity(p)
		_ = f.actor.Close()
	})
	assertDestinationDeparture(t, f, handle)
}

// assertDestinationDeparture requires ordered destination observations after a native swap and quit.
func assertDestinationDeparture(t *testing.T, f *runtimeFixture, handle WorldHandle) {
	t.Helper()
	for _, kind := range []string{"world_change", "leave"} {
		select {
		case ev := <-f.d.events:
			observation := ev.call.Player.Event
			loc := observation.Location
			if loc == nil {
				loc = observation.After
			}
			if observation.Type != kind || loc.World != handle || loc.Epoch != 2 {
				t.Fatalf("departure observation = %+v, location %+v", observation, loc)
			}
		default:
			t.Fatalf("missing %s observation", kind)
		}
	}
}

// TestDisconnectDuringTransferPreparationRecordsNativeDeparture fences a preparation superseded by native quit.
func TestDisconnectDuringTransferPreparationRecordsNativeDeparture(t *testing.T) {
	f := newRuntimeFixture(t)
	<-f.d.events
	handle, _ := f.h.worlds.handle("probe:one")
	destination, _ := f.h.ResolveWorld(handle)
	first := true
	f.setRuntimeChannels(runtimeChannels{prepare: func() (*cinnabarext.WorldChange, error) {
		if first {
			first = false
			f.do(f.source, func(tx *world.Tx) { e, _ := f.actor.Entity(tx); tx.RemoveEntity(e) })
			f.do(destination, func(tx *world.Tx) {
				p := tx.AddEntity(f.actor).(*player.Player)
				f.h.PlayerDisconnected(p)
				tx.RemoveEntity(p)
				_ = f.actor.Close()
			})
		}
		return nil, nil
	}})
	if err := f.travel(t.Context(), "probe:two"); err == nil {
		t.Fatal("departed session completed transfer")
	}
	assertDestinationDeparture(t, f, handle)
}

// TestExpiredWorldNoticeAllowsLaterPlacement preserves native placement and transfers after optional failure.
func TestExpiredWorldNoticeAllowsLaterPlacement(t *testing.T) {
	for _, placement := range []string{"tp", "same_world_respawn", "transfer"} {
		t.Run(placement, func(t *testing.T) {
			m := newLifecycleMatrix(t)
			publicationErr := m.expireGrantDuringTransfer()
			before := m.f.h.epoch(m.f.actor)
			want := mgl64.Vec3{8, 75, 6}
			if placement == "transfer" {
				if err := m.f.travel(t.Context(), "probe:two"); err != nil {
					t.Fatalf("later transfer failed: %v", err)
				}
			} else {
				if err := m.f.actor.Do(func(tx *world.Tx, e world.Entity) {
					p := e.(*player.Player)
					if placement == "same_world_respawn" {
						tx.RemoveEntity(p)
						p = tx.AddEntity(m.f.actor).(*player.Player)
					}
					p.Teleport(want)
					if p.Position() != want {
						t.Fatalf("%s position = %v, want %v", placement, p.Position(), want)
					}
				}).Wait(t.Context()); err != nil {
					t.Fatal(err)
				}
			}
			if publicationErr != nil {
				t.Fatalf("optional publication failure rejected placement: %v", publicationErr)
			}
			if m.f.h.epoch(m.f.actor) != before+1 || m.f.countLocations() != 1 {
				t.Fatal("later placement did not retain one fresh authoritative location")
			}
			m.part.oldInput()
			if m.part.admitted != 0 {
				t.Fatal("expired client part retained callback authority")
			}
			if !m.f.h.playerCurrent(event{actor: m.f.actor, epoch: m.f.h.epoch(m.f.actor)}) {
				t.Fatal("optional failure blocked ordinary gameplay")
			}
			m.drain()
		})
	}
}

// TestWorldChangePanicReleasesRelocation allows another placement after recovery at each boundary.
func TestWorldChangePanicReleasesRelocation(t *testing.T) {
	for _, boundary := range []string{"prepare", "relocate", "publish"} {
		t.Run(boundary, func(t *testing.T) {
			f := newRuntimeFixture(t)
			if boundary == "prepare" {
				f.setRuntimeChannels(runtimeChannels{prepare: func() (*cinnabarext.WorldChange, error) { panic("prepare") }})
			}
			if boundary == "publish" {
				f.h.worldChanged = func(uuid.UUID, WorldHandle, uint64) bool { panic("publish") }
			}
			f.do(f.source, func(tx *world.Tx) {
				defer func() {
					if recover() != boundary {
						t.Fatal("missing expected panic")
					}
				}()
				_ = f.h.changeWorld(f.actor, cinnabarext.CoordinatedChange, func(admit func(*player.Player), _ *PlayerLocation) error {
					if boundary == "relocate" {
						panic("relocate")
					}
					e, _ := f.actor.Entity(tx)
					admit(e.(*player.Player))
					return nil
				})
			})
			f.setRuntimeChannels(runtimeChannels{})
			f.h.worldChanged = nil
			if err := f.travel(t.Context(), "probe:one"); err != nil {
				t.Fatal(err)
			}
		})
	}
}

// TestFailedPublicationDisablesClientPart preserves placement while ending the old grant's authority.
func TestFailedPublicationDisablesClientPart(t *testing.T) {
	m := newLifecycleMatrix(t)
	m.step("handshake")
	m.f.h.worldChanged = func(uuid.UUID, WorldHandle, uint64) bool { return false }
	if err := m.f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatalf("optional notice failure rejected placement: %v", err)
	}
	m.f.h.worldChanged = nil
	m.part.inputAt(m.part.acknowledgeWorld())
	if m.part.admitted != 0 {
		t.Fatal("failed publication retained client-part authority after acknowledgment")
	}
	if err := m.f.travel(t.Context(), "probe:two"); err != nil {
		t.Fatalf("failed publication blocked later travel: %v", err)
	}
	m.step("reconnect")
	m.step("handshake")
	m.part.oldInput()
	if m.part.admitted != 1 {
		t.Fatal("new handshake did not establish fresh client-part authority")
	}
	m.drain()
}
