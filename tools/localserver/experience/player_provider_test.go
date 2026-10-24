package experience

import (
	"os"
	"path/filepath"
	"testing"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/player/playerdb"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// TestReconnectResolvesSavedInstance keeps position identity through storage reopen and instance unload.
func TestReconnectResolvesSavedInstance(t *testing.T) {
	for _, mode := range []string{"open", "unloaded", "reopened", "missing_metadata"} {
		t.Run(mode, func(t *testing.T) {
			f := newRuntimeFixture(t)
			if err := f.travel(t.Context(), "probe:one"); err != nil {
				t.Fatal(err)
			}
			var data player.Config
			handle, _ := f.h.worlds.handle("probe:one")
			guest, _ := f.h.ResolveWorld(handle)
			f.do(guest, func(tx *world.Tx) {
				e, _ := f.actor.Entity(tx)
				p := e.(*player.Player)
				p.Teleport(mgl64.Vec3{321, 84, -123})
				data = p.Data()
				f.h.PlayerDisconnected(p)
				tx.RemoveEntity(p)
				_ = f.actor.Close()
			})
			dir, locations := filepath.Join(t.TempDir(), "players"), t.TempDir()
			provider, err := playerdb.NewProvider(dir)
			if err != nil {
				t.Fatal(err)
			}
			wrapped, err := f.h.PlayerProvider(provider, locations)
			if err != nil {
				t.Fatal(err)
			}
			if err := wrapped.Save(f.actor.UUID(), data, guest); err != nil {
				t.Fatal(err)
			}
			if err := wrapped.Close(); err != nil {
				t.Fatal(err)
			}
			if mode == "unloaded" || mode == "reopened" {
				if err := f.h.unloadWorld(t.Context(), "probe", handle); err != nil {
					t.Fatal(err)
				}
			}
			if mode == "reopened" {
				fresh, err := f.h.createWorld(t.Context(), "probe", WorldInstance{ID: handle.ID, Definition: "probe:plain"})
				if err != nil {
					t.Fatal(err)
				}
				guest, err = f.h.ResolveWorld(fresh)
				if err != nil {
					t.Fatal(err)
				}
			}
			if mode == "missing_metadata" {
				if err := os.Remove(filepath.Join(locations, f.actor.UUID().String()+".json")); err != nil {
					t.Fatal(err)
				}
			}
			provider, err = playerdb.NewProvider(dir)
			if err != nil {
				t.Fatal(err)
			}
			wrapped, err = f.h.PlayerProvider(provider, locations)
			if err != nil {
				t.Fatal(err)
			}
			defer wrapped.Close()
			loaded, w, err := wrapped.Load(f.actor.UUID(), func(world.Dimension) *world.World { return f.source })
			if err != nil {
				t.Fatal(err)
			}
			wantWorld, wantPos := guest, data.Position
			if mode == "unloaded" || mode == "missing_metadata" {
				wantWorld, wantPos = f.source, f.source.Spawn().Vec3Centre()
			}
			if w != wantWorld || loaded.Position != wantPos {
				t.Fatalf("reconnect = %p at %v, want %p at %v", w, loaded.Position, wantWorld, wantPos)
			}
		})
	}
}
