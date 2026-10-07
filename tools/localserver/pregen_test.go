package main

import (
	"bytes"
	"context"
	"errors"
	"io"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
	"github.com/df-mc/dragonfly/server/world/mcdb"
)

// pregenTestGenerator provides cheap deterministic columns for persistence tests.
type pregenTestGenerator struct{}

// GenerateChunk marks every generated column without involving the expensive terrain pipeline.
func (pregenTestGenerator) GenerateChunk(_ world.ChunkPos, c *chunk.Chunk) {
	c.SetBlock(0, 10, 0, 0, world.BlockRuntimeID(block.Stone{}))
}

// DefaultSpawn keeps fixture coordinates independent from the caller's saved spawn.
func (pregenTestGenerator) DefaultSpawn(world.Dimension) cube.Pos { return cube.Pos{} }

func TestPregenerationPersistsSquareAroundSavedNegativeSpawn(t *testing.T) {
	db, err := mcdb.Open(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	defer db.Close()
	pos := world.ChunkPos{-2, -3}
	c := chunk.New(world.DefaultBlockRegistry, world.Overworld.Range())
	c.SetBlock(0, 10, 0, 0, world.BlockRuntimeID(block.Gold{}))
	if err := db.StoreColumn(pos, world.Overworld, &chunk.Column{Chunk: c}); err != nil {
		t.Fatal(err)
	}
	var out bytes.Buffer
	if err := pregenerate(context.Background(), db, pregenTestGenerator{}, cube.Pos{-17, 80, -33}, 1, 3, &out); err != nil {
		t.Fatal(err)
	}
	for x := -3; x <= -1; x++ {
		for z := -4; z <= -2; z++ {
			col, err := db.LoadColumn(world.ChunkPos{int32(x), int32(z)}, world.Overworld)
			if err != nil {
				t.Fatal(err)
			}
			want := world.BlockRuntimeID(block.Stone{})
			if x == -2 && z == -3 {
				want = world.BlockRuntimeID(block.Gold{})
			}
			if got := col.Chunk.Block(0, 10, 0, 0); got != want {
				t.Fatalf("column %d,%d overwritten or missing: %d", x, z, got)
			}
		}
	}
	if !strings.Contains(out.String(), "chunks=9 generated=8 existing=1") {
		t.Fatalf("progress = %s", &out)
	}
	out.Reset()
	if err := pregenerate(context.Background(), db, pregenTestGenerator{}, cube.Pos{-17, 80, -33}, 1, 3, &out); err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out.String(), "generated=0 existing=9") {
		t.Fatalf("reopen = %s", &out)
	}
}

// failingPregenProvider forces a well-formed storage error at the persistence boundary.
type failingPregenProvider struct{ world.NopProvider }

// LoadColumn makes pregeneration fail instead of replacing an unreadable saved column.
func (failingPregenProvider) LoadColumn(world.ChunkPos, world.Dimension) (*chunk.Column, error) {
	return nil, errors.New("read failed")
}

func TestPregenerationStopsOnReadFailureAndCancellation(t *testing.T) {
	if err := pregenerate(context.Background(), failingPregenProvider{}, pregenTestGenerator{}, cube.Pos{}, 1, 2, io.Discard); err == nil || !strings.Contains(err.Error(), "read failed") {
		t.Fatalf("read error=%v", err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := pregenerate(ctx, world.NopProvider{}, pregenTestGenerator{}, cube.Pos{}, 1, 2, io.Discard); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancel=%v", err)
	}
}

func TestPregenFlagsRejectUnsupportedAndUnboundedWork(t *testing.T) {
	for _, extra := range [][]string{{"-pregen-radius", "-1"}, {"-pregen-radius", "513"}, {"-chunk-workers", "0"}, {"-chunk-workers", "17"}, {"-pregen-radius", "1"}, {"-pregen-radius", "1", "-generator", "normal", "-terrain-fixture"}} {
		args := append([]string{"-dir", "d", "-addr", "127.0.0.1:0"}, extra...)
		if _, err := parseSettings(args, io.Discard); err == nil {
			t.Fatalf("accepted %v", extra)
		}
	}
}

func TestNormalPregenerationCompletesBeforeReadyAndReusesSavedColumns(t *testing.T) {
	args := []string{"-dir", t.TempDir(), "-addr", "127.0.0.1:0", "-generator", "normal", "-seed", "-7", "-pregen-radius", "1"}
	for _, counts := range []string{"chunks=9 generated=9 existing=0", "chunks=9 generated=0 existing=9"} {
		var out bytes.Buffer
		if err := run(args, strings.NewReader("stop\n"), &out, io.Discard); err != nil {
			t.Fatal(err)
		}
		text := out.String()
		complete, ready := strings.Index(text, "pregen complete"), strings.Index(text, "ready\n")
		if complete < 0 || ready < complete || !strings.Contains(text, counts) {
			t.Fatalf("startup did not save chunks before readiness: %s", text)
		}
	}
}
