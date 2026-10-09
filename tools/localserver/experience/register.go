package experience

import (
	"bytes"
	"errors"
	"fmt"
	"image"
	"image/png"
	"io"
	"os"
	"slices"
	"strings"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/block/customblock"
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

// Registry holds the registered Experience blocks and items.
type Registry struct {
	types map[string]*blockType // by block id
	items map[string]*itemType  // by item id
}

// Register checks the blocks and items of every loaded Experience and reads their textures,
// geometries and icons. Then it registers every state of each block, its item and each item with
// Dragonfly, and for each Experience a construction creative group named after it, which holds
// its blocks, then its items, and shows the first. A reserved or repeated Experience id, or a bad
// definition, texture, geometry or icon, fails before anything is registered, with an error
// naming the Experience and the block, item or file.
//
// Dragonfly's registries are global: Register may succeed once per process, before
// server.Config.New finalizes them and builds the resource pack from the registered blocks. It is
// not safe for concurrent use.
func Register(loaded []Loaded) (*Registry, error) {
	r := &Registry{types: make(map[string]*blockType), items: make(map[string]*itemType)}
	experiences := make(map[string]bool, len(loaded))
	cache := newAssetCache() // several slots, materials and blocks may share a file
	// blocks[i] and items[i] hold the types of loaded[i]'s blocks and items in their order.
	blocks := make([][]*blockType, len(loaded))
	items := make([][]*itemType, len(loaded))
	for i, l := range loaded {
		if l.ID == vanillaNamespace {
			return nil, fmt.Errorf("experience id %q is reserved", l.ID)
		}
		if experiences[l.ID] {
			return nil, fmt.Errorf("experience %q is loaded twice", l.ID)
		}
		experiences[l.ID] = true
		if len(l.Items) > maxItems {
			return nil, fmt.Errorf("experience %q declares %d items; the limit is %d", l.ID, len(l.Items), maxItems)
		}
		for _, def := range l.Blocks {
			if _, ok := r.types[def.ID]; ok {
				return nil, fmt.Errorf("experience %q: block %q is declared twice", l.ID, def.ID)
			}
			t, err := newBlockType(l.ID, def, cache)
			if err != nil {
				return nil, err
			}
			r.types[def.ID] = t
			blocks[i] = append(blocks[i], t)
		}
		for _, def := range l.Items {
			if _, ok := r.items[def.ID]; ok {
				return nil, fmt.Errorf("experience %q: item %q is declared twice", l.ID, def.ID)
			}
			t, err := newItemType(l.ID, def, l.Blocks, cache)
			if err != nil {
				return nil, err
			}
			r.items[def.ID] = t
			items[i] = append(items[i], t)
		}
	}
	// Every definition is valid; only now does anything reach Dragonfly.
	for i, types := range blocks {
		for _, t := range types {
			t.hash = block.NextHash()
			for s := range t.combinations {
				world.RegisterBlock(Block{t, s})
			}
			world.RegisterItem(Block{t, 0})
		}
		for _, t := range items[i] {
			world.RegisterItem(Item{t})
		}
		var stacks []item.Stack
		for _, t := range types {
			stacks = append(stacks, item.NewStack(Block{t: t}, 1))
		}
		for _, t := range items[i] {
			stacks = append(stacks, item.NewStack(Item{t}, 1))
		}
		if len(stacks) == 0 {
			continue
		}
		exp := loaded[i].ID
		creative.RegisterGroup(creative.Group{
			Category: creative.ConstructionCategory(),
			Name:     exp,
			Icon:     stacks[0],
		})
		for _, stack := range stacks {
			creative.RegisterItem(creative.Item{Stack: stack, Group: exp})
		}
	}
	return r, nil
}

// Lookup returns the registered block with the id, in its default state.
func (r *Registry) Lookup(id string) (Block, bool) {
	t, ok := r.types[id]
	return Block{t: t}, ok
}

// newBlockType checks def, a block of the Experience exp, as the runtime does, and reads its
// textures and geometries through cache. The type it returns is not registered and has no hash
// yet.
func newBlockType(exp string, def BlockDef, cache *assetCache) (*blockType, error) {
	t, err := blockTypeOf(exp, def, cache)
	if err != nil {
		return nil, fmt.Errorf("experience %q: block %q: %w", exp, def.ID, err)
	}
	return t, nil
}

func blockTypeOf(exp string, def BlockDef, cache *assetCache) (*blockType, error) {
	t := &blockType{
		exp:       exp,
		id:        def.ID,
		name:      def.DisplayName,
		textures:  make(map[string]image.Image, len(def.Textures)),
		slots:     make(map[string]string, len(def.Textures)),
		placement: def.Placement,
		traits:    traitsOf(def.Placement),
		declared:  make(map[string][]any, len(def.States)),
		network:   def.Network,
	}
	b := Block{t: t}
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
		return nil, errors.New("it has no mining")
	}
	t.breakInfo = info

	axes, err := stateAxes(exp, def.States, t.traits)
	if err != nil {
		return nil, err
	}
	t.axes, t.radix, t.combinations = axes, make([]uint32, len(axes)), 1
	for i, a := range axes {
		t.radix[i] = t.combinations
		t.combinations *= uint32(len(a.values))
		if !slices.ContainsFunc(t.traits, func(trait customblock.Trait) bool {
			return slices.ContainsFunc(trait.States(), func(s customblock.TraitState) bool { return s.Name == a.name })
		}) {
			t.declared[a.name] = a.values
		}
	}

	if def.Visual != nil {
		if len(def.Textures) > 0 {
			return nil, errors.New("it binds textures and declares a visual, whose materials replace them")
		}
		if err := newVisual(exp, def, t, cache); err != nil {
			return nil, err
		}
		return t, nil
	}
	if len(def.Permutations) > 0 {
		return nil, errors.New("it declares permutations but no visual for them to change")
	}
	for _, texture := range def.Textures {
		img, err := cache.image(texture.Path)
		if err != nil {
			return nil, err
		}
		key := textureKey(def.ID, texture.Slot)
		t.textures[key] = img
		t.slots[texture.Slot] = key
	}
	if b.Texture() == nil {
		return nil, fmt.Errorf("it has no %q or %q texture for its item", allFaces, FaceUp)
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
