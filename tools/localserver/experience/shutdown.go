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
	h.mu.Lock()
	running := h.running && !h.stopping
	if !running && !h.stopping {
		h.stopping = true
		close(h.stop)
	}
	h.mu.Unlock()
	if running {
		for _, d := range h.dispatchers {
			barrier := make(chan struct{})
			select {
			case d.events <- event{barrier: barrier}:
				select {
				case <-barrier:
				case <-ctx.Done():
					errs = append(errs, ctx.Err())
				}
			case <-ctx.Done():
				errs = append(errs, ctx.Err())
			}
		}
	} else {
		// Startup may be stopped before Run gets its first turn. Own its queues here.
		for _, d := range h.dispatchers {
			for len(d.events) != 0 && ctx.Err() == nil {
				h.dispatch(ctx, d, <-d.events)
			}
		}
	}
	h.stopWorkers()
	errs = append(errs, ctx.Err())
	return errors.Join(errors.Join(errs...), h.store.Flush())
}

// stopAdmission waits for queue producers already admitted before closing the gate.
func (h *Host) stopAdmission() {
	h.admission.Lock()
	h.closed.Store(true)
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
