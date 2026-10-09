package experience

import (
	"encoding/json"
	"errors"
	"fmt"
)

// The Go mirror of protocol 5's additions in crates/experience-runtime/src/protocol/visuals.rs
// and player.rs: server WIT 0.5's block states and visuals and the Experience's items, which
// Loaded carries, and the actor's inventory and the network scope, which a CallbackRequest may
// carry, and the server's items, which LoadRequest lists. The goldens keep all of it exact.

// Pixel is a point in a block, in pixels: 0 to 16 on each axis, x east, y up, z south.
type Pixel struct {
	X float32 `json:"x"`
	Y float32 `json:"y"`
	Z float32 `json:"z"`
}

// PixelBox is an axis-aligned box in a block, in pixels.
type PixelBox struct {
	Min Pixel `json:"min"`
	Max Pixel `json:"max"`
}

// RenderMethod is how a material draws (minecraft:material_instances render_method).
type RenderMethod string

const (
	RenderOpaque      RenderMethod = "opaque"
	RenderAlphaTest   RenderMethod = "alpha_test"
	RenderBlend       RenderMethod = "blend"
	RenderDoubleSided RenderMethod = "double_sided"
)

// renderMethods lists every RenderMethod in protocol order.
var renderMethods = []RenderMethod{RenderOpaque, RenderAlphaTest, RenderBlend, RenderDoubleSided}

func (m RenderMethod) MarshalJSON() ([]byte, error) { return marshalEnum(m, renderMethods) }
func (m *RenderMethod) UnmarshalJSON(data []byte) error {
	return unmarshalEnum(data, m, renderMethods)
}

// Flipbook animates a texture as a vertical strip of square frames.
type Flipbook struct {
	TicksPerFrame uint32   `json:"ticks_per_frame"`
	Frames        []uint32 `json:"frames"`
	BlendFrames   bool     `json:"blend_frames"`
}

// Material is one material instance; Path is absolute.
type Material struct {
	Instance     string       `json:"instance"`
	Path         string       `json:"path"`
	RenderMethod RenderMethod `json:"render_method"`
	Flipbook     *Flipbook    `json:"flipbook"`
}

// StateValues are the values one state takes: Bool, or Choices of 1 to 16 strings. Exactly one
// is set.
type StateValues struct {
	Bool    bool
	Choices *[]string
}

// StateDef is a state a block declares.
type StateDef struct {
	Name   string      `json:"name"`
	Values StateValues `json:"values"`
}

// StateValue is one state's value. Exactly one field is set.
type StateValue struct {
	Bool   *bool
	Choice *string
}

// BlockState is a state and its value.
type BlockState struct {
	Name  string     `json:"name"`
	Value StateValue `json:"value"`
}

// BlockStates are a cell's or an op's states; nil encodes as an empty list.
type BlockStates []BlockState

// PlacementState is a Bedrock placement trait state a block takes, set when it is placed.
type PlacementState string

const (
	PlacementCardinalDirection PlacementState = "cardinal_direction"
	PlacementFacingDirection   PlacementState = "facing_direction"
	PlacementBlockFace         PlacementState = "block_face"
	PlacementVerticalHalf      PlacementState = "vertical_half"
)

// placementStates lists every PlacementState in protocol order.
var placementStates = []PlacementState{
	PlacementCardinalDirection, PlacementFacingDirection, PlacementBlockFace,
	PlacementVerticalHalf,
}

func (s PlacementState) MarshalJSON() ([]byte, error) { return marshalEnum(s, placementStates) }
func (s *PlacementState) UnmarshalJSON(data []byte) error {
	return unmarshalEnum(data, s, placementStates)
}

// StateTest is State == value when Equal, else State != value.
type StateTest struct {
	State BlockState `json:"state"`
	Equal bool       `json:"equal"`
}

// Condition is an or of ands of state tests.
type Condition [][]StateTest

// QuarterTurns are quarter turns about each axis, applied x then y then z.
type QuarterTurns struct {
	X uint8 `json:"x"`
	Y uint8 `json:"y"`
	Z uint8 `json:"z"`
}

// BoneVisibility shows Bone only while Visible holds.
type BoneVisibility struct {
	Bone    string    `json:"bone"`
	Visible Condition `json:"visible"`
}

// Visual is what a block looks like; Geometry and material paths are absolute.
type Visual struct {
	Geometry  *string          `json:"geometry"`
	Materials []Material       `json:"materials"`
	Bones     []BoneVisibility `json:"bones"`
	Collision *PixelBox        `json:"collision"`
	Selection *PixelBox        `json:"selection"`
	Rotation  *QuarterTurns    `json:"rotation"`
}

// Permutation holds the parts of a visual that replace the block's while When holds.
type Permutation struct {
	When      Condition         `json:"when"`
	Geometry  *string           `json:"geometry"`
	Materials *[]Material       `json:"materials"`
	Bones     *[]BoneVisibility `json:"bones"`
	Collision *PixelBox         `json:"collision"`
	Selection *PixelBox         `json:"selection"`
	Rotation  *QuarterTurns     `json:"rotation"`
}

// ServerItem carries a registered variant's stack limit, offhand eligibility and classification.
type ServerItem struct {
	ID       string `json:"id"`
	Metadata uint16 `json:"metadata"`
	MaxCount uint8  `json:"max_count"`
	OffHand  bool   `json:"off_hand"`
	Plain    bool   `json:"plain"`
}

// ItemDef is an item of the Experience; Icon is absolute.
type ItemDef struct {
	ID          string `json:"id"`
	DisplayName string `json:"display_name"`
	Icon        string `json:"icon"`
	MaxStack    uint8  `json:"max_stack"`
}

// ItemStack is an item stack; Data, lowercase hex, is only on the Experience's own items, and
// Plain is set when the stack carries nothing else.
type ItemStack struct {
	ID       string  `json:"id"`
	Metadata uint16  `json:"metadata"`
	Count    uint8   `json:"count"`
	MaxCount uint8   `json:"max_count"`
	Data     *string `json:"data"`
	Plain    bool    `json:"plain"`
}

// NewStack is a stack the guest puts somewhere; Data is lowercase hex.
type NewStack struct {
	ID       string  `json:"id"`
	Metadata uint16  `json:"metadata"`
	Count    uint8   `json:"count"`
	Data     *string `json:"data"`
}

// Inventory is the actor's inventory: slots 0 to 8 the hotbar, 9 to 35 the rest of the main
// inventory, 36 the offhand.
type Inventory struct {
	Selected uint8        `json:"selected"`
	Slots    []*ItemStack `json:"slots"`
}

// Network is the network around a callback's anchor: its loaded members, which the snapshot
// holds, and whether its bounds cut it short.
type Network struct {
	Blocks    []BlockPos `json:"blocks"`
	Truncated bool       `json:"truncated"`
}

// SetBlockStateOp sets some states of an owned block at Pos, keeping its data and generation.
type SetBlockStateOp struct {
	Pos    BlockPos    `json:"pos"`
	States BlockStates `json:"states"`
}

// SetSlotOp sets one of the actor's inventory slots; a nil Stack empties it.
type SetSlotOp struct {
	Slot  uint32    `json:"slot"`
	Stack *NewStack `json:"stack"`
}

// DropItemOp spawns an item entity holding Stack at Pos.
type DropItemOp struct {
	Pos   BlockPos `json:"pos"`
	Stack NewStack `json:"stack"`
}

// The "type" of the new union variants on the wire, as protocol.rs names them.
const (
	typeChoices       = "choices"
	typeSetBlockState = "set_block_state"
	typeSetSlot       = "set_slot"
	typeDropItem      = "drop_item"
)

type (
	setBlockStateWire struct {
		variantTag
		*SetBlockStateOp
	}
	setSlotWire struct {
		variantTag
		*SetSlotOp
	}
	dropItemWire struct {
		variantTag
		*DropItemOp
	}
)

func (s BlockStates) MarshalJSON() ([]byte, error) {
	if s == nil {
		return []byte("[]"), nil
	}
	return json.Marshal([]BlockState(s))
}

func (v StateValues) MarshalJSON() ([]byte, error) {
	switch {
	case v.Bool && v.Choices == nil:
		return json.Marshal(variantTag{typeBool})
	case !v.Bool && v.Choices != nil:
		return json.Marshal(scalarWire[[]string]{typeChoices, v.Choices})
	}
	return nil, errors.New("state values must be bool or choices")
}

func (v *StateValues) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*v = StateValues{}
	switch tag {
	case typeBool:
		v.Bool = true
		return decodeStrict(data, &variantTag{})
	case typeChoices:
		return scalarValue(data, &v.Choices)
	}
	return fmt.Errorf("unknown state values type %q", tag)
}

func (v StateValue) MarshalJSON() ([]byte, error) {
	switch {
	case v.Bool != nil:
		return json.Marshal(scalarWire[bool]{typeBool, v.Bool})
	case v.Choice != nil:
		return json.Marshal(scalarWire[string]{typeChoice, v.Choice})
	}
	return nil, errors.New("empty state value")
}

func (v *StateValue) UnmarshalJSON(data []byte) error {
	tag, err := unionTag(data)
	if err != nil {
		return err
	}
	*v = StateValue{}
	switch tag {
	case typeBool:
		return scalarValue(data, &v.Bool)
	case typeChoice:
		return scalarValue(data, &v.Choice)
	}
	return fmt.Errorf("unknown state value type %q", tag)
}
