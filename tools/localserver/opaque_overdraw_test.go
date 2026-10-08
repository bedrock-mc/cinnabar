package main

import (
	"io"
	"testing"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

func TestOpaqueOverdrawSettingsAreOptInAndRepeatable(t *testing.T) {
	args := []string{"-dir", "fixture", "-addr", "127.0.0.1:0", "-opaque-overdraw"}
	s, err := parseSettings(args, io.Discard)
	if err != nil || !s.opaqueOverdraw {
		t.Fatalf("fixture settings: %+v, %v", s, err)
	}
	uc := s.userConfig()
	if uc.World.SaveData || uc.Players.SaveData {
		t.Fatal("fixture must not reuse saved world or player data")
	}
	var conf server.Config
	s.configureOpaqueOverdraw(&conf)
	if conf.RandomTickSpeed != -1 || conf.Generator == nil {
		t.Fatal("fixture must provide stable geometry without random ticks")
	}
	if _, ok := conf.Generator(world.Overworld).(opaqueOverdrawGenerator); !ok {
		t.Fatal("overworld is not using the fixture generator")
	}
	var ordinary server.Config
	settings{}.configureOpaqueOverdraw(&ordinary)
	if ordinary.Generator != nil || ordinary.RandomTickSpeed != 0 {
		t.Fatal("ordinary world configuration changed")
	}
}

func TestOpaqueOverdrawTakesPrecedenceOverNormalGeneration(t *testing.T) {
	s, err := parseSettings([]string{
		"-dir", "fixture", "-addr", "127.0.0.1:0", "-generator", "normal", "-opaque-overdraw",
	}, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	var conf server.Config
	generators, err := s.configureGenerators(&conf)
	if err != nil {
		t.Fatal(err)
	}
	defer generators.close()
	if len(generators) != 0 || conf.Generator != nil {
		t.Fatal("opaque-overdraw fixture initialized normal generators or their spawn source")
	}
	s.configureOpaqueOverdraw(&conf)
	g, ok := conf.Generator(world.Overworld).(opaqueOverdrawGenerator)
	if !ok {
		t.Fatal("opaque-overdraw fixture lost precedence in the overworld")
	}
	for _, dim := range []world.Dimension{world.Nether, world.End} {
		if _, ok := conf.Generator(dim).(world.NopGenerator); !ok {
			t.Fatalf("opaque-overdraw fixture lost precedence in %v", dim)
		}
	}
	w := world.Config{Synchronous: true, Provider: world.NopProvider{}, Generator: g}.New()
	defer w.Close()
	s.applyTo(w)
	if w.Spawn() != g.DefaultSpawn(world.Overworld) {
		t.Fatalf("spawn = %v, want fixture spawn", w.Spawn())
	}
}

func TestOpaqueOverdrawIncludesFoliageAndSealedCave(t *testing.T) {
	counts := map[string]int{}
	for x := -32; x < 0; x++ {
		for z := -16; z < 16; z++ {
			b := opaqueOverdrawBlock(x, opaqueOverdrawGround+1, z, world.Overworld.Range().Min())
			name, _ := b.EncodeBlock()
			counts[name]++
			if lower, ok := b.(block.DoubleTallGrass); ok {
				upper, ok := opaqueOverdrawBlock(x, opaqueOverdrawGround+2, z, world.Overworld.Range().Min()).(block.DoubleTallGrass)
				if !ok || !upper.UpperPart || lower.UpperPart || upper.Type != lower.Type {
					t.Fatal("tall grass is missing its matching upper half")
				}
			}
		}
	}
	for _, b := range []world.Block{block.ShortGrass{}, block.DoubleTallGrass{}, block.Flower{Type: block.Poppy()}, block.Flower{Type: block.Dandelion()}, block.Leaves{Type: block.OakLeaves(), Persistent: true}} {
		name, _ := b.EncodeBlock()
		if counts[name] == 0 {
			t.Errorf("surface is missing %s", name)
		}
	}
	for _, p := range [][3]int{{-32, 48, 1}, {0, 48, 0}, {-16, 48, -16}, {-16, 48, 16}, {-16, 44, 0}, {-16, 53, 0}} {
		if _, ok := opaqueOverdrawBlock(p[0], p[1], p[2], world.Overworld.Range().Min()).(block.Stone); !ok {
			t.Errorf("cave is not sealed by stone at %v", p)
		}
	}
	if b := opaqueOverdrawBlock(-16, 47, 0, world.Overworld.Range().Min()); b != nil {
		t.Fatalf("cave camera is obstructed by %T", b)
	}
	leaves := 0
	for y := opaqueOverdrawGround + 6; y <= opaqueOverdrawGround+10; y++ {
		if b, ok := opaqueOverdrawBlock(33, y, 0, world.Overworld.Range().Min()).(block.Leaves); ok && b.Persistent {
			leaves++
		}
	}
	if leaves < 4 {
		t.Fatalf("canopy has %d leaf layers, want at least four", leaves)
	}
}

func TestOpaqueOverdrawGeneratedChunksRemainIdentical(t *testing.T) {
	world.DefaultBlockRegistry.Finalize()
	g := opaqueOverdrawGenerator{blocks: world.DefaultBlockRegistry}
	for _, pos := range []world.ChunkPos{{-2, 0}, {2, -1}} {
		a := chunk.New(world.DefaultBlockRegistry, world.Overworld.Range())
		b := chunk.New(world.DefaultBlockRegistry, world.Overworld.Range())
		g.GenerateChunk(pos, a)
		g.GenerateChunk(pos, b)
		for x := range uint8(16) {
			for z := range uint8(16) {
				for y := int16(a.Range().Min()); y <= int16(a.Range().Max()); y++ {
					if a.Block(x, y, z, 0) != b.Block(x, y, z, 0) {
						t.Fatalf("different generated blocks in chunk %v at %d,%d,%d", pos, x, y, z)
					}
					wantBiome := opaqueOverdrawBiome(int(pos[0])*16+int(x), int(pos[1])*16+int(z))
					if a.Biome(x, y, z) != uint32(wantBiome.EncodeBiome()) {
						t.Fatalf("unexpected biome in chunk %v at %d,%d,%d", pos, x, y, z)
					}
				}
			}
		}
	}
}

func TestOpaqueOverdrawHasMixedAndUniformBiomeRegions(t *testing.T) {
	mixed := map[int]bool{}
	uniform := map[int]bool{}
	for x := -32; x < 0; x++ {
		for z := -16; z < 16; z++ {
			mixed[opaqueOverdrawBiome(x, z).EncodeBiome()] = true
			uniform[opaqueOverdrawBiome(x+64, z).EncodeBiome()] = true
		}
	}
	if len(mixed) != 3 || len(uniform) != 1 {
		t.Fatalf("biome counts: mixed=%d, uniform=%d", len(mixed), len(uniform))
	}
}

func TestOpaqueOverdrawStopsTime(t *testing.T) {
	g := opaqueOverdrawGenerator{blocks: world.DefaultBlockRegistry}
	w := world.Config{Synchronous: true, Provider: world.NopProvider{}, Generator: g}.New()
	defer w.Close()
	freezeOpaqueOverdrawWorld(w)
	if w.Spawn() != g.DefaultSpawn(world.Overworld) {
		t.Fatalf("spawn = %v, want fixture spawn", w.Spawn())
	}
	w.Do(func(tx *world.Tx) {
		if _, ok := tx.Block(w.Spawn().Side(cube.FaceDown)).(block.Grass); !ok {
			t.Fatal("spawn is not above the fixture grass surface")
		}
	})
	before := w.Time()
	w.AdvanceTick()
	if w.TimeCycle() || w.Time() != before {
		t.Fatal("fixture time advanced between captures")
	}
}
