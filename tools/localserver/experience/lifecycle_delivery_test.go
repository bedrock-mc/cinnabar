package experience

import (
	"context"
	"errors"
	"fmt"
	"io"
	"sync"
	"testing"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// heldResponse withholds a helper response until cancellation closes its pipe.
type heldResponse struct {
	io.ReadCloser
	started  chan struct{}
	released chan struct{}
	once     sync.Once
}

// Read announces a response in flight, then waits for the worker to cancel it.
func (r *heldResponse) Read(data []byte) (int, error) {
	n, err := r.ReadCloser.Read(data)
	r.once.Do(func() { close(r.started) })
	<-r.released
	return n, err
}

// Close releases the held read and closes the owned pipe.
func (r *heldResponse) Close() error {
	close(r.released)
	return r.ReadCloser.Close()
}

// TestLifecycleInterruptedLeave replays a dequeued leave before draining later callbacks.
func TestLifecycleInterruptedLeave(t *testing.T) {
	for _, strikes := range []int{0, strikeLimit - 1} {
		for _, point := range []string{"awaiting_response", "before_commit"} {
			t.Run(fmt.Sprintf("strikes=%d/%s", strikes, point), func(t *testing.T) {
				m := newLifecycleMatrix(t)
				f := m.f
				<-f.d.events // The initial join has no writes.
				m.step("leave")
				for range strikes {
					f.d.sup.strike()
				}
				if point == "awaiting_response" {
					m.cancelCallback()
				} else {
					ctx, cancel := context.WithCancel(t.Context())
					defer cancel()
					f.h.afterCall = func() { f.h.Pause(true); cancel() }
					f.h.Run(ctx)
					f.h.afterCall = nil
				}
				if f.d.sup.Quarantined() {
					t.Fatal("host cancellation quarantined the Experience")
				}
				m.step("shutdown")
				m.check()
			})
		}
	}
}

// cancelCallback interrupts the first Run during helper I/O, retaining its event for Drain.
// A host whose one worker run has already ended has no callback left to interrupt.
func (m *lifecycleMatrix) cancelCallback() {
	f := m.f
	f.h.mu.Lock()
	used := f.h.running || f.h.closing || f.h.stopping
	f.h.mu.Unlock()
	if used {
		return
	}
	ctx, cancel := context.WithCancel(f.t.Context())
	defer cancel()
	response := &heldResponse{ReadCloser: f.d.sup.proc.stdout, started: make(chan struct{}), released: make(chan struct{})}
	f.d.sup.proc.stdout = response
	done := make(chan struct{})
	go func() { defer close(done); f.h.Run(ctx) }()
	select {
	case <-response.started:
		cancel()
		<-done
	case <-done:
		f.t.Fatal("Run stopped before callback I/O")
	}
}

// TestLifecycleUndeliveredLeave refuses a successful drain when a mod cannot receive its leave.
func TestLifecycleUndeliveredLeave(t *testing.T) {
	m := newLifecycleMatrix(t)
	m.step("leave")
	m.f.d.sup.quarantined.Store(true)
	if err := m.f.h.Drain(t.Context()); err == nil {
		t.Fatal("drain succeeded without delivering the final leave")
	}
}

// TestLifecycleCancelledDrain retains leaves for a new drain context after cancellation.
func TestLifecycleCancelledDrain(t *testing.T) {
	m := newLifecycleMatrix(t)
	m.leaves++ // The first drain owns this session's final leave.
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if err := m.f.h.Drain(ctx); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled drain = %v", err)
	}
	m.drain()
}

// TestLifecyclePlayerEventTicks preserves each observation's world tick through delayed delivery.
func TestLifecyclePlayerEventTicks(t *testing.T) {
	m := newLifecycleMatrix(t)
	f := m.f
	<-f.d.events
	for range 3 {
		f.source.AdvanceTick()
	}
	destination, _ := f.h.worlds.handle("probe:two")
	target, err := f.h.ResolveWorld(destination)
	if err != nil {
		t.Fatal(err)
	}
	for range 7 {
		target.AdvanceTick()
	}
	for _, step := range []struct {
		name, kind string
		w          *world.World
	}{
		{"leave", "leave", f.source},
		{"join", "join", f.source},
		{"move", "move", f.source},
		{"portal", "world_change", target},
		{"leave", "leave", target},
	} {
		t.Run(step.name+"/"+step.kind, func(t *testing.T) {
			var want uint64
			f.do(step.w, func(tx *world.Tx) { want = uint64(tx.CurrentTick()) })
			if step.name == "move" {
				f.do(f.source, func(tx *world.Tx) {
					e, _ := f.actor.Entity(tx)
					e.(*player.Player).Move(mgl64.Vec3{2, 0, 3}, 0, 0)
				})
				m.tick++
				if err := f.h.PollPlayers(t.Context(), m.tick); err != nil {
					t.Fatal(err)
				}
			} else {
				m.step(step.name)
			}
			ev := <-f.d.events
			if ev.call.Player.Event.Type != step.kind {
				t.Fatalf("event = %s, want %s", ev.call.Player.Event.Type, step.kind)
			}
			step.w.AdvanceTick()
			snap, err := f.h.snapshot(t.Context(), f.d, &ev)
			if err != nil {
				t.Fatal(err)
			}
			if snap.req.Info.Tick != want {
				t.Fatalf("%s tick = %d, want queued tick %d", step.kind, snap.req.Info.Tick, want)
			}
			f.h.dispatch(t.Context(), f.d, ev)
		})
	}
	m.drain()
}
