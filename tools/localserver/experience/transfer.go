package experience

import (
	"context"
	"errors"
	"fmt"
	"maps"
	"math"
	"slices"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

// commitLifecycle validates the source snapshot and runs one command outside world transactions.
func (h *Host) commitLifecycle(ctx context.Context, d *dispatcher, ev event, snap snapshot, ops []Op) error {
	if len(ops) != 1 || ops[0].World == nil {
		return fmt.Errorf("%w: world operations require an exclusive result", errInvalid)
	}
	h.manager.mu.Lock()
	defer h.manager.mu.Unlock()
	if h.closed.Load() {
		return errClosed
	}
	var result error
	if err := await(ctx, ev.w.Do(func(tx *world.Tx) { _, result = h.current(tx, d.id, ev, snap) })); err != nil {
		return err
	}
	if result != nil {
		return result
	}
	action := ops[0].World.Action
	switch action.Type {
	case "create":
		if action.Create == nil {
			return errInvalid
		}
		_, err := h.createWorld(ctx, d.id, *action.Create)
		return err
	case "unload":
		return h.unloadWorld(ctx, d.id, action.Instance)
	case "transfer":
		if ev.actor == nil || action.Player != ev.actor.UUID().String() {
			return errInvalid
		}
		return h.transfer(ctx, d.id, ev, snap, action.Instance, action.Position)
	}
	return errInvalid
}

// transfer prepares the destination before fencing and detaching, then admits exactly once.
// It never waits in a world transaction. After detach, cancellation completes source recovery.
func (h *Host) transfer(ctx context.Context, owner string, ev event, snap snapshot, destination WorldHandle, pos Position) error {
	if !ownedWorldName(string(destination.ID), owner) {
		return errors.New("destination authority denied")
	}
	w, err := h.ResolveWorld(destination)
	if err != nil {
		return err
	}
	for _, coordinate := range []float64{pos.X, pos.Y, pos.Z} {
		if math.IsNaN(coordinate) || math.IsInf(coordinate, 0) {
			return errors.New("nonfinite destination")
		}
	}
	if err := await(ctx, w.Do(func(tx *world.Tx) {
		anchor := cube.PosFromVec3(pos.vec())
		anchor[1] = w.Range().Min()
		tx.Block(anchor)
	})); err != nil {
		return err
	}
	h.players.mu.Lock()
	state := h.players.items[ev.actor]
	h.players.mu.Unlock()
	if state == nil {
		return errStale
	}
	state.sends.Lock()
	defer state.sends.Unlock()
	var prepared *cinnabarext.WorldChange
	if h.channels != nil {
		prepared, err = h.channels.PrepareWorldChange(ev.actor.UUID())
		if err != nil {
			return err
		}
	}
	defer prepared.Abort()
	var before PlayerLocation
	detached := false
	var result error
	err = await(ctx, ev.w.Do(func(tx *world.Tx) {
		actor, currentErr := h.current(tx, owner, ev, snap)
		if currentErr != nil {
			result = currentErr
			return
		}
		if actor == nil {
			result = errStale
			return
		}
		h.players.mu.Lock()
		defer h.players.mu.Unlock()
		state := h.players.items[ev.actor]
		if state == nil || state.transferring || state.location.Epoch == math.MaxUint64 {
			result = errStale
			return
		}
		before = state.location
		before.Position = positionOf(actor.Position())
		state.location.Epoch++
		state.transferring = true
		h.flushFocus(ev.actor.UUID())
		tx.RemoveEntity(actor)
		detached = true
	}))
	if !detached {
		return errors.Join(err, result)
	}
	if err != nil || ctx.Err() != nil {
		return h.restorePlayer(ev, before, errors.Join(err, ctx.Err()))
	}
	if h.manager.admit != nil {
		if err := h.manager.admit(); err != nil {
			return h.restorePlayer(ev, before, err)
		}
	}
	admitted := false
	err = await(ctx, w.Do(func(tx *world.Tx) {
		if ev.actor.Closed() {
			result = world.ErrEntityClosed
			return
		}
		tx.AddEntityAt(ev.actor, pos.vec())
		admitted = true

		h.players.mu.Lock()
		state := h.players.items[ev.actor]
		after := PlayerLocation{World: destination, Epoch: state.location.Epoch, Position: pos}
		state.location, state.w, state.publishing = after, w, true
		h.players.mu.Unlock()
		// Publish through the bridge without holding a registry lock across external code.
		notified := prepared.Commit(destination.ID)
		if h.worldChanged != nil {
			notified = h.worldChanged(ev.actor.UUID(), destination, after.Epoch) && notified
		}
		h.players.mu.Lock()
		state.transferring, state.publishing = !notified, false
		pending := slices.Sorted(maps.Keys(state.pendingEpochs))
		state.pendingEpochs = nil
		instance, _ := h.worlds.of(w)
		h.observeLocked(ev.actor, instance, w, PlayerEvent{Type: "world_change", Before: &before, After: &after})
		h.players.mu.Unlock()
		if notified {
			for _, exp := range pending {
				h.DeliverEpoch(ev.actor, exp)
			}
		} else {
			result = errors.New("destination admitted, client-part world notice failed; callbacks remain fenced")
		}

	}))
	if !admitted {
		return h.restorePlayer(ev, before, errors.Join(err, result))
	}
	// Cancellation after admission cannot roll a committed location back or move it twice.
	return result
}

// restorePlayer completes a failed detached transfer even after its caller cancels.
func (h *Host) restorePlayer(ev event, before PlayerLocation, cause error) error {
	if !ev.actor.Closed() {
		err := await(context.Background(), ev.w.Do(func(tx *world.Tx) {
			if ev.actor.Closed() {
				return
			}
			tx.AddEntityAt(ev.actor, before.Position.vec())
			h.players.mu.Lock()
			defer h.players.mu.Unlock()
			if state := h.players.items[ev.actor]; state != nil {
				// Keep the new epoch so old callbacks remain stale after restoration.
				before.Epoch = state.location.Epoch
				state.location, state.w, state.transferring = before, ev.w, false
			}
		}))
		if err == nil && !ev.actor.Closed() {
			return cause
		}
		cause = errors.Join(cause, err)
		if !ev.actor.Closed() {
			_ = ev.actor.Close()
		}
	}
	h.players.mu.Lock()
	delete(h.players.items, ev.actor)
	h.players.mu.Unlock()
	return errors.Join(cause, world.ErrEntityClosed)
}
