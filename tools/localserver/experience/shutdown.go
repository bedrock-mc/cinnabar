package experience

import (
	"context"
	"errors"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// Drain stops admission, delivers each session's final leave and drains callbacks before flushing.
// Call with a bounded context before closing the server or any world. A timeout is reported.
func (h *Host) Drain(ctx context.Context) error {
	h.shutdownMu.Lock()
	defer h.shutdownMu.Unlock()
	h.stopAdmission()
	h.Pause(false)
	var errs []error
	h.players.mu.Lock()
	handles := make([]*world.EntityHandle, 0, len(h.players.items))
	for handle := range h.players.items {
		handles = append(handles, handle)
	}
	h.players.mu.Unlock()
	// Entity tasks follow an in-flight transfer to its committed or restored world.
	for _, handle := range handles {
		if err := await(ctx, handle.Do(func(_ *world.Tx, e world.Entity) { h.PlayerDisconnected(e.(*player.Player)) })); err != nil && !errors.Is(err, world.ErrEntityClosed) {
			errs = append(errs, err)
		}
	}
	h.players.mu.Lock()
	leaves := h.players.leaves
	h.players.leaves = nil
	h.players.mu.Unlock()
	h.mu.Lock()
	running := h.running && !h.stopping
	if !running && !h.stopping {
		h.stopping = true
		close(h.stop)
	}
	h.mu.Unlock()
	if running {
		for _, d := range h.dispatchers {
			if err := h.waitDispatcher(ctx, d); err != nil {
				errs = append(errs, err)
				break
			}
		}
	}
	h.stopWorkers()
	for _, d := range h.dispatchers {
		if err := h.drainDispatcher(ctx, d, leaves); err != nil {
			errs = append(errs, err)
			break
		}
	}
	errs = append(errs, ctx.Err())
	return errors.Join(errors.Join(errs...), h.store.Flush())
}

// stopAdmission waits for queue producers already admitted before closing the gate.
func (h *Host) stopAdmission() {
	h.admission.Lock()
	h.players.mu.Lock()
	h.closed.Store(true)
	h.players.mu.Unlock()
	h.admission.Unlock()
}

// stopWorkers prevents Run from starting and waits until no callback can write the store.
func (h *Host) stopWorkers() {
	h.mu.Lock()
	if !h.stopping {
		h.stopping = true
		close(h.stop)
	}
	h.mu.Unlock()
	h.workers.Wait()
}

// drainDispatcher drains a stopped worker's queue and commits retained final leaves on the caller.
func (h *Host) drainDispatcher(ctx context.Context, d *dispatcher, leaves []event) error {
	for len(d.events) != 0 && ctx.Err() == nil {
		ev := <-d.events
		if ev.barrier != nil {
			close(ev.barrier)
		} else {
			h.dispatch(ctx, d, ev)
		}
	}
	if d.sup.registered.Worlds != nil {
		for _, leave := range leaves {
			if err := ctx.Err(); err != nil {
				return err
			}
			h.dispatch(ctx, d, leave)
		}
	}
	return ctx.Err()
}

// waitDispatcher waits for admitted callbacks or yields ownership when Run's workers stop.
func (h *Host) waitDispatcher(ctx context.Context, d *dispatcher) error {
	barrier := make(chan struct{})
	select {
	case d.events <- event{barrier: barrier}:
	case <-h.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
	select {
	case <-barrier:
		return nil
	case <-h.done:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
