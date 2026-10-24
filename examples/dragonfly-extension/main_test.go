package main

import (
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"testing"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/cmd"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/biome"
	"github.com/df-mc/dragonfly/server/world/generator"
)

// TestExampleConfigUsesTemporaryStorage checks that configuration opens no persistent stores.
func TestExampleConfigUsesTemporaryStorage(t *testing.T) {
	dir := t.TempDir()
	uc := exampleConfig("127.0.0.1:19132", filepath.Join(dir, "resources"))
	uc.World.Folder = filepath.Join(dir, "world")
	uc.Players.Folder = filepath.Join(dir, "players")
	conf, err := uc.Config(slog.Default())
	if err != nil {
		t.Fatal(err)
	}
	if conf.WorldProvider != nil {
		t.Cleanup(func() { _ = conf.WorldProvider.Close() })
		t.Error("temporary worlds must not open a persistent provider")
	}
	if conf.PlayerProvider != nil {
		t.Cleanup(func() { _ = conf.PlayerProvider.Close() })
		t.Error("temporary players must not open a persistent provider")
	}
	for _, path := range []string{uc.World.Folder, uc.Players.Folder} {
		if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
			t.Errorf("persistent storage %s exists or cannot be checked: %v", path, err)
		}
	}
}

// TestTravelUsesDestinationSpawn checks a round trip without a dangerous drop above either surface.
func TestTravelUsesDestinationSpawn(t *testing.T) {
	world.DefaultBlockRegistry.Finalize()
	app := &application{ext: &cinnabarext.Server{}}
	for i := range app.worlds {
		app.worlds[i] = (world.Config{
			Synchronous: true,
			Dim:         world.Overworld,
			Generator:   generator.NewFlat(biome.Plains{}, []world.Block{block.Bedrock{}, block.Dirt{}, block.Grass{}}),
		}).New()
		w := app.worlds[i]
		t.Cleanup(func() { _ = w.Close() })
		spawn := w.Spawn()
		spawn[0], spawn[2] = i*16, -i*16
		w.SetSpawn(spawn)
	}
	handle := world.NewEntity(player.Type, player.Config{Position: app.worlds[0].Spawn().Vec3Centre()})
	app.current.Store(handle.UUID(), uint16(0))
	app.worlds[0].Do(func(tx *world.Tx) { tx.AddEntity(handle) })
	current := uint16(0)
	for _, next := range []uint16{1, 0} {
		if err := app.worlds[current].Do(func(tx *world.Tx) {
			e, ok := handle.Entity(tx)
			if !ok {
				t.Error("player is missing from the source world")
				return
			}
			travel{app: app}.Run(e.(*player.Player), &cmd.Output{}, tx)
		}).Err(); err != nil {
			t.Fatal(err)
		}
		if err := app.worlds[next].Do(func(tx *world.Tx) {
			e, ok := handle.Entity(tx)
			if !ok {
				t.Error("player is missing from the destination world")
				return
			}
			pos := e.Position()
			if want := tx.World().Spawn().Vec3Centre(); pos != want {
				t.Errorf("travel position = %v, want destination spawn %v", pos, want)
			}
			spawn := tx.World().Spawn()
			surface := float64(tx.HighestBlock(spawn[0], spawn[2]) + 1)
			if pos[1] < surface || pos[1] > surface+2 {
				t.Errorf("travel height %v is not safely above surface %v", pos[1], surface)
			}
		}).Err(); err != nil {
			t.Fatal(err)
		}
		current = next
	}
}
