package experience

import (
	"context"
	"errors"
	"fmt"
	"math"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/player"
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
	reservation, w, err := h.worlds.reserve(destination)
	if err != nil {
		return err
	}
	defer reservation.Release()
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
	return h.changeWorld(ev.actor, cinnabarext.CoordinatedChange, func(admit func(*player.Player), before *PlayerLocation) error {
		detached := false
		var result error
		err := await(ctx, ev.w.Do(func(tx *world.Tx) {
			// The admission fence is ours; validate the captured callback without reopening it.
			h.players.mu.Lock()
			state := h.players.items[ev.actor]
			current := state != nil && state.location.Epoch == ev.epoch && !h.closed.Load()
			h.players.mu.Unlock()
			if !current {
				result = errStale
				return
			}
			actor, currentErr := h.currentWorld(tx, owner, ev, snap)
			if currentErr != nil || actor == nil {
				result = errors.Join(errStale, currentErr)
				return
			}
			before.Position = positionOf(actor.Position())
			tx.RemoveEntity(actor)
			detached = true
		}))
		if !detached {
			return errors.Join(err, result)
		}
		if err != nil || ctx.Err() != nil {
			return h.restorePlayer(ev, *before, admit, errors.Join(err, ctx.Err()))
		}
		if h.manager.admit != nil {
			if err := h.manager.admit(); err != nil {
				return h.restorePlayer(ev, *before, admit, err)
			}
		}
		admitted := false
		err = await(ctx, w.Do(func(tx *world.Tx) {
			if ev.actor.Closed() {
				result = world.ErrEntityClosed
				return
			}
			actor := tx.AddEntityAt(ev.actor, pos.vec()).(*player.Player)
			admitted = true
			admit(actor)
		}))
		if !admitted {
			return h.restorePlayer(ev, *before, admit, errors.Join(err, result))
		}
		return nil
	})
}

// restorePlayer completes source admission after a detached failure, even if the caller canceled.
func (h *Host) restorePlayer(ev event, before PlayerLocation, admit func(*player.Player), cause error) error {
	if !ev.actor.Closed() {
		err := await(context.Background(), ev.w.Do(func(tx *world.Tx) {
			if !ev.actor.Closed() {
				admit(tx.AddEntityAt(ev.actor, before.Position.vec()).(*player.Player))
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
	state := h.players.items[ev.actor]
	delete(h.players.items, ev.actor)
	h.players.mu.Unlock()
	if state != nil {
		state.reservation.Release()
	}
	if state != nil && state.connection != nil {
		state.connection.Retire()
	}
	return errors.Join(cause, world.ErrEntityClosed)
}
