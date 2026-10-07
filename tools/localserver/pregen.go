package main

import (
	"context"
	"errors"
	"fmt"
	"io"
	"sync"
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
	"github.com/df-mc/goleveldb/leveldb"
)

// pregenResult records one saved or existing column and any storage failure.
type pregenResult struct {
	generated bool
	err       error
}

// pregenerate saves missing overworld columns before players can join, leaving existing data intact.
func pregenerate(ctx context.Context, db world.Provider, g world.Generator, spawn cube.Pos, radius, workers int, out io.Writer) error {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	positions := make(chan world.ChunkPos)
	results := make(chan pregenResult, workers)
	var ioLock sync.Mutex
	var running sync.WaitGroup
	for range workers {
		running.Go(func() {
			for pos := range positions {
				if ctx.Err() != nil {
					return
				}
				result := pregenColumn(db, g, pos, &ioLock)
				select {
				case results <- result:
				case <-ctx.Done():
					return
				}
			}
		})
	}
	go func() {
		defer close(positions)
		cx, cz := int32(spawn[0]>>4), int32(spawn[2]>>4)
		for x := -radius; x <= radius; x++ {
			for z := -radius; z <= radius; z++ {
				select {
				case positions <- world.ChunkPos{cx + int32(x), cz + int32(z)}:
				case <-ctx.Done():
					return
				}
			}
		}
	}()
	go func() { running.Wait(); close(results) }()
	start := time.Now()
	total, done, generated := (2*radius+1)*(2*radius+1), 0, 0
	fmt.Fprintf(out, "pregen start total=%d radius=%d workers=%d\n", total, radius, workers)
	var failure error
	for result := range results {
		if result.err != nil {
			failure = errors.Join(failure, result.err)
			cancel()
			continue
		}
		done++
		if result.generated {
			generated++
		}
		if done%32 == 0 || done == total {
			fmt.Fprintf(out, "pregen progress %d/%d generated=%d existing=%d\n", done, total, generated, done-generated)
		}
	}
	if failure != nil {
		return failure
	}
	if err := ctx.Err(); err != nil {
		return fmt.Errorf("pregenerate world: %w", err)
	}
	elapsed := time.Since(start)
	fmt.Fprintf(out, "pregen complete chunks=%d generated=%d existing=%d elapsed_ms=%.3f generated_chunks_per_second=%.3f\n", done, generated, done-generated, float64(elapsed)/float64(time.Millisecond), float64(generated)/elapsed.Seconds())
	return nil
}

// pregenColumn serialises provider access while allowing independent generation work to overlap.
func pregenColumn(db world.Provider, g world.Generator, pos world.ChunkPos, ioLock *sync.Mutex) pregenResult {
	ioLock.Lock()
	_, err := db.LoadColumn(pos, world.Overworld)
	ioLock.Unlock()
	if err == nil {
		return pregenResult{}
	}
	if !errors.Is(err, leveldb.ErrNotFound) {
		return pregenResult{err: fmt.Errorf("load pregeneration column %v: %w", pos, err)}
	}
	c := chunk.New(world.DefaultBlockRegistry, world.Overworld.Range())
	g.GenerateChunk(pos, c)
	ioLock.Lock()
	err = db.StoreColumn(pos, world.Overworld, &chunk.Column{Chunk: c})
	ioLock.Unlock()
	if err != nil {
		return pregenResult{err: fmt.Errorf("save pregeneration column %v: %w", pos, err)}
	}
	return pregenResult{generated: true}
}
