package main

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
	"github.com/df-mc/dragonfly/server/world/mcdb"
)

const (
	terrainDefaultRadius = 24
	terrainMinRadius     = 12
	terrainMaxRadius     = 64
	terrainTime          = 6000
)

// configureTerrainFixture leaves Dragonfly's default generators and work limits untouched unless opted in.
func (s settings) configureTerrainFixture(conf *server.Config) {
	if !s.terrainFixture {
		return
	}
	conf.Generator = func(world.Dimension) world.Generator { return newTerrainFixture() }
	conf.RandomTickSpeed = -1
	conf.ChunkLoadWorkers = 4
}

// freezeTerrainFixture prevents weather and daylight changes between matched captures.
func freezeTerrainFixture(w *world.World) {
	w.StopWeatherCycle()
	w.StopThundering()
	w.StopRaining()
	w.StopTime()
	w.SetTime(terrainTime)
}

// generateTerrainFixture creates a new database before client timing; an existing path is never opened.
func generateTerrainFixture(s settings, out io.Writer) (err error) {
	if s.terrainFixtureRadius < terrainMinRadius || s.terrainFixtureRadius > terrainMaxRadius {
		return fmt.Errorf("terrain fixture radius must be %d..%d", terrainMinRadius, terrainMaxRadius)
	}
	if err := os.MkdirAll(s.dir, 0700); err != nil {
		return fmt.Errorf("create fixture directory: %w", err)
	}
	dbPath := filepath.Join(s.dir, "db")
	if err := os.Mkdir(dbPath, 0700); err != nil {
		return fmt.Errorf("create new fixture database (existing paths are refused): %w", err)
	}
	g := newTerrainFixture()
	db, err := mcdb.Open(dbPath)
	if err != nil {
		return err
	}
	defer func() { err = errors.Join(err, db.Close()) }()
	set := db.Settings()
	set.Name = "Synthetic non-flat frame workload"
	set.Spawn = g.DefaultSpawn(world.Overworld)
	set.Time = terrainTime
	set.TimeCycle, set.WeatherCycle = false, false
	set.Raining, set.Thundering = false, false
	set.DefaultGameMode = world.GameModeCreative
	set.Difficulty = world.DifficultyPeaceful
	set.TickRange = 0
	db.SaveSettings(set)
	count := 0
	for x := -s.terrainFixtureRadius; x <= s.terrainFixtureRadius; x++ {
		for z := -s.terrainFixtureRadius; z <= s.terrainFixtureRadius; z++ {
			c := chunk.New(world.DefaultBlockRegistry, world.Overworld.Range())
			p := world.ChunkPos{int32(x), int32(z)}
			g.GenerateChunk(p, c)
			if err := db.StoreColumn(p, world.Overworld, &chunk.Column{Chunk: c}); err != nil {
				return err
			}
			count++
		}
		fmt.Fprintf(out, "generated row %d/%d\n", x+s.terrainFixtureRadius+1, 2*s.terrainFixtureRadius+1)
	}
	manifest := map[string]any{
		"kind": "synthetic_non_vanilla", "seed": terrainSeed, "generator_version": 1,
		"radius_chunks": s.terrainFixtureRadius, "chunks": count,
		"min_block": -16 * s.terrainFixtureRadius, "max_block": 16*s.terrainFixtureRadius + 15,
		"sea_level": terrainSea, "time": terrainTime, "spawn": set.Spawn,
		"categories":   []string{"opaque stone/grass/dirt/cliffs/caves", "cutout persistent oak leaves", "translucent source water"},
		"camera_start": []float64{48, 94, -75}, "camera_yaw": 5.4, "camera_pitch": 20,
		"flight_route": [][]float64{{48, 130, -75}, {228, 130, -75}},
		"notes":        "Synthetic workload, not vanilla terrain generation. No persistent actor or particle workload. Copy this directory for each run.",
	}
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(s.dir, "fixture-manifest.json"), append(data, '\n'), 0600); err != nil {
		return err
	}
	fmt.Fprintf(out, "generated %d chunks, synthetic seed %d\n", count, terrainSeed)
	return nil
}
