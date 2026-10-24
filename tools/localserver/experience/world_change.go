package experience

import (
	"errors"
	"maps"
	"math"
	"slices"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// changeWorld owns admission for coordinator moves, sampled native swaps and login placement.
// relocate calls admit inside the destination transaction, or inside the source transaction on recovery.
// Transport callbacks only enqueue work, so preparation never waits for a world transaction.
func (h *Host) changeWorld(handle *world.EntityHandle, mode cinnabarext.WorldChangeKind, relocate func(admit func(*player.Player), before *PlayerLocation) error) error {
	h.players.mu.Lock()
	state := h.players.items[handle]
	if state == nil || state.transferring && !state.retiring || state.location.Epoch == math.MaxUint64 || mode == cinnabarext.CoordinatedChange && (state.retiring || h.closed.Load()) {
		h.players.mu.Unlock()
		return errStale
	}
	before := state.location
	state.transferring = true
	h.players.mu.Unlock()

	var prepared *cinnabarext.WorldChange
	var err error
	if state.connection != nil {
		prepared, err = state.connection.PrepareWorldChange(mode)
	}
	defer prepared.Abort()
	completed := false
	defer func() {
		if !completed {
			h.players.mu.Lock()
			if h.players.items[handle] == state && state.location.Epoch == before.Epoch {
				state.transferring = false
			}
			h.players.mu.Unlock()
		}
	}()
	if err != nil {
		return err
	}
	h.flushFocus(handle.UUID())
	var publication error
	err = relocate(func(p *player.Player) {
		h.players.mu.Lock()
		if completed || h.players.items[handle] != state || state.location.Epoch != before.Epoch {
			h.players.mu.Unlock()
			publication = errStale
			return
		}
		h.players.mu.Unlock()
		instance, ok := h.worlds.of(p.Tx().World())
		if !ok {
			publication = ErrStaleWorld
			return
		}
		// Dragonfly synchronizes changed world pointers; same-world admission needs an explicit teleport.
		if mode == cinnabarext.CoordinatedChange && before.World == instance.handle || mode == cinnabarext.InitialPlacement {
			h.syncPlayerPosition(p, state, positionOf(p.Position()))
		}
		after := PlayerLocation{World: instance.handle, Epoch: before.Epoch + 1, Position: positionOf(p.Position())}
		h.players.mu.Lock()
		state.location, state.w, state.publishing = after, p.Tx().World(), true
		observation := PlayerEvent{Type: "world_change", Before: &before, After: &after}
		if mode == cinnabarext.InitialPlacement {
			observation = PlayerEvent{Type: "join", Location: &after}
		}
		h.observeLocked(p, instance, observation)
		h.players.mu.Unlock()
		completed = true
		notified := prepared.CommitAt(after.World.ID, after.Epoch)
		if h.worldChanged != nil {
			notified = h.worldChanged(handle.UUID(), after.World, after.Epoch) && notified
		}
		h.players.mu.Lock()
		state.transferring, state.publishing = !notified, false
		pending := slices.Sorted(maps.Keys(state.pendingEpochs))
		state.pendingEpochs = nil
		h.players.mu.Unlock()
		if notified {
			for _, exp := range pending {
				h.DeliverEpoch(handle, exp)
			}
		} else {
			publication = errors.New("destination admitted, client-part world notice failed; callbacks remain fenced")
		}
	}, &before)
	return errors.Join(err, publication)
}

// syncPlayerPosition marks the coordinator's teleport so the handler does not start another admission.
func (h *Host) syncPlayerPosition(p *player.Player, state *playerState, pos Position) {
	h.players.mu.Lock()
	state.synchronizing = true
	h.players.mu.Unlock()
	defer func() { h.players.mu.Lock(); state.synchronizing = false; h.players.mu.Unlock() }()
	p.Teleport(pos.vec())
}

// TeleportPlayer routes a native authoritative position change through admission.
// Call from HandleTeleport and cancel its outer teleport when this returns true.
func (h *Host) TeleportPlayer(p *player.Player, pos Position) bool {
	h.players.mu.Lock()
	state := h.players.items[p.H()]
	if state == nil || state.synchronizing {
		h.players.mu.Unlock()
		return false
	}
	if state.retiring || state.transferring {
		h.players.mu.Unlock()
		return true
	}
	h.players.mu.Unlock()
	err := h.changeWorld(p.H(), cinnabarext.NativeChange, func(admit func(*player.Player), _ *PlayerLocation) error {
		h.syncPlayerPosition(p, state, pos)
		admit(p)
		return nil
	})
	if err != nil {
		h.log.Error("native teleport admission failed", "error", err)
	}
	return true
}
