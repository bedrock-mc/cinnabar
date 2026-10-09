package experience

import "time"

// dataQuota is the total number of private data bytes one Experience may store.
const dataQuota = 16 << 20

// flushInterval is how often RunFlusher writes dirty Experience data to disk.
const flushInterval = 5 * time.Second

// loadDeadline bounds how long a helper may take to answer load, compilation included.
const loadDeadline = 30 * time.Second

// resultDeadline bounds how long a helper may take to answer one callback.
const resultDeadline = 2 * time.Second

// maxRestarts is how many helper restarts restartWindow may hold; one more quarantines the
// Experience.
const maxRestarts = 3

// restartWindow is the span over which helper restarts are counted.
const restartWindow = 5 * time.Minute

// strikeLimit is how many strikes within strikeWindow quarantine the Experience.
const strikeLimit = 3

// strikeWindow is the span over which strikes are counted.
const strikeWindow = time.Minute

// shutdownGrace is how long a helper may take to exit after the shutdown frame before it is
// killed.
const shutdownGrace = time.Second

// maxFrameBytes is the largest frame body. It must equal the Rust runtime's MAX_FRAME_BYTES,
// which TestFrameLimitMatchesRust checks against the limits fixture.
const maxFrameBytes = 4 << 20

// stderrLineBytes is the longest helper stderr line that is logged; the rest of a longer line is
// dropped.
const stderrLineBytes = 4 << 10

// maxTextureBytes is the largest texture file that an Experience block may use.
const maxTextureBytes = 4 << 20

// maxTextureSide is the largest width and height of an Experience block's texture, in pixels.
const maxTextureSide = 1024

// eventQueueCap is how many events one Experience's queue holds; an event that finds it full is
// dropped and counted.
const eventQueueCap = 256

// maxNeighborEventsPerTick is how many neighbor events, one per position, one Experience admits
// per tick of a world.
const maxNeighborEventsPerTick = 64

// dropLogInterval is how often, at most, an Experience's dropped events are logged.
const dropLogInterval = time.Second

// provisionalFocusRange is the farthest, in blocks from a player's eyes to the block's centre,
// that a player's focus counts. PROVISIONAL, labeled incomplete in plan.md. Vanilla closes a
// block container screen when the player is farther than its pick range from the block,
// comparing the squared distance from the player's eye position to the block's centre. That
// range has one constant for touch input, one for another input mode, and otherwise one for
// survival and one for creative. Those constants are not yet known, so the value is still
// Dragonfly's survival reach for using a block, and one range serves every input mode and game
// mode.
const provisionalFocusRange = 8.0

// The commit check enforces these runtime limits again. Each must equal its Rust constant, which
// TestCommitLimitsMatchRust checks against the limits fixture.
const (
	// maxBlockDataBytes is the most data one block may hold: Rust's MAX_BLOCK_DATA_BYTES.
	maxBlockDataBytes = 65_536
	// maxStagedOps is the most ops one callback may commit: Rust's MAX_STAGED_OPS.
	maxStagedOps = 64
	// maxTells is the most tells one callback may send: Rust's MAX_TELLS.
	maxTells = 4
	// maxTellBytes is the most UTF-8 bytes of one tell: Rust's MAX_TELL_BYTES.
	maxTellBytes = 256
	// maxClientSends is the most client messages one callback may send: Rust's
	// MAX_CLIENT_SENDS.
	maxClientSends = 8
)

// Registration checks a block type by the runtime's bounds again. Each must equal its Rust
// constant, which TestCommitLimitsMatchRust checks against the limits fixture.
const (
	// maxNameBytes is the most bytes in a state's name after "<id>:", a string state value, a
	// material instance and a geometry's name: Rust's MAX_NAME_BYTES.
	maxNameBytes = 64
	// maxStateValues is the most values of one string state: Rust's MAX_STATE_VALUES.
	maxStateValues = 16
	// maxStateCombinations is the most state combinations of one block: Rust's
	// MAX_STATE_COMBINATIONS.
	maxStateCombinations = 65_536
	// maxBones is the most bones whose visibility one visual or permutation sets: Rust's
	// MAX_BONES.
	maxBones = 64
	// maxPermutations is the most permutations of one block: Rust's MAX_PERMUTATIONS.
	maxPermutations = 64
	// maxMaterials is the most materials of one visual or permutation: Rust's MAX_MATERIALS.
	maxMaterials = 32
	// maxConditionTests is the most state tests of one condition: Rust's MAX_CONDITION_TESTS.
	maxConditionTests = 64
	// maxFlipbookFrames is the most frames one flipbook lists: Rust's MAX_FLIPBOOK_FRAMES.
	maxFlipbookFrames = 256
	// maxGeometryBytes is the largest geometry file: Rust's MAX_GEOMETRY_BYTES.
	maxGeometryBytes = 1 << 20
)

// The network bounds, which the flood keeps. Each must equal its Rust constant, which
// TestCommitLimitsMatchRust checks against the limits fixture.
const (
	// maxNetworkBlocks is the most members of one callback's network: Rust's
	// MAX_NETWORK_BLOCKS.
	maxNetworkBlocks = 1024
	// maxNetworkDataBytes is the most data of one callback's network, summed over its members:
	// Rust's MAX_NETWORK_DATA_BYTES.
	maxNetworkDataBytes = 524_288
)

// The inventory's shape and the item bounds. Each must equal its Rust constant, which
// TestCommitLimitsMatchRust checks against the limits fixture.
const (
	// inventorySlots is the slots of an actor's inventory: the 36 of the main inventory, the
	// hotbar first, then the offhand. Rust's INVENTORY_SLOTS.
	inventorySlots = 37
	// hotbarSlots is the slots of the hotbar, of which one is selected: Rust's HOTBAR_SLOTS.
	hotbarSlots = 9
	// maxItems is the most items one Experience registers: Rust's MAX_ITEMS.
	maxItems = 64
	// maxStackSize is the most one stack of an Experience's item holds: Rust's MAX_STACK_SIZE.
	maxStackSize = 64
	// maxItemDataBytes is the most data of an Experience's own on one stack: Rust's
	// MAX_ITEM_DATA_BYTES.
	maxItemDataBytes = 8192
	// maxServerItems is the most items the adapter lists as the server's: Rust's
	// MAX_SERVER_ITEMS.
	maxServerItems = 16_384
)
