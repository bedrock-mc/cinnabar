package experience

import (
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"testing"
	"testing/synctest"
	"time"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// TestTemporaryInstanceRetainsUnloadedTerrain revisits a modified chunk during one opening.
func TestTemporaryInstanceRetainsUnloadedTerrain(t *testing.T) {
	tempRoot := t.TempDir()
	t.Setenv("TMPDIR", tempRoot)
	synctest.Test(t, func(t *testing.T) {
		registration := &WorldRegistration{Definitions: []WorldDefinition{{ID: "probe:temporary", Settings: WorldSettings{Generator: GeneratorEmpty, Dimension: DimensionOverworld}}}, Startup: []WorldInstance{{ID: "probe:temporary", Definition: "probe:temporary"}}}
		h := NewHost(&Registry{}, openTestStore(t, t.TempDir()), map[string]*Supervisor{"probe": {registered: Loaded{Worlds: registration}}}, "world", nil, slog.New(slog.DiscardHandler))
		defer func() {
			if err := h.Close(); err != nil {
				t.Error(err)
			}
			// The database releases its memory pool on a final virtual timer.
			synctest.Wait()
			<-time.After(time.Second)
			synctest.Wait()
		}()
		if err := h.ConfigureWorlds(t.Context(), t.TempDir()); err != nil {
			t.Fatal(err)
		}
		backing, err := filepath.Glob(filepath.Join(tempRoot, "cinnabar-world-*"))
		if err != nil || len(backing) != 1 {
			t.Fatalf("temporary backing: %v, %v", backing, err)
		}
		handle, _ := h.worlds.handle("probe:temporary")
		w, _ := h.ResolveWorld(handle)
		if w.Spawn() != (cube.Pos{}) {
			t.Fatal("temporary backing changed the empty generator spawn")
		}
		pos := cube.Pos{0, 70, 0}
		if err := w.Do(func(tx *world.Tx) { tx.SetBlock(pos, block.Stone{}, nil) }).Wait(t.Context()); err != nil {
			t.Fatal(err)
		}
		// Advance the virtual clock past chunk eviction and drain its transaction.
		<-time.After(3 * time.Minute)
		synctest.Wait()
		if err := w.Do(func(tx *world.Tx) {
			if _, loaded := tx.BlockLoaded(pos); loaded {
				t.Error("chunk was not evicted")
			}
			if tx.Block(pos) != (block.Stone{}) {
				t.Error("temporary terrain lost during opening")
			}
		}).Wait(t.Context()); err != nil {
			t.Fatal(err)
		}
		if err := h.unloadWorld(t.Context(), "probe", handle); err != nil {
			t.Fatal(err)
		}
		if _, err := os.Stat(backing[0]); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("closed instance left backing storage: %v", err)
		}
		fresh, err := h.createWorld(t.Context(), "probe", WorldInstance{ID: handle.ID, Definition: "probe:temporary"})
		if err != nil {
			t.Fatal(err)
		}
		w, _ = h.ResolveWorld(fresh)
		if err := w.Do(func(tx *world.Tx) {
			if tx.Block(pos) == (block.Stone{}) {
				t.Error("temporary terrain survived instance close")
			}
		}).Wait(t.Context()); err != nil {
			t.Fatal(err)
		}
	})
}

// TestSavedDefinitionRejectsTemporaryReplacement preserves saved instance identity across mode changes.
func TestSavedDefinitionRejectsTemporaryReplacement(t *testing.T) {
	f := newRuntimeFixture(t)
	def := WorldDefinition{ID: "probe:saved", Settings: WorldSettings{Generator: GeneratorEmpty, Dimension: DimensionOverworld, Persistent: true}}
	f.h.manager.definitions[def.ID] = def
	instance := WorldInstance{ID: "probe:saved", Definition: def.ID}
	handle, err := f.h.createWorld(t.Context(), "probe", instance)
	if err != nil {
		t.Fatal(err)
	}
	if err := f.h.unloadWorld(t.Context(), "probe", handle); err != nil {
		t.Fatal(err)
	}
	def.Settings.Persistent = false
	f.h.manager.definitions[def.ID] = def
	if _, err := f.h.createWorld(t.Context(), "probe", instance); err == nil {
		t.Fatal("saved persistent instance reopened as temporary terrain")
	}
}

// TestPlayerObservationsCarryInventory permits live inventory writes while retaining stale-slot checks.
func TestPlayerObservationsCarryInventory(t *testing.T) {
	f := newRuntimeFixture(t)
	for _, kind := range []string{"join", "move", "world_change", "leave"} {
		switch kind {
		case "move":
			f.do(f.source, func(tx *world.Tx) { e, _ := f.actor.Entity(tx); e.(*player.Player).Teleport(mgl64.Vec3{1, 70, 0}) })
			if err := f.h.PollPlayers(t.Context(), 1); err != nil {
				t.Fatal(err)
			}
		case "world_change":
			if err := f.travel(t.Context(), "probe:one"); err != nil {
				t.Fatal(err)
			}
		case "leave":
			if err := f.actor.Do(func(tx *world.Tx, e world.Entity) { f.h.PlayerDisconnected(e.(*player.Player)) }).Wait(t.Context()); err != nil {
				t.Fatal(err)
			}
		}
		ev := <-f.d.events
		if ev.call.Player.Event.Type != kind {
			t.Fatalf("event = %s, want %s", ev.call.Player.Event.Type, kind)
		}
		snap, err := f.h.snapshot(t.Context(), f.d, &ev)
		if err != nil {
			t.Fatal(err)
		}
		if kind == "leave" {
			if snap.req.Inventory != nil || snap.req.Actor != nil || len(snap.slots) != 0 {
				t.Fatal("leave retained live inventory authority")
			}
			continue
		}
		if snap.req.Inventory == nil || len(snap.slots) != inventorySlots {
			t.Fatalf("%s inventory unavailable", kind)
		}
		ops := []Op{{SetSlot: &SetSlotOp{Slot: 0, Stack: &NewStack{ID: "minecraft:stone", Count: 3}}}}
		f.do(ev.w, func(tx *world.Tx) {
			if _, err := f.h.apply(tx, "probe", ev, snap, ops); err != nil {
				t.Fatalf("%s set slot: %v", kind, err)
			}
			e, _ := f.actor.Entity(tx)
			stack, _ := e.(*player.Player).Inventory().Item(0)
			if stack.Count() != 3 {
				t.Fatal("slot write did not reach the player")
			}
			_ = e.(*player.Player).Inventory().SetItem(0, item.NewStack(block.Dirt{}, snap.slots[0].Count()+1))
			if _, err := f.h.apply(tx, "probe", ev, snap, ops); !errors.Is(err, errStale) {
				t.Fatalf("changed slot accepted: %v", err)
			}
		})
	}
}

// lifecycleProbe tracks worlds before they become available to callbacks.
type lifecycleProbe struct{ opened map[*world.World]bool }

// WorldOpened marks each admitted world and gives it non-default local settings.
func (p *lifecycleProbe) WorldOpened(w *world.World) {
	p.opened[w] = true
	w.SetPaused(true)
	w.SetDifficulty(world.DifficultyPeaceful)
}

// WorldClosed releases one world from lifecycle ownership.
func (p *lifecycleProbe) WorldClosed(w *world.World) { delete(p.opened, w) }

// TestInstanceLifecycleOwnsStartupAndRuntimeWorlds covers the common factory and both close paths.
func TestInstanceLifecycleOwnsStartupAndRuntimeWorlds(t *testing.T) {
	p := &lifecycleProbe{opened: make(map[*world.World]bool)}
	f := newRuntimeFixture(t, p)
	if len(p.opened) != 2 {
		t.Fatal("startup instances bypassed lifecycle owner")
	}
	runtime, err := f.h.createWorld(t.Context(), "probe", WorldInstance{ID: "probe:runtime", Definition: "probe:plain"})
	if err != nil {
		t.Fatal(err)
	}
	if len(p.opened) != 3 {
		t.Fatal("runtime instance bypassed lifecycle owner")
	}
	for w := range p.opened {
		if !w.Paused() || w.Difficulty() != world.DifficultyPeaceful {
			t.Fatal("instance was admitted before initialization")
		}
	}
	if err := f.h.unloadWorld(t.Context(), "probe", runtime); err != nil {
		t.Fatal(err)
	}
	if len(p.opened) != 2 {
		t.Fatal("unloaded instance retained lifecycle owner")
	}
	if err := f.h.Close(); err != nil {
		t.Fatal(err)
	}
	if len(p.opened) != 0 {
		t.Fatal("host close retained lifecycle worlds")
	}
}
