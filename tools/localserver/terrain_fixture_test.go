package main

import (
	"bytes"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"reflect"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

// TestTerrainFixtureOptIn preserves the normal generators and validates the offline generation flags.
func TestTerrainFixtureOptIn(t *testing.T) {
	base := []string{"-dir", "unused", "-addr", "127.0.0.1:0"}
	for _, enabled := range []bool{false, true} {
		args := append([]string{}, base...)
		if enabled {
			args = append(args, "-terrain-fixture")
		}
		s, err := parseSettings(args, io.Discard)
		if err != nil {
			t.Fatal(err)
		}
		conf := server.Config{}
		s.configureTerrainFixture(&conf)
		if enabled {
			if conf.Generator == nil {
				t.Fatal("fixture generator was not enabled")
			}
			if _, ok := conf.Generator(world.Overworld).(terrainFixture); !ok || conf.RandomTickSpeed >= 0 || conf.ChunkLoadWorkers <= 1 {
				t.Fatal("fixture generator and work settings were not enabled")
			}
		} else if conf.Generator != nil || conf.RandomTickSpeed != 0 || conf.ChunkLoadWorkers != 0 {
			t.Fatal("default generator or work settings changed")
		}
	}
	if s, err := parseSettings([]string{"-dir", "unused", "-terrain-fixture-generate"}, io.Discard); err != nil || !s.terrainFixtureGenerate {
		t.Fatalf("offline generation needs no listen address: %v", err)
	}
	for _, radius := range []int{terrainMinRadius - 1, terrainMaxRadius + 1} {
		if _, err := parseSettings(append(base, "-terrain-fixture-radius", fmt.Sprint(radius)), io.Discard); err == nil {
			t.Fatalf("invalid radius %d accepted", radius)
		}
	}
}

// TestTerrainFixtureWeather leaves normal worlds alone and clears existing storms for fixture captures.
func TestTerrainFixtureWeather(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		w := world.Config{RandomTickSpeed: -1}.New()
		w.StartThundering(time.Hour)
		s := settings{gameMode: "creative", diff: "peaceful", terrainFixture: enabled}
		s.applyTo(w)
		var raining, thundering bool
		if err := w.Do(func(tx *world.Tx) { raining, thundering = tx.Raining(), tx.Thundering() }).Wait(t.Context()); err != nil {
			t.Fatal(err)
		}
		if w.TimeCycle() == enabled || raining == enabled || thundering == enabled {
			t.Errorf("fixture=%v: time cycle=%v rain=%v thunder=%v", enabled, w.TimeCycle(), raining, thundering)
		}
		if enabled && w.Time() != terrainTime {
			t.Errorf("fixture time = %d", w.Time())
		}
		if err := w.Close(); err != nil {
			t.Fatal(err)
		}
	}
}

// TestTerrainFixtureRefusesExistingDatabase protects directories, files, and symlinks before opening a database.
func TestTerrainFixtureRefusesExistingDatabase(t *testing.T) {
	for _, kind := range []string{"directory", "file", "symlink"} {
		t.Run(kind, func(t *testing.T) {
			dir := t.TempDir()
			path := filepath.Join(dir, "db")
			switch kind {
			case "directory":
				if err := os.Mkdir(path, 0700); err != nil {
					t.Fatal(err)
				}
			case "file":
				if err := os.WriteFile(path, []byte("saved world"), 0600); err != nil {
					t.Fatal(err)
				}
			case "symlink":
				if err := os.Symlink(filepath.Join(dir, "missing"), path); err != nil {
					t.Skipf("symlink creation unavailable: %v", err)
				}
			}
			before, err := os.Lstat(path)
			if err != nil {
				t.Fatal(err)
			}
			var out bytes.Buffer
			err = generateTerrainFixture(settings{dir: dir, terrainFixtureRadius: terrainDefaultRadius}, &out)
			if !errors.Is(err, os.ErrExist) || out.Len() != 0 {
				t.Fatalf("existing path was not rejected before generation: %v, %q", err, out.String())
			}
			after, err := os.Lstat(path)
			if err != nil || before.Mode() != after.Mode() || before.Size() != after.Size() {
				t.Fatalf("existing path changed: %v", err)
			}
		})
	}
}

// TestTerrainFixtureDeterministic checks fixed input stability and all three terrain material categories.
func TestTerrainFixtureDeterministic(t *testing.T) {
	g := newTerrainFixture()
	p := world.ChunkPos{0, 0}
	a, b := chunk.New(world.DefaultBlockRegistry, world.Overworld.Range()), chunk.New(world.DefaultBlockRegistry, world.Overworld.Range())
	g.GenerateChunk(p, a)
	g.GenerateChunk(p, b)
	if !reflect.DeepEqual(chunk.Encode(a, chunk.DiskEncoding), chunk.Encode(b, chunk.DiskEncoding)) {
		t.Fatal("identical input changed terrain encoding")
	}
	counts := map[uint32]int{}
	minHeight, maxHeight := 999, -999
	for x := -3; x <= 3; x++ {
		for z := -3; z <= 3; z++ {
			c := chunk.New(world.DefaultBlockRegistry, world.Overworld.Range())
			g.GenerateChunk(world.ChunkPos{int32(x), int32(z)}, c)
			for bx := 0; bx < 16; bx++ {
				for bz := 0; bz < 16; bz++ {
					h := terrainHeight(x*16+bx, z*16+bz)
					minHeight, maxHeight = min(minHeight, h), max(maxHeight, h)
					for y := int16(0); y < 150; y++ {
						counts[c.Block(uint8(bx), y, uint8(bz), 0)]++
					}
				}
			}
		}
	}
	if maxHeight-minHeight < 20 {
		t.Fatal("insufficient terrain relief")
	}
	for _, id := range []uint32{g.water, g.leaves, g.log, g.grass, g.stone} {
		if counts[id] == 0 {
			t.Fatalf("missing render material %d", id)
		}
	}
}

// TestTerrainFixtureClipsToDimensionHeight guards the Nether chunk whose hills and trees exceed Y=127.
func TestTerrainFixtureClipsToDimensionHeight(t *testing.T) {
	newTerrainFixture().GenerateChunk(world.ChunkPos{-64, -61}, chunk.New(world.DefaultBlockRegistry, world.Nether.Range()))
}
