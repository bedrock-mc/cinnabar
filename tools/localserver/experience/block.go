package experience

import (
	"image"
	"sync/atomic"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/block/customblock"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/item/category"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// allFaces is the material slot of every face that a block does not bind on its own, as
// ALL_FACES in the runtime's load.rs names it.
const allFaces = "*"

// fullCube is the collision and selection box of an Experience block without boxes of its own.
var fullCube = cube.Box(0, 0, 0, 1, 1, 1)

// blockType is one registered Experience block, shared by every Block of the type.
type blockType struct {
	// exp is the id of the Experience, id the block's id and name its display name.
	exp, id, name string
	// hash is the base hash from block.NextHash, taken at registration.
	hash uint64
	// textures holds the decoded texture of each texture key, and slots the texture key of each
	// bound material slot of a block without a visual.
	textures map[string]image.Image
	slots    map[string]string
	// breakInfo is built once, so that BreakInfo allocates nothing.
	breakInfo block.BreakInfo
	// axes are the block's states, its placement traits' first; radix is the factor of each in
	// a state index, and combinations the number of state indices.
	axes         []axis
	radix        []uint32
	combinations uint32
	// placement lists the placement traits' states, which a player's placement sets; traits are
	// the fork's traits for them.
	placement []PlacementState
	traits    []customblock.Trait
	// declared holds the values of each state the block declares itself.
	declared map[string][]any
	// visual is the block's look when it declares one; nil draws its textures on the full cube.
	visual *visualType
	// network is set for a network member, whose callbacks have the network around it.
	network bool
}

// Block is an Experience block, in a world or as an item: its type and its state, an index of
// the type's state combinations. An Experience keeps a block's data in the Store, so the data
// never reaches chunk NBT, and two Blocks of a type in the same state are equal and hash alike.
// The item is a Block in state 0, where every state has its first value.
type Block struct {
	t *blockType
	s uint32
}

// The Dragonfly interfaces that Block implements; they keep its method signatures exact.
var (
	_ world.CustomBlockBuildable  = Block{}
	_ world.CustomBlockAnimated   = Block{}
	_ world.CustomBlockGeometries = Block{}
	_ block.Permutable            = Block{}
	_ block.Traited               = Block{}
	_ world.CustomItem            = Block{}
	_ block.Breakable             = Block{}
	_ block.Activatable           = Block{}
	_ item.UsableOnBlock          = Block{}
	_ world.NeighbourUpdateTicker = Block{}
)

// EncodeBlock returns the block's id and the value of each of its states.
func (b Block) EncodeBlock() (string, map[string]any) {
	properties := make(map[string]any, len(b.t.axes))
	for i, a := range b.t.axes {
		properties[a.name] = a.values[b.index(i)]
	}
	return b.t.id, properties
}

// Hash returns the type's base hash and the state index.
func (b Block) Hash() (uint64, uint64) {
	return b.t.hash, uint64(b.s)
}

// Model is a full solid cube, unless the block's collision box in its state is another box.
func (b Block) Model() world.BlockModel {
	if box := b.collision(); box != fullCube {
		return boxModel{box}
	}
	return model.Solid{}
}

// collision is the block's collision box in its state: its visual's, as the last permutation
// that holds and sets one replaces it.
func (b Block) collision() cube.BBox {
	if b.t.visual == nil {
		return fullCube
	}
	box := b.t.visual.base.CollisionBox
	for _, p := range b.t.visual.permutations {
		if p.properties.CollisionBox != (cube.BBox{}) && holds(p.when, b) {
			box = p.properties.CollisionBox
		}
	}
	return box
}

// boxModel collides as one box smaller than the block, so none of its faces is solid.
type boxModel struct{ box cube.BBox }

func (m boxModel) BBox(cube.Pos, world.BlockSource) []cube.BBox { return []cube.BBox{m.box} }

func (boxModel) FaceSolid(cube.Pos, cube.Face, world.BlockSource) bool { return false }

// Properties are the block's visual without its permutations; without one, the block is a full
// cube that collides and is selected as one, with an opaque material for each bound slot.
func (b Block) Properties() customblock.Properties {
	if b.t.visual != nil {
		return b.t.visual.base
	}
	materials := make(map[string]customblock.Material, len(b.t.slots))
	for slot, key := range b.t.slots {
		materials[slot] = customblock.NewMaterial(key, customblock.OpaqueRenderMethod())
	}
	return customblock.Properties{
		Cube:         true,
		CollisionBox: fullCube,
		SelectionBox: fullCube,
		Textures:     materials,
	}
}

// Name is the display name of the block and its item.
func (b Block) Name() string {
	return b.t.name
}

// Geometry is the geometry file of the block's visual; nil for the full cube.
func (b Block) Geometry() []byte {
	if b.t.visual == nil {
		return nil
	}
	return b.t.visual.geometry
}

// Geometries are the further geometry files of the block's permutations, by name.
func (b Block) Geometries() map[string][]byte {
	if b.t.visual == nil {
		return nil
	}
	return b.t.visual.further
}

// Flipbooks animates the block's textures by texture key.
func (b Block) Flipbooks() map[string]customblock.Flipbook {
	if b.t.visual == nil {
		return nil
	}
	return b.t.visual.flipbooks
}

// States are the values of each state the block declares itself; its placement traits add
// theirs.
func (b Block) States() map[string][]any {
	return b.t.declared
}

// Permutations are the block's permutations in order, each with its condition as Molang.
func (b Block) Permutations() []customblock.Permutation {
	if b.t.visual == nil {
		return nil
	}
	permutations := make([]customblock.Permutation, len(b.t.visual.permutations))
	for i, p := range b.t.visual.permutations {
		permutations[i] = customblock.Permutation{Properties: p.properties, Condition: molang(p.when)}
	}
	return permutations
}

// Traits are the placement traits whose states the block takes.
func (b Block) Traits() []customblock.Trait {
	return b.t.traits
}

// Textures holds the decoded texture of each texture key, which the resource pack carries.
func (b Block) Textures() map[string]image.Image {
	return b.t.textures
}

// EncodeItem returns the block's id as its item's.
func (b Block) EncodeItem() (string, int16) {
	return b.t.id, 0
}

// Texture is the item's texture: the "*" texture, else the "up" face's, or with a visual the
// texture of its "*" material, else its first.
func (b Block) Texture() image.Image {
	if b.t.visual != nil {
		return b.t.textures[b.t.visual.icon]
	}
	key, ok := b.t.slots[allFaces]
	if !ok {
		key = b.t.slots[string(FaceUp)]
	}
	return b.t.textures[key]
}

// Category puts the item in the construction tab.
func (Block) Category() category.Category {
	return category.Construction()
}

// BreakInfo follows the block's mining; its BreakHandler hands the break to the hook sink.
func (b Block) BreakInfo() block.BreakInfo {
	return b.t.breakInfo
}

// UseOnBlock hands the use of the block's item on the block at pos to the hook sink, which does
// the placing.
func (b Block) UseOnBlock(
	pos cube.Pos, face cube.Face, clickPos mgl64.Vec3, tx *world.Tx, user item.User,
	ctx *item.UseContext,
) bool {
	return currentHooks().useOnBlock(b, pos, face, clickPos, tx, user, ctx)
}

// Activate hands an interaction with the block to the hook sink. It always consumes the
// interaction.
func (b Block) Activate(
	pos cube.Pos, clickedFace cube.Face, tx *world.Tx, u item.User, ctx *item.UseContext,
) bool {
	currentHooks().activate(b, pos, clickedFace, tx, u, ctx)
	return true
}

// NeighbourUpdateTick hands a change next to the block to the hook sink.
func (b Block) NeighbourUpdateTick(pos, changedNeighbour cube.Pos, tx *world.Tx) {
	currentHooks().neighbourUpdateTick(b, pos, changedNeighbour, tx)
}

// hookSink receives the hooks of every Experience block, with the block and the hook's own
// arguments, on the goroutine of the block's world.
type hookSink interface {
	// useOnBlock handles the block's item used on the block at pos and reports whether it was
	// used.
	useOnBlock(
		b Block, pos cube.Pos, face cube.Face, clickPos mgl64.Vec3, tx *world.Tx, user item.User,
		ctx *item.UseContext,
	) bool
	// activate handles an interaction with the block at pos.
	activate(
		b Block, pos cube.Pos, clickedFace cube.Face, tx *world.Tx, u item.User,
		ctx *item.UseContext,
	)
	// neighbourUpdateTick handles a change at changedNeighbour next to the block at pos.
	neighbourUpdateTick(b Block, pos, changedNeighbour cube.Pos, tx *world.Tx)
	// breakHandler handles the block at pos having been broken.
	breakHandler(b Block, pos cube.Pos, tx *world.Tx, u item.User)
}

// sinkHolder lets an atomic.Pointer hold a hookSink.
type sinkHolder struct{ hookSink }

// installedHooks holds the hook sink that the block hooks call, which the world goroutines read.
// A Host installs itself; until then, and after it closes, it is nil.
var installedHooks atomic.Pointer[sinkHolder]

// currentHooks returns the installed hook sink, or noHooks, which ignores every hook, when none
// is installed.
func currentHooks() hookSink {
	if s := installedHooks.Load(); s != nil {
		return s.hookSink
	}
	return noHooks{}
}

// noHooks ignores every hook. Its useOnBlock uses nothing, so the item places nothing.
type noHooks struct{}

func (noHooks) useOnBlock(
	Block, cube.Pos, cube.Face, mgl64.Vec3, *world.Tx, item.User, *item.UseContext,
) bool {
	return false
}

func (noHooks) activate(Block, cube.Pos, cube.Face, *world.Tx, item.User, *item.UseContext) {}

func (noHooks) neighbourUpdateTick(Block, cube.Pos, cube.Pos, *world.Tx) {}

func (noHooks) breakHandler(Block, cube.Pos, *world.Tx, item.User) {}
