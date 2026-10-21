package experience

// Server WIT 0.6's part of the protocol (protocol 6): the server's crafting recipes, which load
// carries, the scheduled_update call and the schedule_update op.

// ItemMatch is an item a recipe input accepts: ID, with Metadata, or any metadata when nil.
type ItemMatch struct {
	ID       string  `json:"id"`
	Metadata *uint16 `json:"metadata"`
}

// Crafted is what a recipe makes.
type Crafted struct {
	ID       string `json:"id"`
	Metadata uint16 `json:"metadata"`
	Count    uint8  `json:"count"`
}

// Recipe is a crafting-table recipe. A shaped one has its Shape, width then height, and Inputs row
// by row, an empty list for an empty cell; a shapeless one has no Shape and an input per item it
// takes. Each input lists the items it accepts.
type Recipe struct {
	Shape  *[2]uint8     `json:"shape"`
	Inputs [][]ItemMatch `json:"inputs"`
	Output Crafted       `json:"output"`
}

// ScheduledUpdateCall runs when an update the Experience scheduled for its block at Pos is due.
// Its snapshot is that of an interaction with the block.
type ScheduledUpdateCall struct {
	Pos BlockPos `json:"pos"`
}

// ScheduleUpdateOp schedules an update of the owned block at Pos, Ticks game ticks after the
// result commits.
type ScheduleUpdateOp struct {
	Pos   BlockPos `json:"pos"`
	Ticks uint32   `json:"ticks"`
}

// The "type" of 0.6's union variants on the wire, as protocol.rs names them.
const (
	typeScheduledUpdate = "scheduled_update"
	typeScheduleUpdate  = "schedule_update"
)

type (
	scheduledUpdateWire struct {
		variantTag
		*ScheduledUpdateCall
	}
	scheduleUpdateWire struct {
		variantTag
		*ScheduleUpdateOp
	}
)
