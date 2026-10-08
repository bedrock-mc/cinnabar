package main

import (
	"errors"
	"fmt"
	"io"
	"sync/atomic"
	"time"

	vanilla "github.com/bedrock-mc/vanilla-gen"
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

type worldGenerators map[world.Dimension]vanilla.Generator

func (s settings) configureGenerators(conf *server.Config) (worldGenerators, error) {
	generators := make(worldGenerators)
	fixture := s.terrainFixture || s.opaqueOverdraw
	if s.generator != "normal" || fixture {
		return generators, nil
	}
	for _, dim := range []world.Dimension{world.Overworld, world.Nether, world.End} {
		generator, err := vanilla.NewForDimensionWithConfig(s.seed, dim, vanilla.GeneratorConfig{
			Acceleration: vanilla.AccelerationConfig{Mode: vanilla.AccelerationAuto},
		})
		if err != nil {
			return nil, errors.Join(fmt.Errorf("configure world generator: %w", err), generators.close())
		}
		generators[dim] = generator
	}
	conf.Generator = func(dim world.Dimension) world.Generator { return generators[dim] }
	return generators, nil
}

func (generators worldGenerators) close() error {
	var err error
	for _, generator := range generators {
		err = errors.Join(err, generator.Close())
	}
	return err
}

// generationMeter counts generation work without sampling process metadata.
type generationMeter struct {
	world.Generator
	count, nanos atomic.Uint64
}

// GenerateChunk records only generation elapsed time, excluding storage, lighting and networking.
func (g *generationMeter) GenerateChunk(pos world.ChunkPos, c *chunk.Chunk) {
	start := time.Now()
	g.Generator.GenerateChunk(pos, c)
	g.nanos.Add(uint64(time.Since(start)))
	g.count.Add(1)
}

// ConcurrentChunkGeneration preserves the normal generator's parallel generation contract.
func (g *generationMeter) ConcurrentChunkGeneration() bool { return true }

// meterGeneration reports work per dimension after the server has drained its generation workers.
func meterGeneration(conf *server.Config, out io.Writer) func() {
	generate := conf.Generator
	meters := map[world.Dimension]*generationMeter{}
	for _, dim := range []world.Dimension{world.Overworld, world.Nether, world.End} {
		meters[dim] = &generationMeter{Generator: generate(dim)}
	}
	conf.Generator = func(dim world.Dimension) world.Generator { return meters[dim] }
	start := time.Now()
	return func() {
		elapsed := time.Since(start).Seconds()
		for _, dim := range []world.Dimension{world.Overworld, world.Nether, world.End} {
			g := meters[dim]
			fmt.Fprintf(out, "generation dimension=%v chunks=%d elapsed_seconds=%.3f work_ms=%.3f chunks_per_second=%.3f\n", dim, g.count.Load(), elapsed, float64(g.nanos.Load())/float64(time.Millisecond), float64(g.count.Load())/elapsed)
		}
	}
}
