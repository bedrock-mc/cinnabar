package main

import (
	"math"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/biome"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

const terrainSeed uint64 = 0xc1aa8a20261006
const terrainSea = 62

// terrainFixture is a deterministic synthetic renderer workload, not vanilla terrain generation.
type terrainFixture struct{ stone, dirt, grass, bedrock, water, sand, log, leaves uint32 }

// newTerrainFixture resolves immutable block IDs once for parallel chunk generation.
func newTerrainFixture() terrainFixture {
	br := world.DefaultBlockRegistry
	br.Finalize()
	return terrainFixture{br.BlockRuntimeID(block.Stone{}), br.BlockRuntimeID(block.Dirt{}), br.BlockRuntimeID(block.Grass{}), br.BlockRuntimeID(block.Bedrock{}), br.BlockRuntimeID(block.Water{Depth: 8, Still: true}), br.BlockRuntimeID(block.Sand{}), br.BlockRuntimeID(block.Log{}), br.BlockRuntimeID(block.Leaves{Persistent: true})}
}

// terrainHash gives each lattice point a stable seed-dependent value.
func terrainHash(x, z int) uint64 {
	v := uint64(int64(x))*0x9e3779b185ebca87 ^ uint64(int64(z))*0xc2b2ae3d27d4eb4f ^ terrainSeed
	v ^= v >> 30
	v *= 0xbf58476d1ce4e5b9
	v ^= v >> 27
	v *= 0x94d049bb133111eb
	return v ^ (v >> 31)
}

// terrainNoise blends a seeded lattice without seams at chunk boundaries.
func terrainNoise(x, z float64) float64 {
	ix, iz := int(math.Floor(x)), int(math.Floor(z))
	fx, fz := x-float64(ix), z-float64(iz)
	fx = fx * fx * (3 - 2*fx)
	fz = fz * fz * (3 - 2*fz)
	val := func(x, z int) float64 { return float64(terrainHash(x, z)>>11)/float64(uint64(1)<<53)*2 - 1 }
	a, b := val(ix, iz)*(1-fx)+val(ix+1, iz)*fx, val(ix, iz+1)*(1-fx)+val(ix+1, iz+1)*fx
	return a*(1-fz) + b*fz
}

// terrainHeight combines wooded hills, a river valley, and occasional exposed cliffs.
func terrainHeight(x, z int) int {
	xf, zf := float64(x), float64(z)
	valley := math.Abs(zf - 30*math.Sin(xf/95) - 12)
	h := 80 + 34*terrainNoise(xf/105, zf/105) + 13*terrainNoise(xf/31, zf/31) + 4*terrainNoise(xf/10, zf/10)
	h -= 35 * math.Exp(-valley*valley/1000)
	if terrainNoise(xf/80+19, zf/80-27) > 0.38 {
		h += 16
	}
	return int(math.Round(h))
}

// terrainCave leaves connected tunnels and open cliff mouths without random state.
func terrainCave(x, y, z, top int) bool {
	if y < 5 || y > top-3 {
		return false
	}
	xf, yf, zf := float64(x), float64(y), float64(z)
	a := math.Sin(xf/19) + math.Sin(zf/23) + math.Sin(yf/11)
	b := math.Cos(xf/31-zf/29) + math.Sin(yf/8)
	return math.Abs(a) < 0.26 && math.Abs(b) < 0.46
}

// GenerateChunk fills one independent terrain column and clips nearby trees into it.
func (g terrainFixture) GenerateChunk(pos world.ChunkPos, c *chunk.Chunk) {
	minY, maxY := c.Range().Min(), c.Range().Max()
	bi := uint32(biome.Plains{}.EncodeBiome())
	ox, oz := int(pos[0])*16, int(pos[1])*16
	for x := 0; x < 16; x++ {
		for z := 0; z < 16; z++ {
			wx, wz := ox+x, oz+z
			top := terrainHeight(wx, wz)
			for y := minY; y <= min(max(top, terrainSea), maxY); y++ {
				id := g.stone
				switch {
				case y == minY:
					id = g.bedrock
				case y > top:
					id = g.water
				case terrainCave(wx, y, wz, top):
					continue
				case y == top && top <= terrainSea+1:
					id = g.sand
				case y == top:
					id = g.grass
				case y > top-4:
					id = g.dirt
				}
				c.SetBlock(uint8(x), int16(y), uint8(z), 0, id)
			}
			for y := minY; y <= maxY; y++ {
				c.SetBiome(uint8(x), int16(y), uint8(z), bi)
			}
		}
	}
	for gx := int(math.Floor(float64(ox-3) / 11)); gx <= int(math.Floor(float64(ox+18)/11)); gx++ {
		for gz := int(math.Floor(float64(oz-3) / 11)); gz <= int(math.Floor(float64(oz+18)/11)); gz++ {
			h := terrainHash(gx, gz)
			if h%5 == 0 {
				continue
			}
			x, z := gx*11+int((h>>8)%7)+2, gz*11+int((h>>16)%7)+2
			y := terrainHeight(x, z)
			if y <= terrainSea+2 {
				continue
			}
			height := 5 + int((h>>24)%3)
			put := func(wx, wy, wz int, id uint32) {
				if wx >= ox && wx < ox+16 && wz >= oz && wz < oz+16 && wy <= maxY {
					c.SetBlock(uint8(wx-ox), int16(wy), uint8(wz-oz), 0, id)
				}
			}
			for dy := height - 2; dy <= height+1; dy++ {
				for dx := -2; dx <= 2; dx++ {
					for dz := -2; dz <= 2; dz++ {
						if terrainAbs(dx)+terrainAbs(dz) == 4 && dy != height-1 {
							continue
						}
						put(x+dx, y+dy, z+dz, g.leaves)
					}
				}
			}
			for dy := 1; dy <= height; dy++ {
				put(x, y+dy, z, g.log)
			}
		}
	}
}

// DefaultSpawn keeps a new player above the deterministic river overlook.
func (terrainFixture) DefaultSpawn(world.Dimension) cube.Pos {
	return cube.Pos{0, max(terrainHeight(0, 0), terrainSea) + 2, 0}
}

// terrainAbs returns an integer magnitude for the small tree crown mask.
func terrainAbs(v int) int {
	if v < 0 {
		return -v
	}
	return v
}
