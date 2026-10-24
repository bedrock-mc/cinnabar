package experience

import (
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
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

// ViewEntity records guest actors introduced through normal world visibility.
func (v *entityViewer) ViewEntity(e world.Entity) {
	if _, ok := e.(*modEntity); ok {
		v.adds++
	}
}

// ViewEntityMovement records authoritative guest actor movement.
func (v *entityViewer) ViewEntityMovement(e world.Entity, _ mgl64.Vec3, _ cube.Rotation, _ bool) {
	if _, ok := e.(*modEntity); ok {
		v.moves++
	}
}

// HideEntity records guest actors removed from this viewer.
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
	var despawn *event
	for len(f.d.events) > 0 {
		ev := <-f.d.events
		if ev.call.Entity != nil && ev.call.Entity.Event.Type == "despawned" {
			copy := ev
			despawn = &copy
		}
	}
	if despawn == nil {
		t.Fatal("unload omitted despawn observation")
	}
	snap, err := f.h.snapshot(t.Context(), f.d, despawn)
	if err != nil || snap.req.Actor != nil || snap.req.Info.Context.Current != worldHandle {
		t.Fatalf("removal observation lost its opening fence: %v", err)
	}
}

func TestEntityRegistrationBuildsResourcesAndPreservesFrameworkFactories(t *testing.T) {
	dir := t.TempDir()
	visual, err := visualExperience(dir, "probe", false)
	if err != nil {
		t.Fatal(err)
	}
	controller := filepath.Join(dir, "ship.render.json")
	if err := os.WriteFile(controller, []byte(`{"render_controllers":{"controller.render.probe.ship":{"geometry":"Geometry.default","textures":["Texture.default"],"materials":[{"*":"Material.default"}]}}}`), 0600); err != nil {
		t.Fatal(err)
	}
	def := entityDefinition()
	def.Appearance = EntityAppearance{Type: "custom", Art: &EntityArt{Geometry: *visual.Blocks[0].Visual.Geometry, Texture: visual.Blocks[0].Visual.Materials[0].Path, RenderController: controller}}
	typ, err := newModEntityType("probe", def, newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	registry := (&Registry{entityTypes: map[string]*modEntityType{def.ID: typ}}).EntityRegistry()
	if found, ok := registry.Lookup(def.ID); !ok || found != typ {
		t.Fatal("entity missing from framework registry")
	}
	if registry.Config().Item == nil || registry.Config().Arrow == nil {
		t.Fatal("built-in factories lost")
	}
	resources := typ.Resources()
	var client struct {
		Entity struct {
			Description struct {
				Identifier string `json:"identifier"`
			} `json:"description"`
		} `json:"minecraft:client_entity"`
	}
	if json.Unmarshal(resources.ClientEntity, &client) != nil || client.Entity.Description.Identifier != def.ID {
		t.Fatal("wrong client entity identity")
	}
	var behaviour struct {
		Entity struct {
			Description struct {
				Identifier string `json:"identifier"`
			} `json:"description"`
		} `json:"minecraft:entity"`
	}
	if json.Unmarshal(resources.Definition, &behaviour) != nil || behaviour.Entity.Description.Identifier != def.ID {
		t.Fatal("wrong behaviour identity")
	}
	for _, name := range []string{"models/entity/probe/ship.geo.json", "textures/entity/probe/ship.png", "render_controllers/probe/ship.json"} {
		if len(resources.Assets[name]) == 0 {
			t.Errorf("missing resource %s", name)
		}
	}
	schema := typ.EntityPropertyDefinitions()
	properties := schema["properties"].([]any)
	if len(properties) != 1 || properties[0].(map[string]any)["name"] != def.Properties[0].ID {
		t.Fatal("property schema differs from registration")
	}
}

func TestEntityBuiltinAppearanceRetainsCustomIdentityAndBaseClass(t *testing.T) {
	def := entityDefinition()
	typ, err := newModEntityType("probe", def, newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	if typ.EncodeEntity() != def.ID || typ.BaseEntityIdentifier() != def.Appearance.ID {
		t.Fatal("appearance replaced the custom identity or lost its base")
	}
	var behaviour struct {
		Entity struct {
			Description struct {
				Identifier string `json:"identifier"`
				Runtime    string `json:"runtime_identifier"`
			} `json:"description"`
		} `json:"minecraft:entity"`
	}
	if err := json.Unmarshal(typ.Resources().Definition, &behaviour); err != nil {
		t.Fatal(err)
	}
	if behaviour.Entity.Description.Identifier != def.ID || behaviour.Entity.Description.Runtime != def.Appearance.ID {
		t.Fatal("behaviour pack lost the custom or base identity")
	}
}

func TestEntityInvalidBatchPublishesNothing(t *testing.T) {
	f := newEntityFixture(t)
	handle := f.spawnEntity()
	f.d.seq++
	ev, snap := f.request()
	recorder := &tellRecorder{}
	f.h.tell = recorder
	invalid := snap.req.Info.Context.Entities[0].State
	invalid.Scale = -1
	f.do(ev.w, func(tx *world.Tx) {
		ops := []Op{{Tell: &TellOp{Player: f.actor.UUID().String(), Text: "must not commit"}}, {Entity: &EntityCommand{Action: EntityOp{Type: "update", State: &invalid}}}}
		if _, err := f.h.apply(tx, "probe", ev, snap, ops); !errors.Is(err, errInvalid) {
			t.Fatalf("invalid state accepted: %v", err)
		}
		e, _ := f.h.findEntity(tx, "probe", handle)
		if e.Scale() != 1 {
			t.Fatal("invalid state changed actor")
		}
	})
	if len(recorder.all()) != 0 {
		t.Fatal("batch partially published")
	}
}

func TestPlayerEntitySnapshotsRetainObservationAndFenceDeparture(t *testing.T) {
	f := newEntityFixture(t)
	handle := f.spawnEntity()
	f.do(f.source, func(tx *world.Tx) {
		e, _ := f.actor.Entity(tx)
		instance, _ := f.h.worlds.of(tx.World())
		f.h.players.mu.Lock()
		defer f.h.players.mu.Unlock()
		location := f.h.players.items[f.actor].location
		f.h.observeLocked(e.(*player.Player), instance, PlayerEvent{Type: "join", Location: &location})
	})
	ev := <-f.d.events
	f.d.seq++
	snap, err := f.h.snapshot(t.Context(), f.d, &ev)
	if err != nil || len(snap.req.Info.Context.Entities) != 1 || snap.req.Info.Context.Entities[0].State.Handle != handle {
		t.Fatalf("player observation lost its owned actor: %v", err)
	}
	f.do(f.source, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, "probe", ev, snap, []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "mount", Handle: handle}}}}); err != nil {
			t.Fatalf("live player observation could not board: %v", err)
		}
	})
	if err := f.travel(t.Context(), "probe:one"); err != nil {
		t.Fatal(err)
	}
	historical, err := f.h.snapshot(t.Context(), f.d, &ev)
	if err != nil || len(historical.req.Info.Context.Entities) != 1 {
		t.Fatalf("departure discarded the historical observation: %v", err)
	}
	f.do(f.source, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, "probe", ev, historical, []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "mount", Handle: handle}}}}); !errors.Is(err, errStale) {
			t.Fatalf("departed player's actor authority survived: %v", err)
		}
	})
}

func TestUnrepresentableMountRejectsWholeEntityBatch(t *testing.T) {
	f := newEntityFixture(t)
	def := entityDefinition()
	def.Seats[0].Position = Position{X: 1.5e308, Z: 1.5e308}
	typ, err := newModEntityType("probe", def, newAssetCache())
	if err != nil {
		t.Fatal(err)
	}
	typ.host = f.h
	f.h.reg.entityTypes[def.ID] = typ
	f.d.sup.registered.Entities = []EntityDefinition{def}
	f.d.seq++
	ev, snap := f.request()
	handle := EntityHandle{snap.req.Info.EventSequence*maxStagedOps + 2, ev.instance.handle.Generation}
	state := EntityState{Handle: handle, Kind: def.ID, Position: Position{X: 8, Y: 70, Z: 8}, Rotation: EntityRotation{Yaw: 45}, Scale: 1, Properties: []EntityProperty{{ID: def.Properties[0].ID}}}
	data := "01"
	f.do(ev.w, func(tx *world.Tx) {
		ops := []Op{
			{StoreWrite: &StoreWriteOp{Key: StoreKey{Scope: StoreInstance, Schema: 1}, Data: &data}},
			{Entity: &EntityCommand{Action: EntityOp{Type: "spawn", State: &state}}},
			{Entity: &EntityCommand{Action: EntityOp{Type: "mount", Handle: handle}}},
		}
		if _, err := f.h.apply(tx, "probe", ev, snap, ops); !errors.Is(err, errInvalid) {
			t.Fatalf("unrepresentable mount accepted: %v", err)
		}
		if len(f.h.entitySnapshot(tx, "probe")) != 0 {
			t.Fatal("refused mount left its staged actor")
		}
	})
	address := stateAddress{Scope: StoreInstance, Owner: string(ev.instance.handle.ID)}
	if record := f.h.store.readStates("probe", []stateAddress{address})[0]; record.Data != nil {
		t.Fatal("refused mount changed private state")
	}
}
