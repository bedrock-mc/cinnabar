package main

import (
	"io"
	"log/slog"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

func TestConfiguredServerSupportsPassiveActors(t *testing.T) {
	s := settings{dir: t.TempDir(), addr: "127.0.0.1:0", generator: "normal", seed: 42, gameMode: "creative", diff: "normal"}
	conf, err := s.userConfig().Config(slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	generators, err := s.configureGenerators(&conf)
	if err != nil {
		t.Fatal(err)
	}
	defer generators.close()
	srv := conf.New()
	srv.Listen()
	defer srv.Close()
	for _, name := range []string{"minecraft:cow", "minecraft:sheep", "minecraft:pig", "minecraft:chicken"} {
		if _, ok := srv.World().EntityRegistry().Lookup(name); !ok {
			t.Errorf("configured server cannot create or reopen %s", name)
		}
	}
}

func TestNormalWorldCreatesPassiveWorldActors(t *testing.T) {
	s := settings{dir: t.TempDir(), addr: "127.0.0.1:0", generator: "normal", seed: 42, gameMode: "creative", diff: "normal"}
	conf, err := s.userConfig().Config(slog.New(slog.NewTextHandler(io.Discard, nil)))
	if err != nil {
		t.Fatal(err)
	}
	generators, err := s.configureGenerators(&conf)
	if err != nil {
		t.Fatal(err)
	}
	defer generators.close()
	defer conf.WorldProvider.Close()
	defer conf.PlayerProvider.Close()
	w := world.Config{Generator: conf.Generator(world.Overworld), Entities: conf.Entities, Synchronous: true, RandomTickSpeed: -1}.New()
	defer w.Close()
	s.applyTo(w)
	s.configureAnimals(w)
	w.SetTickRange(4)
	w.SetTime(6000)
	w.StopTime()
	loader := world.NewLoader(4, w, world.NopViewer{})
	w.Do(func(tx *world.Tx) {
		spawn := generators[world.Overworld].DefaultSpawn(world.Overworld)
		at := mgl64.Vec3{float64(spawn[0]) + .5, float64(tx.HighestLightBlocker(spawn[0], spawn[2]) + 1), float64(spawn[2]) + .5}
		tx.AddEntity(world.NewEntity(player.Type, player.Config{Position: at, GameMode: world.GameModeCreative}))
		loader.Move(tx, at)
		loader.Load(tx, 100)
	})
	for range 2000 {
		w.AdvanceTick()
	}
	w.Do(func(tx *world.Tx) {
		count := 0
		for e := range tx.Entities() {
			if a, ok := e.(*entity.Animal); ok {
				count++
				if a.Dead() {
					t.Error("natural actor is already dead")
				}
				feet := cube.PosFromVec3(a.Position())
				name, _ := tx.Block(feet.Side(cube.FaceDown)).EncodeBlock()
				if name != "minecraft:grass" && name != "minecraft:grass_block" {
					t.Errorf("actor was admitted on %s", name)
				}
			}
		}
		if count == 0 {
			t.Fatal("normal terrain with an active player produced no passive world actors")
		}
		loader.Close(tx)
	})
}
