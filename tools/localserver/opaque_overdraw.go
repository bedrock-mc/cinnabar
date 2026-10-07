package main

import (
	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/biome"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

const opaqueOverdrawGround = 63

// configureOpaqueOverdraw selects coordinate-defined fixture geometry and disables random ticks.
func (s settings) configureOpaqueOverdraw(conf *server.Config) {
	if !s.opaqueOverdraw {
		return
	}
	conf.RandomTickSpeed = -1
	blocks := conf.Blocks
	if blocks == nil {
		blocks = world.DefaultBlockRegistry
	}
	conf.Generator = func(dim world.Dimension) world.Generator {
		if dim != world.Overworld {
			return world.NopGenerator{}
		}
		return opaqueOverdrawGenerator{blocks: blocks}
	}
}

// freezeOpaqueOverdrawWorld pins the spawn, illumination and weather across repeated captures.
func freezeOpaqueOverdrawWorld(w *world.World) {
	if w.Dimension() == world.Overworld {
		w.SetSpawn(opaqueOverdrawGenerator{}.DefaultSpawn(w.Dimension()))
	}
	w.SetTime(6000)
	w.StopTime()
	w.StopRaining()
	w.StopThundering()
	w.StopWeatherCycle()
}

// opaqueOverdrawGenerator puts dense foliage and a nearby enclosed cave within one view-distance-five region.
type opaqueOverdrawGenerator struct{ blocks world.BlockRegistry }

// DefaultSpawn starts on the foliage surface west of the forest canopy.
func (opaqueOverdrawGenerator) DefaultSpawn(world.Dimension) cube.Pos {
	return cube.Pos{-16, opaqueOverdrawGround + 1, 0}
}

// GenerateChunk uses coordinate-only geometry so chunk scheduling cannot change the benchmark scene.
func (g opaqueOverdrawGenerator) GenerateChunk(pos world.ChunkPos, c *chunk.Chunk) {
	for x := range uint8(16) {
		for z := range uint8(16) {
			wx, wz := int(pos[0])*16+int(x), int(pos[1])*16+int(z)
			for y := c.Range().Min(); y <= opaqueOverdrawGround+12; y++ {
				b := opaqueOverdrawBlock(wx, y, wz, c.Range().Min())
				if b != nil {
					c.SetBlock(x, int16(y), z, 0, g.blocks.BlockRuntimeID(b))
				}
			}
			for y := c.Range().Min(); y <= c.Range().Max(); y++ {
				c.SetBiome(x, int16(y), z, uint32(opaqueOverdrawBiome(wx, wz).EncodeBiome()))
			}
		}
	}
}

// opaqueOverdrawBiome blends the western half's tints and keeps a uniform plains control to the east.
func opaqueOverdrawBiome(x, z int) world.Biome {
	if x < 0 {
		switch ((x >> 3) + (z >> 3)) % 3 {
		case 1, -1:
			return biome.Swamp{}
		case 2, -2:
			return biome.Forest{}
		}
	}
	return biome.Plains{}
}

// opaqueOverdrawBlock keeps the surface, overlapping leaf crowns and enclosed stone control deterministic.
func opaqueOverdrawBlock(x, y, z, bottom int) world.Block {
	if y == bottom {
		return block.Bedrock{}
	}
	if y <= opaqueOverdrawGround {
		if x > -32 && x < 0 && z > -16 && z < 16 && y > 44 && y < 53 {
			return nil
		}
		if x == -32 && y == 48 && z == 0 {
			return block.Glowstone{}
		}
		if y == opaqueOverdrawGround {
			return block.Grass{}
		}
		if y >= opaqueOverdrawGround-2 {
			return block.Dirt{}
		}
		return block.Stone{}
	}
	if x < -64 || x >= 64 || z < -64 || z >= 64 {
		return nil
	}
	if x >= 16 {
		if x%8 == 4 && (z+64)%8 == 4 && y <= opaqueOverdrawGround+8 {
			return block.Log{Wood: block.OakWood(), Axis: cube.Y}
		}
		if y >= opaqueOverdrawGround+6 && y <= opaqueOverdrawGround+10 {
			if (x+3*z+y)%7 != 0 {
				return block.Leaves{Type: block.OakLeaves(), Persistent: true}
			}
		}
	}
	if y == opaqueOverdrawGround+1 {
		switch ((x+64)*3 + z + 64) % 8 {
		case 0:
			return block.Flower{Type: block.Poppy()}
		case 1:
			return block.Flower{Type: block.Dandelion()}
		case 2, 3, 4:
			return block.DoubleTallGrass{Type: block.NormalDoubleTallGrass()}
		case 5:
			return block.Leaves{Type: block.OakLeaves(), Persistent: true}
		default:
			return block.ShortGrass{}
		}
	}
	if y == opaqueOverdrawGround+2 {
		kind := ((x+64)*3 + z + 64) % 8
		if kind >= 2 && kind <= 4 {
			return block.DoubleTallGrass{Type: block.NormalDoubleTallGrass(), UpperPart: true}
		}
	}
	return nil
}
