package experience

// SimulationStep is fixed-step time for one live world instance.
type SimulationStep struct {
	Tick          uint64 `json:"tick"`
	DurationNanos uint64 `json:"duration_nanos"`
}

// ScheduleHandle identifies a mod's callback in one instance opening.
type ScheduleHandle struct {
	World    WorldHandle `json:"world"`
	Sequence uint64      `json:"sequence"`
	Slot     uint32      `json:"slot"`
}

// SimulationContext contains the current step and cancellable callback identities.
type SimulationContext struct {
	Step      SimulationStep   `json:"step"`
	Sequence  uint64           `json:"sequence"`
	Schedules []ScheduleHandle `json:"schedules"`
}

// TickCall carries one admitted simulation step.
type TickCall struct {
	Step SimulationStep `json:"step"`
}

// TimerCall carries one admitted scheduled callback.
type TimerCall struct {
	Handle ScheduleHandle `json:"handle"`
	Step   SimulationStep `json:"step"`
}

// ScheduleOp registers a callback relative to its commit tick. Zero Period runs once.
type ScheduleOp struct {
	Handle ScheduleHandle `json:"handle"`
	After  uint32         `json:"after"`
	Period uint32         `json:"period"`
}

// CancelScheduleOp cancels an active callback, including any queued occurrence.
type CancelScheduleOp struct {
	Handle ScheduleHandle `json:"handle"`
}

type tickCallWire struct {
	variantTag
	*TickCall
}
type timerCallWire struct {
	variantTag
	*TimerCall
}
type scheduleWire struct {
	variantTag
	*ScheduleOp
}
type cancelScheduleWire struct {
	variantTag
	*CancelScheduleOp
}
