package main

import (
	"encoding/json"
	"fmt"
	"os"
	"path"
	"path/filepath"
	"slices"
	"strings"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/df-mc/dragonfly/server/world/biome"
	"github.com/df-mc/dragonfly/server/world/chunk"
)

const (
	galleryGround  = 63
	galleryColumns = 16
)

// galleryState identifies one exact palette entry and its four independent samples.
type galleryState struct {
	RuntimeID  uint32          `json:"runtime_id"`
	Name       string          `json:"name"`
	Properties map[string]any  `json:"properties"`
	State      string          `json:"state"`
	Chunk      world.ChunkPos  `json:"chunk"`
	Samples    []gallerySample `json:"samples"`
	Views      []galleryView   `json:"views"`
}

type gallerySample struct {
	Scenario string   `json:"scenario"`
	Position cube.Pos `json:"position"`
}

type galleryView struct {
	Eye   [3]float64 `json:"eye"`
	Yaw   float64    `json:"yaw"`
	Pitch float64    `json:"pitch"`
}

type blockGallery struct {
	blocks  world.BlockRegistry
	states  []galleryState
	byChunk map[world.ChunkPos]int
	stone   uint32
}

// newBlockGallery selects exact registered states without constructing gameplay blocks.
// Filters are comma-separated identifier globs, for example *stairs or *glass*.
func newBlockGallery(blocks world.BlockRegistry, filter string) (*blockGallery, error) {
	blocks.Finalize()
	patterns := strings.Split(filter, ",")
	for i, pattern := range patterns {
		pattern = strings.TrimSpace(pattern)
		if pattern == "" {
			pattern = "*"
		}
		if !strings.Contains(pattern, ":") {
			pattern = "minecraft:" + pattern
		}
		if _, err := path.Match(pattern, ""); err != nil {
			return nil, fmt.Errorf("block gallery filter: %w", err)
		}
		patterns[i] = pattern
	}
	g := &blockGallery{blocks: blocks, byChunk: make(map[world.ChunkPos]int), stone: blocks.BlockRuntimeID(block.Stone{})}
	for id := 0; id < blocks.BlockCount(); id++ {
		name, properties, ok := blocks.RuntimeIDToState(uint32(id))
		if !ok {
			return nil, fmt.Errorf("block gallery: missing palette entry %d", id)
		}
		matched := false
		for _, pattern := range patterns {
			match, _ := path.Match(pattern, name)
			matched = matched || match
		}
		if !matched {
			continue
		}
		state, err := json.Marshal(properties)
		if err != nil {
			return nil, err
		}
		g.states = append(g.states, galleryState{RuntimeID: uint32(id), Name: name, Properties: properties, State: string(state)})
	}
	if len(g.states) == 0 {
		return nil, fmt.Errorf("block gallery filter %q matches no registered states", filter)
	}
	slices.SortFunc(g.states, func(a, b galleryState) int {
		if order := strings.Compare(a.Name, b.Name); order != 0 {
			return order
		}
		return strings.Compare(a.State, b.State)
	})
	for i := range g.states {
		s := &g.states[i]
		s.Chunk = world.ChunkPos{int32(i % galleryColumns), int32(i / galleryColumns)}
		x, z := int(s.Chunk[0])*16, int(s.Chunk[1])*16
		s.Samples = []gallerySample{
			{"isolated", cube.Pos{x + 4, galleryGround + 1, z + 4}},
			{"same", cube.Pos{x + 11, galleryGround + 1, z + 4}},
			{"solid", cube.Pos{x + 4, galleryGround + 1, z + 11}},
			{"cross", cube.Pos{x + 11, galleryGround + 1, z + 11}},
		}
		s.Views = []galleryView{
			{[3]float64{float64(x) + 8, galleryGround + 10, float64(z) - 4}, 0, 30},
			{[3]float64{float64(x) + 20, galleryGround + 7, float64(z) + 8}, 90, 20},
			{[3]float64{float64(x) + 8, galleryGround + 22, float64(z) + 8}, 0, 90},
		}
		g.byChunk[s.Chunk] = i
	}
	return g, nil
}

// configureBlockGallery keeps the opt-in fixture isolated from saved world generators.
func (s settings) configureBlockGallery(conf *server.Config) error {
	if !s.blockGallery {
		return nil
	}
	blocks := conf.Blocks
	if blocks == nil {
		blocks = world.DefaultBlockRegistry
	}
	g, err := newBlockGallery(blocks, s.blockGalleryFilter)
	if err != nil {
		return err
	}
	manifest := struct {
		States  []galleryState `json:"states"`
		Ground  int            `json:"ground_y"`
		Columns int            `json:"columns"`
	}{g.states, galleryGround, galleryColumns}
	data, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(s.dir, 0700); err != nil {
		return err
	}
	if err := os.WriteFile(filepath.Join(s.dir, "block-gallery.json"), append(data, '\n'), 0600); err != nil {
		return err
	}
	conf.RandomTickSpeed = -1
	conf.Generator = func(dim world.Dimension) world.Generator {
		if dim != world.Overworld {
			return world.NopGenerator{}
		}
		return g
	}
	for i, listen := range conf.Listeners {
		conf.Listeners[i] = blockGalleryListener(listen, g)
	}
	return nil
}

// DefaultSpawn starts just north of the first cell, looking into the gallery.
func (*blockGallery) DefaultSpawn(world.Dimension) cube.Pos {
	return cube.Pos{8, galleryGround + 1, -3}
}

// placements visits every sample and its neighbours without changing their state properties.
func (g *blockGallery) placements(s galleryState, put func(cube.Pos, uint32)) {
	for _, sample := range s.Samples {
		p := sample.Position
		put(p, s.RuntimeID)
		switch sample.Scenario {
		case "same", "solid":
			id := s.RuntimeID
			if sample.Scenario == "solid" {
				id = g.stone
			}
			for _, offset := range []cube.Pos{{1, 0, 0}, {0, 1, 0}, {0, 0, 1}} {
				put(p.Add(offset), id)
			}
		case "cross":
			for _, offset := range []cube.Pos{{1, 0, 0}, {-1, 0, 0}, {0, 0, 1}, {0, 0, -1}} {
				put(p.Add(offset), s.RuntimeID)
			}
		}
	}
}

// GenerateChunk writes raw palette IDs so unsupported gameplay blocks survive intact.
func (g *blockGallery) GenerateChunk(pos world.ChunkPos, c *chunk.Chunk) {
	for x := range uint8(16) {
		for z := range uint8(16) {
			c.SetBlock(x, galleryGround, z, 0, g.stone)
			for y := c.Range().Min(); y <= c.Range().Max(); y++ {
				c.SetBiome(x, int16(y), z, uint32(biome.Plains{}.EncodeBiome()))
			}
		}
	}
	if i, ok := g.byChunk[pos]; ok {
		g.placements(g.states[i], func(p cube.Pos, id uint32) { c.SetBlock(uint8(p[0]&15), int16(p[1]), uint8(p[2]&15), 0, id) })
	}
}
