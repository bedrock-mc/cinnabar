package localserver

import (
	"errors"
	"fmt"
	"io"
	"sync"
	"sync/atomic"
	"time"

	vanilla "github.com/bedrock-mc/vanilla-gen"
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

type worldGenerators map[world.Dimension]world.Generator

// pendingGenerators are registered generators built in the background once begun, so their
// construction overlaps the rest of server setup.
type pendingGenerators struct {
	// build carries begin's true or discard's false, whichever comes first.
	build      chan bool
	signalled  sync.Once
	done       chan struct{}
	generators worldGenerators
	err        error
	taken      bool
}

// startGenerators readies the selected factory to build after block registration.
// Synthetic fixtures supply their own generators.
func (s settings) startGenerators() *pendingGenerators {
	p := &pendingGenerators{build: make(chan bool, 1), done: make(chan struct{}), generators: make(worldGenerators)}
	factory, ok := lookupGenerator(s.generator)
	if !ok {
		p.err = fmt.Errorf("unknown generator %q", s.generator)
		close(p.done)
		return p
	}
	if s.terrainFixture || s.opaqueOverdraw {
		close(p.done)
		return p
	}
	go func() {
		defer close(p.done)
		if <-p.build {
			p.generators, p.err = factory(s.seed)
			if p.err == nil && len(p.generators) > 0 {
				for _, dim := range []world.Dimension{world.Overworld, world.Nether, world.End} {
					if p.generators[dim] == nil {
						p.err = fmt.Errorf("generator %q is missing dimension %v", s.generator, dim)
						break
					}
				}
			}
			if p.err != nil {
				p.err = errors.Join(p.err, p.generators.close())
				p.generators = nil
			}
		}
	}()
	return p
}

// begin starts construction. Building finalises the default block registry, so every block,
// including Experience blocks, must be registered first.
func (p *pendingGenerators) begin() {
	p.signal(true)
}

func (p *pendingGenerators) signal(build bool) {
	p.signalled.Do(func() { p.build <- build })
}

// buildGenerators constructs the three dimensions' generators concurrently; on failure none stay open.
func buildGenerators(seed int64) (worldGenerators, error) {
	dims := []world.Dimension{world.Overworld, world.Nether, world.End}
	built := make([]vanilla.Generator, len(dims))
	errs := make([]error, len(dims))
	var wg sync.WaitGroup
	for i, dim := range dims {
		wg.Go(func() {
			built[i], errs[i] = vanilla.NewForDimensionWithConfig(seed, dim, vanilla.GeneratorConfig{
				Acceleration: vanilla.AccelerationConfig{Mode: vanilla.AccelerationAuto},
			})
		})
	}
	wg.Wait()
	generators := make(worldGenerators)
	for i, dim := range dims {
		if errs[i] == nil {
			generators[dim] = built[i]
		}
	}
	if err := errors.Join(errs...); err != nil {
		return nil, errors.Join(fmt.Errorf("configure world generator: %w", err), generators.close())
	}
	return generators, nil
}

// warmGeneration builds and discards a generator for every dimension, so a standby server has the
// seed-independent structure and biome data ready before it learns its world.
func warmGeneration() {
	if generators, err := buildGenerators(0); err == nil {
		_ = generators.close()
	}
}

// configure waits for the generators and installs them in conf; the caller then closes them.
func (p *pendingGenerators) configure(conf *server.Config) (worldGenerators, error) {
	p.begin()
	<-p.done
	p.taken = true
	if p.err != nil {
		return nil, p.err
	}
	if len(p.generators) > 0 {
		generators := p.generators
		conf.Generator = func(dim world.Dimension) world.Generator { return generators[dim] }
	}
	return p.generators, nil
}

// discard closes generators that startup abandoned before configuring them; unbegun ones are
// never built.
func (p *pendingGenerators) discard() {
	if p.taken {
		return
	}
	p.signal(false)
	<-p.done
	p.taken = true
	_ = p.generators.close()
}

func (s settings) configureGenerators(conf *server.Config) (worldGenerators, error) {
	return s.startGenerators().configure(conf)
}

func (generators worldGenerators) close() error {
	var err error
	for _, generator := range generators {
		if closer, ok := generator.(io.Closer); ok {
			err = errors.Join(err, closer.Close())
		}
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

// ConcurrentChunkGeneration preserves the selected generator's concurrency contract.
func (g *generationMeter) ConcurrentChunkGeneration() bool {
	parallel, ok := g.Generator.(interface{ ConcurrentChunkGeneration() bool })
	return ok && parallel.ConcurrentChunkGeneration()
}

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

// accelerationStatus describes generators that expose optional acceleration diagnostics.
func accelerationStatus(generator world.Generator) any {
	if accelerated, ok := generator.(interface {
		AccelerationStatus() vanilla.AccelerationStatus
	}); ok {
		return accelerated.AccelerationStatus()
	}
	return "unavailable"
}
