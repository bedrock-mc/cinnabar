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
	for _, d := range h.dispatchers {
		if err := h.drainDispatcher(ctx, d, leaves, running); err != nil {
			errs = append(errs, err)
			break
		}
	}
	h.stopWorkers()
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

// drainDispatcher delivers retained leaves outside world tasks, then waits for their final commits.
// If Run never started, the shutdown caller owns the queue and dispatches it synchronously.
func (h *Host) drainDispatcher(ctx context.Context, d *dispatcher, leaves []event, running bool) error {
	if !running {
		for len(d.events) != 0 && ctx.Err() == nil {
			h.dispatch(ctx, d, <-d.events)
		}
	}
	if d.sup.registered.Worlds != nil {
		for _, leave := range leaves {
			if err := ctx.Err(); err != nil {
				return err
			}
			if !running {
				h.dispatch(ctx, d, leave)
				continue
			}
			select {
			case d.events <- leave:
			case <-ctx.Done():
				return ctx.Err()
			}
		}
	}
	if !running {
		return ctx.Err()
	}
	barrier := make(chan struct{})
	select {
	case d.events <- event{barrier: barrier}:
	case <-ctx.Done():
		return ctx.Err()
	}
	select {
	case <-barrier:
		return nil
	case <-ctx.Done():
		return ctx.Err()
	}
}
