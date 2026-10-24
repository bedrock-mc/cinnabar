package experience

import (
	"errors"
	"testing"

	"github.com/df-mc/dragonfly/server/world"
)

// simulationFixture enables callbacks and removes the fixture's initial player observation.
func simulationFixture(t *testing.T) *runtimeFixture {
	t.Helper()
	f := newRuntimeFixture(t)
	f.d.sup.registered.Simulation = true
	<-f.d.events
	return f
}

// simulationEvent finds this instance's next tick while retaining FIFO order within it.
func simulationEvent(t *testing.T, f *runtimeFixture, id string) event {
	t.Helper()
	for len(f.d.events) != 0 {
		ev := <-f.d.events
		if string(ev.instance.handle.ID) == id {
			return ev
		}
	}
	t.Fatal("missing simulation event")
	return event{}
}

// applySimulation commits a result using the actual snapshot and validation path.
func applySimulation(t *testing.T, f *runtimeFixture, ev event, ops func(snapshot) []Op) snapshot {
	t.Helper()
	f.d.seq++
	snap, err := f.h.snapshot(t.Context(), f.d, &ev)
	if err != nil {
		t.Fatal(err)
	}
	f.do(ev.w, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, f.d.id, ev, snap, ops(snap)); err != nil {
			t.Fatal(err)
		}
	})
	return snap
}

func TestSimulationCadenceAndTimerOrdering(t *testing.T) {
	f := simulationFixture(t)
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	if ev.call.Tick.Step.Tick != 1 || ev.call.Tick.Step.DurationNanos != uint64(tickDuration) {
		t.Fatal("wrong fixed step")
	}
	var handle ScheduleHandle
	applySimulation(t, f, ev, func(s snapshot) []Op {
		handle = ScheduleHandle{World: ev.instance.handle, Sequence: s.req.Info.Simulation.Sequence}
		return []Op{{Schedule: &ScheduleOp{handle, 2, 2}}}
	})
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Timer != nil {
		t.Fatal("timer fired early")
	}
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Tick == nil || ev.call.Tick.Step.Tick != 3 {
		t.Fatal("tick must precede timers")
	}
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Timer == nil || ev.call.Timer.Handle != handle || ev.call.Timer.Step.Tick != 3 {
		t.Fatal("missing due timer")
	}
	f.h.finishTimer(f.d.id, handle)
	f.h.AdmitSimulationTick()
	simulationEvent(t, f, "probe:one")
	f.h.AdmitSimulationTick()
	simulationEvent(t, f, "probe:one")
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Timer == nil || ev.call.Timer.Step.Tick != 5 {
		t.Fatal("repeat cadence changed")
	}
}

func TestSimulationCancelFencesQueuedTimer(t *testing.T) {
	f := simulationFixture(t)
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	var handle ScheduleHandle
	applySimulation(t, f, ev, func(s snapshot) []Op {
		handle = ScheduleHandle{World: ev.instance.handle, Sequence: s.req.Info.Simulation.Sequence}
		return []Op{{Schedule: &ScheduleOp{handle, 1, 0}}}
	})
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, "probe:one")
	applySimulation(t, f, ev, func(snapshot) []Op { return []Op{{CancelSchedule: &CancelScheduleOp{handle}}} })
	ev = simulationEvent(t, f, "probe:one")
	if ev.call.Timer == nil || f.h.timerCurrent(f.d.id, handle) {
		t.Fatal("cancelled queued timer retained authority")
	}
}

func TestSimulationPausesOnUnloadAndHostPause(t *testing.T) {
	f := simulationFixture(t)
	f.h.AdmitSimulationTick()
	ev := simulationEvent(t, f, "probe:one")
	old := ev.instance.handle
	if err := f.h.unloadWorld(t.Context(), f.d.id, old); err != nil {
		t.Fatal(err)
	}
	f.h.AdmitSimulationTick()
	f.h.AdmitSimulationTick()
	if f.h.simulation.ticks[old.ID] != 1 {
		t.Fatal("unloaded instance time advanced")
	}
	fresh, err := f.h.createWorld(t.Context(), f.d.id, WorldInstance{ID: old.ID, Definition: "probe:plain"})
	if err != nil {
		t.Fatal(err)
	}
	for len(f.d.events) > 0 {
		<-f.d.events
	}
	f.h.Pause(true)
	f.h.AdmitSimulationTick()
	if len(f.d.events) != 0 {
		t.Fatal("paused host admitted simulation")
	}
	f.h.Pause(false)
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, "probe:one")
	if ev.instance.handle != fresh || ev.call.Tick.Step.Tick != 2 {
		t.Fatal("reopened instance lost paused time or reused generation")
	}
	if _, err := f.h.snapshot(t.Context(), f.d, &event{w: ev.w, instance: worldInstance{handle: old}, call: ev.call}); !errors.Is(err, errStale) {
		t.Fatal("old opening retained callback authority")
	}
}

func TestSimulationOverflowIsNonblockingAndCounted(t *testing.T) {
	f := simulationFixture(t)
	for len(f.d.events) < cap(f.d.events) {
		f.d.events <- event{}
	}
	f.h.AdmitSimulationTick()
	if f.h.simulation.ticks[OverworldInstance] != 1 {
		t.Fatal("full queue stopped simulation")
	}
	if n := dropCountOf(&f.d.dropped); n == 0 {
		t.Fatal("overflow was not counted")
	}
}

func TestSimulationTimerCommittedBeforeFirstTickSurvivesClockStart(t *testing.T) {
	f := simulationFixture(t)
	ev, snap := f.request()
	handle := ScheduleHandle{World: ev.instance.handle, Sequence: snap.req.Info.Simulation.Sequence}
	f.do(ev.w, func(tx *world.Tx) {
		if _, err := f.h.apply(tx, f.d.id, ev, snap, []Op{{Schedule: &ScheduleOp{handle, 1, 0}}}); err != nil {
			t.Fatal(err)
		}
	})
	f.h.AdmitSimulationTick()
	ev = simulationEvent(t, f, string(handle.World.ID))
	if ev.call.Tick == nil {
		t.Fatal("first tick missing")
	}
	ev = simulationEvent(t, f, string(handle.World.ID))
	if ev.call.Timer == nil || ev.call.Timer.Handle != handle {
		t.Fatal("clock initialization discarded committed timer")
	}
}
