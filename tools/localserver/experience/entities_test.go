package experience

import (
	"errors"
	"slices"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// entityDefinition is an owned ship with two distinguishable seats and one property.
func entityDefinition() EntityDefinition {
	zero := 0.0
	return EntityDefinition{ID: "probe:ship", Collision: EntityBox{Min: Position{X: -0.5, Z: -0.5}, Max: Position{X: 0.5, Y: 1, Z: 0.5}}, Appearance: EntityAppearance{Type: "vanilla", ID: "minecraft:boat"}, Seats: []EntitySeat{{Position: Position{Y: 1}, RotationLimit: &zero, Steers: true}, {Position: Position{Y: 1}}}, Properties: []EntityPropertyDef{{ID: "probe:throttle", Min: -1, Max: 1}}}
}

// newEntityFixture adds entity registration to the synchronous world fixture.
func newEntityFixture(t *testing.T) *runtimeFixture {
	t.Helper()
	f := newRuntimeFixture(t)
	typ, err := newModEntityType("probe", entityDefinition(), newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	typ.host = f.h
	f.h.reg.entityTypes = map[string]*modEntityType{typ.def.ID: typ}
	f.d.sup.registered.Entities = []EntityDefinition{typ.def}
	for len(f.d.events) > 0 {
		<-f.d.events
	}
	return f
}

// spawnEntity stages a new identity in the same way as the guest callback.
func (f *runtimeFixture) spawnEntity() EntityHandle {
	f.t.Helper()
	f.d.seq++
	ev, snap := f.request()
	handle := EntityHandle{snap.req.Info.EventSequence*maxStagedOps + 1, ev.instance.handle.Generation}
	state := EntityState{Handle: handle, Kind: "probe:ship", Position: Position{Y: 70}, Scale: 1, Properties: []EntityProperty{{ID: "probe:throttle"}}}
	f.do(ev.w, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, "probe", ev, snap, []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "spawn", State: &state}}}}); err != nil {
			f.t.Fatal(err)
		}
	})
	return handle
}

// entityViewer records normal world replication for one nearby client.
type entityViewer struct {
	world.NopViewer
	adds, moves, removes int
}

func (v *entityViewer) ViewEntity(e world.Entity) {
	if _, ok := e.(*modEntity); ok {
		v.adds++
	}
}
func (v *entityViewer) ViewEntityMovement(e world.Entity, _ mgl64.Vec3, _ cube.Rotation, _ bool) {
	if _, ok := e.(*modEntity); ok {
		v.moves++
	}
}
func (v *entityViewer) HideEntity(e world.Entity) {
	if _, ok := e.(*modEntity); ok {
		v.removes++
	}
}

// watchEntityChunk attaches a viewer after its chunk is resident.
func (f *runtimeFixture) watchEntityChunk(w *world.World, v *entityViewer) {
	f.t.Helper()
	f.do(w, func(tx *world.Tx) {
		_ = tx.Block(cube.Pos{0, 70, 0})
		loader := world.NewLoader(0, w, v)
		loader.Move(tx, mgl64.Vec3{0, 70, 0})
		loader.Load(tx, 1)
	})
}

func TestEntityReplicationStaleHandlesAndWorldIsolation(t *testing.T) {
	f := newEntityFixture(t)
	otherHandle, _ := f.h.worlds.handle("probe:one")
	other, _ := f.h.ResolveWorld(otherHandle)
	a, b, outside := &entityViewer{}, &entityViewer{}, &entityViewer{}
	f.watchEntityChunk(f.source, a)
	f.watchEntityChunk(f.source, b)
	f.watchEntityChunk(other, outside)
	handle := f.spawnEntity()
	if a.adds != 1 || b.adds != 1 || outside.adds != 0 {
		t.Fatalf("spawn viewers: %+v %+v %+v", a, b, outside)
	}
	f.d.seq++
	ev, snap := f.request()
	state := snap.req.Info.Context.Entities[0].State
	state.Position.X = 2
	state.Rotation.Yaw = 90
	state.Velocity.X = 0.25
	state.NameTag = "Ship"
	state.Scale = 1.5
	state.Variant = 2
	state.Properties[0].Value = 0.5
	f.do(f.source, func(tx *world.Tx) {
		ops := []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "update", State: &state}}}}
		if _, err := f.h.apply(tx, "probe", ev, snap, ops); err != nil {
			t.Fatal(err)
		}
		e, ok := f.h.findEntity(tx, "probe", handle)
		if !ok || e.NameTag() != "Ship" || e.Variant() != 2 || e.Scale() != 1.5 || e.EntityProperties().FloatProperties[0].Value != 0.5 {
			t.Fatal("metadata not applied")
		}
		if _, err := f.h.apply(tx, "probe", ev, snap, ops); !errors.Is(err, errStale) {
			t.Fatalf("stale revision accepted: %v", err)
		}
	})
	if a.moves != 1 || b.moves != 1 || outside.moves != 0 {
		t.Fatalf("movement viewers: %+v %+v %+v", a, b, outside)
	}
	f.d.seq++
	ev, snap = f.request()
	f.do(f.source, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, "probe", ev, snap, []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "despawn", Handle: handle}}}}); err != nil {
			t.Fatal(err)
		}
		if _, ok := f.h.findEntity(tx, "probe", handle); ok {
			t.Fatal("despawned handle resolved")
		}
		if _, err := f.h.apply(tx, "probe", ev, snap, []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "update", State: &state}}}}); !errors.Is(err, errStale) {
			t.Fatalf("removed actor accepted: %v", err)
		}
	})
	if a.removes != 1 || b.removes != 1 || outside.removes != 0 {
		t.Fatalf("despawn viewers: %+v %+v %+v", a, b, outside)
	}
}

func TestEntitySeatEventsSteeringAndPlayerTransfer(t *testing.T) {
	f := newEntityFixture(t)
	handle := f.spawnEntity()
	f.d.seq++
	ev, snap := f.request()
	f.do(f.source, func(tx *world.Tx) {
		p, _ := f.actor.Entity(tx)
		e, _ := f.h.findEntity(tx, "probe", handle)
		f.h.EntityInteraction(p.(*player.Player), e, false)
		f.h.EntityInteraction(p.(*player.Player), e, true)
		if _, err := f.h.apply(tx, "probe", ev, snap, []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "mount", Handle: handle, Seat: 1}}}}); err != nil {
			t.Fatal(err)
		}
		got := e.snapshot().Riders
		if len(got) != 1 || got[0].Seat != 1 {
			t.Fatalf("identical seat offsets lost seat identity: %v", got)
		}
		tx.Dismount(p)
		if !tx.Mount(p, e, 0) {
			t.Fatal("mount failed")
		}
		tx.HandleRiderInput(p, world.RiderInput{Move: mgl64.Vec2{0.5, 1}, Yaw: 10})
	})
	var observed []string
	for len(f.d.events) > 0 {
		ev := <-f.d.events
		if ev.call.Entity != nil {
			observed = append(observed, ev.call.Entity.Event.Type)
			if ev.epoch != f.h.epoch(f.actor) {
				t.Fatal("entity observation lost player epoch")
			}
		}
	}
	if !slices.Equal(observed, []string{"interact", "attacked", "mounted", "dismounted", "mounted", "steering"}) {
		t.Fatalf("observation order: %v", observed)
	}
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	f.do(f.source, func(tx *world.Tx) {
		e, _ := f.h.findEntity(tx, "probe", handle)
		if len(e.snapshot().Riders) != 0 {
			t.Fatal("transfer retained a rider")
		}
	})
	f.d.seq++
	destination, snapshot := f.request()
	f.do(destination.w, func(tx *world.Tx) {
		state := EntityState{Handle: handle, Kind: "probe:ship", Position: Position{Y: 70}, Scale: 1, Properties: []EntityProperty{{ID: "probe:throttle"}}}
		if _, err := f.h.apply(tx, "probe", destination, snapshot, []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "update", State: &state}}}}); !errors.Is(err, errInvalid) {
			t.Fatalf("cross-world entity handle accepted: %v", err)
		}
	})
}

func TestEntityWorldUnloadClosesHandlesAndLeases(t *testing.T) {
	f := newEntityFixture(t)
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	handle := f.spawnEntity()
	worldHandle, _ := f.h.worlds.handle("probe:one")
	w, _ := f.h.ResolveWorld(worldHandle)
	var native *world.EntityHandle
	f.do(w, func(tx *world.Tx) { e, _ := f.h.findEntity(tx, "probe", handle); native = e.H() })
	if err := f.travel(t.Context(), "probe:two"); err != nil {
		t.Fatal(err)
	}
	if err := f.h.unloadWorld(t.Context(), "probe", worldHandle); err != nil {
		t.Fatal(err)
	}
	if !native.Closed() {
		t.Fatal("unloaded world retained an entity handle")
	}
}
