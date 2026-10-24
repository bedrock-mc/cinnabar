package experience

import (
	"math"
	"path/filepath"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// TestInstanceCloseDiscardsOnlyTemporaryPlacements checks quota refunds across unload and shutdown.
func TestInstanceCloseDiscardsOnlyTemporaryPlacements(t *testing.T) {
	for _, shutdown := range []bool{false, true} {
		t.Run(map[bool]string{false: "unload", true: "shutdown"}[shutdown], func(t *testing.T) {
			f := newRuntimeFixture(t)
			def := WorldDefinition{ID: "probe:saved", Settings: WorldSettings{Generator: GeneratorEmpty, Dimension: DimensionOverworld, Persistent: true}}
			f.h.manager.definitions[def.ID] = def
			saved, err := f.h.createWorld(t.Context(), "probe", WorldInstance{ID: "probe:saved", Definition: def.ID})
			if err != nil {
				t.Fatal(err)
			}
			temporary, _ := f.h.worlds.handle("probe:one")
			for _, exp := range []string{"probe", "other"} {
				for _, handle := range []WorldHandle{temporary, saved} {
					key := Key{Instance: handle.ID, Y: 70}
					f.h.store.Place(exp, key)
					if err := f.h.store.SetData(exp, key, []byte("data"), true); err != nil {
						t.Fatal(err)
					}
				}
			}
			if err := f.h.store.Flush(); err != nil {
				t.Fatal(err)
			}
			if shutdown {
				if err := f.h.Close(); err != nil {
					t.Fatal(err)
				}
			} else {
				for _, handle := range []WorldHandle{temporary, saved} {
					if err := f.h.unloadWorld(t.Context(), "probe", handle); err != nil {
						t.Fatal(err)
					}
				}
			}
			if err := f.h.store.Flush(); err != nil {
				t.Fatal(err)
			}
			for _, store := range []*Store{f.h.store, openTestStore(t, f.h.store.dir)} {
				for _, exp := range []string{"probe", "other"} {
					if _, ok := store.Token(exp, Key{Instance: temporary.ID, Y: 70}); ok {
						t.Error("discarded terrain retained placement")
					}
					if data, ok := store.Data(exp, Key{Instance: saved.ID, Y: 70}); !ok || string(data) != "data" {
						t.Error("persistent placement lost")
					}
					if store.Budget(exp) != dataQuota-4 {
						t.Errorf("budget = %d, want %d", store.Budget(exp), dataQuota-4)
					}
				}
			}
		})
	}
}

// TestPersistentInstanceInitializesSpawn preserves an explicitly saved spawn on later openings.
func TestPersistentInstanceInitializesSpawn(t *testing.T) {
	for _, gen := range []Generator{GeneratorEmpty, GeneratorFlat} {
		t.Run(string(gen), func(t *testing.T) {
			f := newRuntimeFixture(t)
			def := WorldDefinition{ID: "probe:saved", Settings: WorldSettings{Generator: gen, Dimension: DimensionOverworld, Persistent: true}}
			f.h.manager.definitions[def.ID] = def
			instance := WorldInstance{ID: "probe:saved", Definition: def.ID}
			handle, err := f.h.createWorld(t.Context(), "probe", instance)
			if err != nil {
				t.Fatal(err)
			}
			w, _ := f.h.ResolveWorld(handle)
			var want cube.Pos
			if gen == GeneratorFlat {
				want = cube.Pos{0, world.Overworld.Range().Min() + 4, 0}
			}
			if w.Spawn() != want {
				t.Errorf("new spawn = %v, want %v", w.Spawn(), want)
			}
			saved := cube.Pos{12, 85, -9}
			w.SetSpawn(saved)
			if err := f.h.unloadWorld(t.Context(), "probe", handle); err != nil {
				t.Fatal(err)
			}
			handle, err = f.h.createWorld(t.Context(), "probe", instance)
			if err != nil {
				t.Fatal(err)
			}
			w, _ = f.h.ResolveWorld(handle)
			if w.Spawn() != saved {
				t.Errorf("saved spawn = %v, want %v", w.Spawn(), saved)
			}
		})
	}
}

// TestDisconnectReconcilesUnpolledWorldChange models a native transfer followed immediately by quit.
func TestDisconnectReconcilesUnpolledWorldChange(t *testing.T) {
	f := newRuntimeFixture(t)
	<-f.d.events // Join precedes the native world switch.
	handle, _ := f.h.worlds.handle("probe:one")
	destination, _ := f.h.ResolveWorld(handle)
	f.do(f.source, func(tx *world.Tx) { e, _ := f.actor.Entity(tx); tx.RemoveEntity(e) })
	f.do(destination, func(tx *world.Tx) {
		p := tx.AddEntity(f.actor).(*player.Player)
		f.h.PlayerDisconnected(p)
		tx.RemoveEntity(p)
		_ = f.actor.Close()
	})
	for _, kind := range []string{"world_change", "leave"} {
		select {
		case ev := <-f.d.events:
			if ev.call.Player.Event.Type != kind {
				t.Fatalf("event = %s, want %s", ev.call.Player.Event.Type, kind)
			}
			loc := ev.call.Player.Event.Location
			if loc == nil {
				loc = ev.call.Player.Event.After
			}
			if loc.World != handle || loc.Epoch != 2 || ev.w != destination || ev.instance.handle != handle {
				t.Fatalf("wrong destination: %+v", ev)
			}
			if kind != "leave" {
				continue
			}
			snap, err := f.h.snapshot(t.Context(), f.d, &ev)
			if err != nil {
				t.Fatal(err)
			}
			data := "abcd"
			f.do(destination, func(tx *world.Tx) {
				_, err := f.h.apply(tx, "probe", ev, snap, []Op{{StoreWrite: &StoreWriteOp{Key: StoreKey{Scope: StorePlayer, Schema: 1}, Data: &data}}})
				if err != nil {
					t.Fatalf("final private write: %v", err)
				}
			})
			got := f.h.store.readStates("probe", []stateAddress{{Scope: StorePlayer, Owner: f.actor.UUID().String()}})[0]
			if got.Data == nil || *got.Data != data {
				t.Fatal("final private write lost")
			}
		default:
			t.Fatalf("missing %s", kind)
		}
	}
	if len(f.d.events) != 0 {
		t.Fatal("duplicate observation")
	}
}

// TestFailedInstanceAdmissionReleasesResources rolls back terrain and pause ownership after open.
func TestFailedInstanceAdmissionReleasesResources(t *testing.T) {
	tempRoot := t.TempDir()
	t.Setenv("TMPDIR", tempRoot)
	lifecycle := &lifecycleProbe{opened: make(map[*world.World]bool)}
	f := newRuntimeFixture(t, lifecycle)
	f.h.store.nextWorldGeneration = math.MaxUint64
	key := Key{Instance: "probe:failed", Y: 70}
	f.h.store.Place("probe", key)
	if err := f.h.store.SetData("probe", key, []byte("old"), true); err != nil {
		t.Fatal(err)
	}
	if _, err := f.h.createWorld(t.Context(), "probe", WorldInstance{ID: key.Instance, Definition: "probe:plain"}); err == nil {
		t.Fatal("exhausted generation admitted an instance")
	}
	if len(lifecycle.opened) != 2 {
		t.Fatal("failed admission retained pause ownership")
	}
	backing, err := filepath.Glob(filepath.Join(tempRoot, "cinnabar-world-*"))
	if err != nil || len(backing) != 2 {
		t.Fatalf("failed admission retained terrain: %v, %v", backing, err)
	}
	if _, ok := f.h.store.Token("probe", key); ok || f.h.store.Budget("probe") != dataQuota {
		t.Fatal("failed admission retained discarded placement data")
	}
	if _, ok := f.h.worlds.handle(key.Instance); ok {
		t.Fatal("failed instance remained bound")
	}
}
