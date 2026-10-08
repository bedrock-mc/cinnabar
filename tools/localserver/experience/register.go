package experience

import (
	"bytes"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"path/filepath"
	"strings"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/creative"
	"github.com/df-mc/dragonfly/server/world"
)

// unbreakableHardness is the hardness that Dragonfly and the client read as never broken by
// mining.
const unbreakableHardness = -1

// unbreakableBlastResistance keeps explosions off an unbreakable block. It is vanilla bedrock's
// blast resistance, which every vanilla block that mining never breaks shares.
const unbreakableBlastResistance = 3_600_000

// harvestableByHand and effectiveWithNoTool make a block harvestable by hand and mined no faster
// with any tool, which is how the client times the break.
var (
	harvestableByHand   = func(item.Tool) bool { return true }
	effectiveWithNoTool = func(item.Tool) bool { return false }
)

// vanillaNamespace is the namespace of vanilla blocks and items. It is no Experience's id: a block
// in it would collide with a vanilla block, which Dragonfly refuses with a panic, or pose as one.
const vanillaNamespace = "minecraft"

// Registry holds the registered Experience blocks.
type Registry struct {
	types map[string]*blockType // by block id
}

// Register checks the blocks of every loaded Experience and decodes their textures. Then it
// registers each block and its item with Dragonfly, and for each Experience a construction
// creative group named after it, which holds its blocks and shows the first. A reserved or
// repeated Experience id, or a bad definition or texture, fails before anything is registered,
// with an error naming the Experience and the block or file.
//
// Dragonfly's registries are global: Register may succeed once per process, before
// server.Config.New finalizes them and builds the resource pack from the registered blocks. It is
// not safe for concurrent use.
func Register(loaded []Loaded) (*Registry, error) {
	r := &Registry{types: make(map[string]*blockType)}
	experiences := make(map[string]bool, len(loaded))
	decoded := make(map[string]image.Image) // by file path, which several slots may bind
	// blocks[i] holds the types of loaded[i]'s blocks in their order.
	blocks := make([][]*blockType, len(loaded))
	for i, l := range loaded {
		if l.ID == vanillaNamespace {
			return nil, fmt.Errorf("experience id %q is reserved", l.ID)
		}
		if experiences[l.ID] {
			return nil, fmt.Errorf("experience %q is loaded twice", l.ID)
		}
		experiences[l.ID] = true
		for _, def := range l.Blocks {
			if _, ok := r.types[def.ID]; ok {
				return nil, fmt.Errorf("experience %q: block %q is declared twice", l.ID, def.ID)
			}
			t, err := newBlockType(l.ID, def, decoded)
			if err != nil {
				return nil, err
			}
			r.types[def.ID] = t
			blocks[i] = append(blocks[i], t)
		}
	}
	// Every definition is valid; only now does anything reach Dragonfly.
	for i, types := range blocks {
		for _, t := range types {
			t.hash = block.NextHash()
			world.RegisterBlock(Block{t})
			world.RegisterItem(Block{t})
		}
		if len(types) == 0 {
			continue
		}
		exp := loaded[i].ID
		creative.RegisterGroup(creative.Group{
			Category: creative.ConstructionCategory(),
			Name:     exp,
			Icon:     item.NewStack(Block{types[0]}, 1),
		})
		for _, t := range types {
			creative.RegisterItem(creative.Item{Stack: item.NewStack(Block{t}, 1), Group: exp})
		}
	}
	return r, nil
}

// Lookup returns the registered block with the id.
func (r *Registry) Lookup(id string) (Block, bool) {
	t, ok := r.types[id]
	return Block{t}, ok
}

// newBlockType checks def, a block of the Experience exp, and decodes its textures, taking those
// already in decoded from there and adding the rest. The type it returns is not registered and
// has no hash yet.
func newBlockType(exp string, def BlockDef, decoded map[string]image.Image) (*blockType, error) {
	t := &blockType{
		exp:      exp,
		id:       def.ID,
		name:     def.DisplayName,
		textures: make(map[string]image.Image, len(def.Textures)),
		slots:    make(map[string]string, len(def.Textures)),
	}
	b := Block{t}
	info := block.BreakInfo{
		Harvestable: harvestableByHand,
		Effective:   effectiveWithNoTool,
		Drops: func(item.Tool, []item.Enchantment) []item.Stack {
			return []item.Stack{item.NewStack(b, 1)}
		},
		BreakHandler: func(pos cube.Pos, tx *world.Tx, u item.User) {
			currentHooks().breakHandler(b, pos, tx, u)
		},
	}
	switch m := def.Mining; {
	case m.Breakable != nil:
		info.Hardness = float64(m.Breakable.Hardness)
		// Dragonfly's own blocks resist explosions as much as mining unless they say otherwise.
		info.BlastResistance = info.Hardness
	case m.Unbreakable != nil:
		info.Hardness = unbreakableHardness
		info.BlastResistance = unbreakableBlastResistance
	default:
		return nil, fmt.Errorf("experience %q: block %q has no mining", exp, def.ID)
	}
	t.breakInfo = info

	for _, texture := range def.Textures {
		img, ok := decoded[texture.Path]
		if !ok {
			var err error
			if img, err = loadTexture(texture.Path); err != nil {
				return nil, fmt.Errorf("experience %q: block %q: texture %s: %w",
					exp, def.ID, filepath.Base(texture.Path), err)
			}
			decoded[texture.Path] = img
		}
		key := textureKey(def.ID, texture.Slot)
		t.textures[key] = img
		t.slots[texture.Slot] = key
	}
	if b.Texture() == nil {
		return nil, fmt.Errorf("experience %q: block %q has no %q or %q texture for its item",
			exp, def.ID, allFaces, FaceUp)
	}
	return t, nil
}

// textureKey is the texture key of a block's material slot: "<ns>.<name>.<slot>", with "*" as
// "all". It is built from the whole block id, so one block name in two Experiences gets two keys.
func textureKey(id, slot string) string {
	ns, name, _ := strings.Cut(id, ":")
	if slot == allFaces {
		slot = "all"
	}
	return ns + "." + name + "." + slot
}

// loadTexture reads and decodes the PNG file at path. It refuses a file over maxTextureBytes
// before reading it, and an image wider or higher than maxTextureSide before decoding its pixels.
func loadTexture(path string) (image.Image, error) {
	f, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	info, err := f.Stat()
	if err != nil {
		return nil, err
	}
	if info.Size() > maxTextureBytes {
		return nil, fmt.Errorf("the file has %d bytes; the limit is %d", info.Size(), maxTextureBytes)
	}
	data := make([]byte, info.Size())
	if _, err := io.ReadFull(f, data); err != nil {
		return nil, err
	}
	config, err := png.DecodeConfig(bytes.NewReader(data))
	if err != nil {
		return nil, err
	}
	if config.Width > maxTextureSide || config.Height > maxTextureSide {
		return nil, fmt.Errorf("the image is %d×%d pixels; the limit is %d×%d",
			config.Width, config.Height, maxTextureSide, maxTextureSide)
	}
	return png.Decode(bytes.NewReader(data))
}
