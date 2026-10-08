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

// fullCube is the collision and selection box of every Experience block.
var fullCube = cube.Box(0, 0, 0, 1, 1, 1)

// blockType is one registered Experience block, shared by every Block of the type.
type blockType struct {
	// exp is the id of the Experience, id the block's id and name its display name.
	exp, id, name string
	// hash is the base hash from block.NextHash, taken at registration.
	hash uint64
	// textures holds the decoded texture of each texture key, and slots the texture key of each
	// bound material slot.
	textures map[string]image.Image
	slots    map[string]string
	// breakInfo is built once, so that BreakInfo allocates nothing.
	breakInfo block.BreakInfo
}

// Block is an Experience block, in a world or as an item. It holds nothing but its type: an
// Experience keeps a block's data in the Store, so the data never reaches chunk NBT, and every
// Block of a type is equal and hashes alike.
type Block struct{ t *blockType }

// The Dragonfly interfaces that Block implements; they keep its method signatures exact.
var (
	_ world.CustomBlockBuildable  = Block{}
	_ world.CustomItem            = Block{}
	_ block.Breakable             = Block{}
	_ block.Activatable           = Block{}
	_ item.UsableOnBlock          = Block{}
	_ world.NeighbourUpdateTicker = Block{}
)

// EncodeBlock returns the block's id. An Experience block has no states.
func (b Block) EncodeBlock() (string, map[string]any) {
	return b.t.id, map[string]any{}
}

// Hash returns the type's base hash; there is no state to hash.
func (b Block) Hash() (uint64, uint64) {
	return b.t.hash, 0
}

// Model is a full solid cube.
func (Block) Model() world.BlockModel {
	return model.Solid{}
}

// Properties make the block a full cube that collides and is selected as one, with an opaque
// material for each bound slot.
func (b Block) Properties() customblock.Properties {
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

// Geometry is nil: the block is the default cube.
func (Block) Geometry() []byte {
	return nil
}

// Textures holds the decoded texture of each texture key, which the resource pack carries.
func (b Block) Textures() map[string]image.Image {
	return b.t.textures
}

// EncodeItem returns the block's id as its item's.
func (b Block) EncodeItem() (string, int16) {
	return b.t.id, 0
}

// Texture is the item's texture: the "*" texture, else the "up" face's.
func (b Block) Texture() image.Image {
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
