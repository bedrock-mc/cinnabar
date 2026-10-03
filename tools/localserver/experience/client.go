package experience

import (
	"sync/atomic"
	"time"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
)

// ClientChannels is the server half of the client parts (tools/localserver/extension): the
// typed channels that each Experience's client part declares, and delivery to the client part
// that a player runs. Its methods may be called from any goroutine.
type ClientChannels interface {
	// Declares reports whether the client part of the Experience exp declares channel, revision
	// schema, in the direction to the client.
	Declares(exp, channel string, schema uint16) bool
	// Send sends payload on channel, revision schema, to the client part of exp that player
	// runs, and reports whether it went out: false when the player has no active client part, or
	// when the message does not fit the channel or the connection's limits.
	Send(player uuid.UUID, exp, channel string, schema uint16, payload []Scalar) bool
}

// DeliverClientMessage queues the client-message callback of the Experience exp for payload,
// which player's client part sent on channel, revision schema, without blocking. The callback
// runs in the world that player is in when its turn comes, with no snapshot, and commits like any
// other. It reports false when the message was dropped: the Host does not run exp, has closed,
// or the Experience's queue is full.
func (h *Host) DeliverClientMessage(player *world.EntityHandle, exp, channel string, schema uint16, payload []Scalar) bool {
	d, ok := h.dispatchers[exp]
	if !ok || h.closed.Load() {
		return false
	}
	call := &ClientMessageCall{
		Player:  player.UUID().String(),
		Channel: channel,
		Schema:  schema,
		// The protocol has a list here, never null, and the caller keeps its slice.
		Payload: append(make([]Scalar, 0, len(payload)), payload...),
	}
	select {
	case d.events <- event{actor: player, call: Call{ClientMessage: call}}:
		return true
	default:
		h.drop(d)
		return false
	}
}

// sendClient sends a committed client message of the Experience d to player, or counts it as
// dropped: on a channel that the Experience's client part does not declare, or not sent by the
// server half, typically because player has no active client part.
func (h *Host) sendClient(d *dispatcher, player uuid.UUID, op *SendClientOp) {
	if h.channels != nil && !h.channels.Declares(d.id, op.Channel, op.Schema) {
		if n, ok := d.undeclared.add(); ok {
			h.log.Warn("client messages on undeclared channels dropped", "experience", d.id,
				"channel", op.Channel, "schema", op.Schema, "dropped", n)
		}
		return
	}
	if h.channels == nil || !h.channels.Send(player, d.id, op.Channel, op.Schema, op.Payload) {
		if n, ok := d.unsent.add(); ok {
			h.log.Debug("client messages not sent", "experience", d.id, "dropped", n)
		}
	}
}

// dropCount counts things dropped, and limits how often the count is logged.
type dropCount struct {
	n atomic.Uint64
	// lastLog is when, in Unix nanoseconds, the count was last logged.
	lastLog atomic.Int64
}

// add counts one more drop. It returns the count, and whether to log it now: at most once per
// dropLogInterval.
func (c *dropCount) add() (uint64, bool) {
	n := c.n.Add(1)
	now := time.Now().UnixNano()
	last := c.lastLog.Load()
	return n, now-last >= int64(dropLogInterval) && c.lastLog.CompareAndSwap(last, now)
}
