package experience

import (
	"context"
	"fmt"
	"math"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
)

// simulationKey scopes callback authority to its mod and live world opening.
type simulationKey struct {
	owner string
	world WorldHandle
}

// simulationTimer retains a queued occurrence until the worker has consumed it.
type simulationTimer struct {
	due     uint64
	period  uint32
	pending bool
}

// simulationClock retains instance time while unloaded; timers belong to live openings only.
type simulationClock struct {
	mu     sync.Mutex
	ticks  map[cinnabarext.WorldID]uint64
	timers map[simulationKey]map[ScheduleHandle]simulationTimer
}

// compareSchedules provides stable due-event and snapshot ordering.
func compareSchedules(a, b ScheduleHandle) int {
	if a.Sequence < b.Sequence {
		return -1
	}
	if a.Sequence > b.Sequence {
		return 1
	}
	if a.Slot < b.Slot {
		return -1
	}
	if a.Slot > b.Slot {
		return 1
	}
	return 0
}

// AdmitSimulationTick advances loaded instances and queues work without waiting for a world or guest.
func (h *Host) AdmitSimulationTick() {
	if h.closed.Load() {
		return
	}
	h.mu.Lock()
	paused := h.closing
	select {
	case <-h.resume:
	default:
		paused = true
	}
	h.mu.Unlock()
	if paused {
		return
	}
	// Copy bindings before waiting for a commit: world hooks also resolve bindings.
	h.worlds.mu.RLock()
	instances := make([]worldInstance, 0, len(h.worlds.byWorld))
	for _, instance := range h.worlds.byWorld {
		instances = append(instances, instance)
	}
	h.worlds.mu.RUnlock()
	slices.SortFunc(instances, func(a, b worldInstance) int { return strings.Compare(string(a.handle.ID), string(b.handle.ID)) })
	h.simulation.mu.Lock()
	defer h.simulation.mu.Unlock()
	if h.simulation.ticks == nil {
		h.simulation.ticks = make(map[cinnabarext.WorldID]uint64)
	}
	if h.simulation.timers == nil {
		h.simulation.timers = make(map[simulationKey]map[ScheduleHandle]simulationTimer)
	}
	for _, instance := range instances {
		w, err := h.worlds.resolve(instance.handle)
		if err != nil {
			continue
		}
		id := instance.handle.ID
		tick := h.simulation.ticks[id]
		if tick == math.MaxUint64 {
			continue
		}
		tick++
		h.simulation.ticks[id] = tick
		step := SimulationStep{tick, uint64(tickDuration)}
		for owner, d := range h.dispatchers {
			if !d.sup.registered.Simulation || d.sup.Quarantined() {
				continue
			}
			h.enqueue(owner, event{w: w, instance: instance, call: Call{Tick: &TickCall{step}}})
			timers := h.simulation.timers[simulationKey{owner, instance.handle}]
			handles := make([]ScheduleHandle, 0, len(timers))
			for handle, timer := range timers {
				if timer.due <= tick {
					handles = append(handles, handle)
				}
			}
			slices.SortFunc(handles, compareSchedules)
			for _, handle := range handles {
				timer := timers[handle]
				if timer.period != 0 {
					delta := uint64(timer.period) - (tick-timer.due)%uint64(timer.period)
					timer.due = tick + delta
					if timer.due < tick {
						timer.due = math.MaxUint64
					}
				}
				if timer.pending {
					if timer.period != 0 {
						h.drop(d)
					}
				} else {
					ev := event{w: w, instance: instance, call: Call{Timer: &TimerCall{handle, step}}}
					select {
					case d.events <- ev:
						timer.pending = true
					default:
						h.drop(d)
					}
				}
				timers[handle] = timer
			}
		}
	}
}

// simulationContext snapshots active handles and the event's immutable simulation time.
func (h *Host) simulationContext(owner string, ev event, sequence uint64) *SimulationContext {
	h.simulation.mu.Lock()
	defer h.simulation.mu.Unlock()
	step := SimulationStep{h.simulation.ticks[ev.instance.handle.ID], uint64(tickDuration)}
	if ev.call.Tick != nil {
		step = ev.call.Tick.Step
	}
	if ev.call.Timer != nil {
		step = ev.call.Timer.Step
	}
	handles := make([]ScheduleHandle, 0)
	for handle := range h.simulation.timers[simulationKey{owner, ev.instance.handle}] {
		handles = append(handles, handle)
	}
	slices.SortFunc(handles, compareSchedules)
	return &SimulationContext{step, sequence, handles}
}

// timerCurrent rejects a queued occurrence after cancellation or instance unload.
func (h *Host) timerCurrent(owner string, handle ScheduleHandle) bool {
	if _, err := h.worlds.resolve(handle.World); err != nil {
		return false
	}
	h.simulation.mu.Lock()
	defer h.simulation.mu.Unlock()
	timer, ok := h.simulation.timers[simulationKey{owner, handle.World}][handle]
	return ok && timer.pending
}

// finishTimer consumes one occurrence without resurrecting a cancelled handle.
func (h *Host) finishTimer(owner string, handle ScheduleHandle) {
	h.simulation.mu.Lock()
	defer h.simulation.mu.Unlock()
	timers := h.simulation.timers[simulationKey{owner, handle.World}]
	if timer, ok := timers[handle]; ok {
		if timer.period == 0 {
			delete(timers, handle)
		} else {
			timer.pending = false
			timers[handle] = timer
		}
	}
}

// validateTimers simulates a result under the clock lock before any world writes commit.
func (h *Host) validateTimers(owner string, ev event, snap snapshot, ops []Op) (map[ScheduleHandle]simulationTimer, error) {
	if h.simulation.timers == nil {
		h.simulation.timers = make(map[simulationKey]map[ScheduleHandle]simulationTimer)
	}
	var result map[ScheduleHandle]simulationTimer
	var slot uint32
	for _, op := range ops {
		if op.Schedule == nil && op.CancelSchedule == nil {
			continue
		}
		if snap.req.Info.Simulation == nil {
			return nil, fmt.Errorf("%w: simulation context missing", errInvalid)
		}
		if result == nil {
			result = make(map[ScheduleHandle]simulationTimer)
			for handle, timer := range h.simulation.timers[simulationKey{owner, ev.instance.handle}] {
				result[handle] = timer
			}
		}
		if schedule := op.Schedule; schedule != nil {
			handle := schedule.Handle
			if handle.World != ev.instance.handle || handle.Sequence != snap.req.Info.Simulation.Sequence || handle.Slot != slot || schedule.After == 0 || schedule.After > maxScheduleTicks || schedule.Period > maxScheduleTicks || len(result) >= maxScheduledUpdates {
				return nil, fmt.Errorf("%w: invalid schedule", errInvalid)
			}
			if _, exists := result[handle]; exists {
				return nil, fmt.Errorf("%w: duplicate schedule", errInvalid)
			}
			due := h.simulation.ticks[handle.World.ID] + uint64(schedule.After)
			if due < uint64(schedule.After) {
				return nil, fmt.Errorf("%w: simulation clock exhausted", errInvalid)
			}
			result[handle] = simulationTimer{due: due, period: schedule.Period}
			slot++
		} else {
			handle := op.CancelSchedule.Handle
			if handle.World != ev.instance.handle {
				return nil, fmt.Errorf("%w: foreign timer", errInvalid)
			}
			if _, exists := result[handle]; !exists {
				return nil, errStale
			}
			delete(result, handle)
		}
	}
	return result, nil
}

// forgetSimulation invalidates timers when an opening unloads, retaining its paused clock.
func (h *Host) forgetSimulation(handle WorldHandle) {
	h.simulation.mu.Lock()
	defer h.simulation.mu.Unlock()
	for key := range h.simulation.timers {
		if key.world == handle {
			delete(h.simulation.timers, key)
		}
	}
}

// runSimulationTicks owns the fixed clock edge independently of world snapshot tasks.
func (h *Host) runSimulationTicks(ctx context.Context) {
	ticker := time.NewTicker(tickDuration)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			h.AdmitSimulationTick()
		}
	}
}
