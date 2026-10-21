package localserver

import (
	"bytes"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world/chunk"
	"github.com/df-mc/dragonfly/server/world/mcdb"
	"io"
	"path/filepath"
	"testing"
	"testing/synctest"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
)

func TestGeneratorAndSeedFlagsReachEveryDimension(t *testing.T) {
	settings, err := parseSettings([]string{"-dir", "d", "-addr", "127.0.0.1:0", "-generator", "normal", "-seed", "-7"}, io.Discard)
	if err != nil || settings.generator != "normal" || settings.seed != -7 {
		t.Fatalf("settings = %+v, %v", settings, err)
	}
	var conf server.Config
	generators, err := settings.configureGenerators(&conf)
	if err != nil {
		t.Fatal(err)
	}
	defer generators.close()
	for _, dim := range []world.Dimension{world.Overworld, world.Nether, world.End} {
		if conf.Generator(dim) == nil {
			t.Fatalf("missing generator for %v", dim)
		}
	}
}

func TestFlatKeepsTheServerFlatGenerators(t *testing.T) {
	var conf server.Config
	generators, err := (settings{generator: "flat", seed: -7}).configureGenerators(&conf)
	if err != nil || len(generators) != 0 || conf.Generator != nil {
		t.Fatalf("flat config changed: %+v, %v", conf, err)
	}
}

func TestNormalWorldGeneratesTerrainBeyondTheFlatLayers(t *testing.T) {
	var conf server.Config
	generators, err := (settings{generator: "normal", seed: -7}).configureGenerators(&conf)
	if err != nil {
		t.Fatal(err)
	}
	defer generators.close()
	terrain := chunk.New(world.DefaultBlockRegistry, cube.Range{-64, 319})
	conf.Generator(world.Overworld).GenerateChunk(world.ChunkPos{0, 0}, terrain)
	air := world.BlockRuntimeID(block.Air{})
	if terrain.Block(0, 0, 0, 0) == air {
		t.Fatal("normal generator left the surface world empty above the flat layers")
	}
}

func TestNormalWorldStartupKeepsSavedSpawnOnReopen(t *testing.T) {
	dir := t.TempDir()
	args := []string{"-dir", dir, "-addr", "127.0.0.1:0", "-generator", "normal", "-seed", "-7"}
	var stdout bytes.Buffer
	if err := Run(args, bytes.NewBufferString("stop\n"), &stdout, io.Discard); err != nil {
		t.Fatal(err)
	}
	if stdout.String() != "ready\n" {
		t.Fatalf("startup = %q", stdout.String())
	}
	dbPath := filepath.Join(dir, "db")
	db, err := mcdb.Open(dbPath)
	if err != nil {
		t.Fatal(err)
	}
	saved := db.Settings()
	saved.Spawn = cube.Pos{123, 80, -456}
	db.SaveSettings(saved)
	if err := db.Close(); err != nil {
		t.Fatal(err)
	}
	stdout.Reset()
	if err := Run(args, bytes.NewBufferString("stop\n"), &stdout, io.Discard); err != nil {
		t.Fatal(err)
	}
	db, err = mcdb.Open(dbPath)
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if db.Settings().Spawn != saved.Spawn {
		t.Fatalf("saved spawn reset to %v", db.Settings().Spawn)
	}
}

// A new normal world keeps the generator's spawn without first generating a column at the origin
// to search for one.
func TestNewNormalWorldSavesTheGeneratorSpawnWithoutSearching(t *testing.T) {
	dir := t.TempDir()
	args := []string{"-dir", dir, "-addr", "127.0.0.1:0", "-generator", "normal", "-seed", "-7"}
	if err := Run(args, bytes.NewBufferString("stop\n"), io.Discard, io.Discard); err != nil {
		t.Fatal(err)
	}
	generators, err := (settings{generator: "normal", seed: -7}).configureGenerators(&server.Config{})
	if err != nil {
		t.Fatal(err)
	}
	defer generators.close()
	want := generators[world.Overworld].DefaultSpawn(world.Overworld)
	db, err := mcdb.Open(filepath.Join(dir, "db"))
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	if got := db.Settings().Spawn; got != want {
		t.Fatalf("spawn = %v, want the generator's %v", got, want)
	}
	spawnChunk := world.ChunkPos{int32(want[0] >> 4), int32(want[2] >> 4)}
	if max(abs(spawnChunk[0]), abs(spawnChunk[1])) <= spawnWarmRadius+1 {
		t.Skipf("spawn chunk %v is too near the origin to tell a search from the warm-up", spawnChunk)
	}
	if _, err := db.LoadColumn(world.ChunkPos{}, world.Overworld); err == nil {
		t.Fatal("startup generated the origin column to search for a spawn")
	}
}

func abs(v int32) int32 {
	if v < 0 {
		return -v
	}
	return v
}

// Generators wait to be begun, so Experience blocks register before construction finalises the
// block registry, and abandoned ones never build.
func TestGeneratorsBuildOnlyOnceBegun(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		pending := (settings{generator: "normal", seed: -7}).startGenerators()
		synctest.Wait()
		select {
		case <-pending.done:
			t.Fatal("generators were built before blocks finished registering")
		default:
		}
		pending.discard()
		if len(pending.generators) != 0 {
			t.Fatal("discarded generators were built")
		}
	})
}
