package experience

import (
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
)

// focus is the block of an Experience that a player last used: the last on-interact that ran for
// that player and one of the Experience's blocks. A client message or an epoch of the player gets
// its snapshot while it is valid.
type focus struct {
	// player is the player's handle in the session that used the block; a later session of the
	// same player has another.
	player *world.EntityHandle
	dim    int
	pos    cube.Pos
	// generation is the placement generation of the block that was used. Another placement at
	// pos starts a new one.
	generation uint64
}

// focus is the call's focus: that of a client message or an epoch, or nil.
func (c Call) focus() *BlockPos {
	switch {
	case c.ClientMessage != nil:
		return c.ClientMessage.Focus
	case c.Epoch != nil:
		return c.Epoch.Focus
	}
	return nil
}

// setFocus sets the focus of a client message or an epoch to pos.
func (c Call) setFocus(pos BlockPos) {
	switch {
	case c.ClientMessage != nil:
		c.ClientMessage.Focus = &pos
	case c.Epoch != nil:
		c.Epoch.Focus = &pos
	}
}

// recordFocus makes the anchor of ev, an interaction, its actor's focus, if the Experience takes
// one and snap found the anchor to be its own block. It replaces the actor's earlier focus.
func (d *dispatcher) recordFocus(ev event, snap snapshot) {
	if !d.sup.Focus() || ev.actor == nil || len(snap.cells) == 0 || !snap.cells[0].owned {
		return
	}
	anchor := snap.cells[0]
	d.focusMu.Lock()
	defer d.focusMu.Unlock()
	d.focuses[ev.actor.UUID()] = focus{
		player: ev.actor, dim: ev.dim.num, pos: anchor.pos, generation: anchor.token.Generation,
	}
}

// focusOf returns the focus of player, in the session whose handle it is.
func (d *dispatcher) focusOf(player *world.EntityHandle) (focus, bool) {
	d.focusMu.Lock()
	defer d.focusMu.Unlock()
	f, ok := d.focuses[player.UUID()]
	return f, ok && f.player == player
}

// focusOn gives ev, a client message or an epoch, its actor's focus as its anchor if the
// Experience takes one and the focus is valid in tx, the world of actor, the actor's entity: the
// block is in that world's dimension, within provisionalFocusRange of the actor's eyes, loaded,
// and still the Experience's block of the generation that was used. Otherwise ev keeps no focus.
func (h *Host) focusOn(tx *world.Tx, d *dispatcher, ev *event, actor world.Entity) {
	if !d.sup.Focus() {
		return
	}
	f, ok := d.focusOf(ev.actor)
	if !ok || f.dim != ev.dim.num {
		return
	}
	if entity.EyePosition(actor).Sub(f.pos.Vec3Centre()).Len() > provisionalFocusRange {
		return
	}
	if st := h.cellState(tx, d.id, ev.dim, f.pos); !st.owned || st.token.Generation != f.generation {
		return
	}
	ev.anchor = f.pos
	ev.call.setFocus(blockPos(f.pos))
}

// PlayerLeft forgets the focus of the player with the UUID in every Experience. The server calls
// it when that player disconnects.
func (h *Host) PlayerLeft(player uuid.UUID) {
	for _, d := range h.dispatchers {
		d.focusMu.Lock()
		delete(d.focuses, player)
		d.focusMu.Unlock()
	}
}
