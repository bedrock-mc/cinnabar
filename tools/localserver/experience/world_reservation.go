package experience

import (
	"errors"
	"sync"

	"github.com/df-mc/dragonfly/server/world"
)

// worldReservation retains one opening while its holder can still admit a player.
type worldReservation struct {
	bindings *worldBindings
	handle   WorldHandle
	once     sync.Once
}

// reserve acquires a holder atomically with resolving its opening, before unload can invalidate it.
func (b *worldBindings) reserve(handle WorldHandle) (*worldReservation, *world.World, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	w := b.byID[handle.ID]
	if w == nil || b.byWorld[w].handle != handle {
		return nil, nil, ErrStaleWorld
	}
	if b.reservations == nil {
		b.reservations = make(map[WorldHandle]uint64)
	}
	b.reservations[handle]++
	return &worldReservation{bindings: b, handle: handle}, w, nil
}

// Release drops exactly this holder; an old release cannot affect a reopened instance.
func (r *worldReservation) Release() {
	if r == nil {
		return
	}
	r.once.Do(func() {
		b := r.bindings
		b.mu.Lock()
		defer b.mu.Unlock()
		if b.reservations[r.handle] <= 1 {
			delete(b.reservations, r.handle)
		} else {
			b.reservations[r.handle]--
		}
	})
}

// unload invalidates an unreserved opening atomically with the last holder check.
// The caller has already checked physical membership inside its world transaction.
func (b *worldBindings) unload(handle WorldHandle) error {
	b.mu.Lock()
	defer b.mu.Unlock()
	w := b.byID[handle.ID]
	if w == nil || b.byWorld[w].handle != handle {
		return ErrStaleWorld
	}
	if b.reservations[handle] != 0 {
		return errors.New("world has admission reservations")
	}
	delete(b.byID, handle.ID)
	delete(b.byWorld, w)
	return nil
}
