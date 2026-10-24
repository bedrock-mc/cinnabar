package experience

import (
	"sync/atomic"
	"time"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
)

// ClientChannels is the server half of the client parts (tools/localserver/cinnabarext): the
// typed channels that each Experience's client part declares, and delivery to the client part
// that a player runs. Its methods may be called from any goroutine.
type ClientChannels interface {
	// PlayerSession pins the accepted transport for the lifetime of one player login.
	PlayerSession(player uuid.UUID) cinnabarext.PlayerSession
	// Declares reports whether the client part of the Experience exp declares channel, revision
	// schema, in the direction to the client.
	Declares(exp, channel string, schema uint16) bool
	// SendRecord sends payload on channel, revision schema, to the client part of exp that player
	// runs, and reports whether it went out: false when the player has no active client part, or
	// when the message does not fit the channel or the connection's limits.
	SendRecord(player uuid.UUID, exp, channel string, schema uint16, payload []cinnabarext.Scalar) bool
	// SendControl retains a negotiation reply until credit refills or its connection or epoch ends.
	SendControl(player uuid.UUID, message cinnabarext.Message) error
}

// DeliverClientMessage queues the client-message callback of the Experience exp for payload,
// which player's client part sent on channel, revision schema, without blocking. The callback
// runs in the world that player is in when its turn comes, with no snapshot, and commits like any
// other. It reports false when the message was dropped: the Host does not run exp, has closed,
// or the Experience's queue is full.
func (h *Host) DeliverClientMessage(player *world.EntityHandle, exp, channel string, schema uint16, payload []cinnabarext.Scalar) bool {
	h.admission.RLock()
	defer h.admission.RUnlock()
	d, ok := h.dispatchers[exp]
	if !ok || h.closed.Load() {
		return false
	}
	epoch, admitted := h.admitPlayerWork(player)
	if !admitted {
		return false
	}
	call := &ClientMessageCall{
		Player:  player.UUID().String(),
		Channel: channel,
		Schema:  schema,
		// The protocol has a list here, never null, and the caller keeps its slice.
		Payload: append(make([]cinnabarext.Scalar, 0, len(payload)), payload...),
	}
	select {
	case d.events <- event{actor: player, epoch: epoch, call: Call{ClientMessage: call}}:
		return true
	default:
		h.drop(d)
		return false
	}
}

// DeliverEpoch queues the epoch callback of the Experience exp, whose client part that player
// runs moved to a new world epoch and kept running, without blocking, so the Experience can
// resend its state. Like a client message, the callback runs in the world that player is in when
// its turn comes, with no snapshot, and commits like any other. It reports false when the epoch
// was dropped: the Host does not run exp, has closed, or the Experience's queue is full.
func (h *Host) DeliverEpoch(player *world.EntityHandle, exp string) bool {
	h.admission.RLock()
	defer h.admission.RUnlock()
	d, ok := h.dispatchers[exp]
	if !ok || h.closed.Load() {
		return false
	}
	if h.deferEpoch(player, exp) {
		return true
	}
	epoch, admitted := h.admitPlayerWork(player)
	if !admitted {
		return false
	}
	call := &EpochCall{Player: player.UUID().String()}
	select {
	case d.events <- event{actor: player, epoch: epoch, call: Call{Epoch: call}}:
		return true
	default:
		h.drop(d)
		return false
	}
}

// clientControl adapts a staged reply to the transport's existing typed control queue.
type clientControl struct {
	bundle string
	op     *SendClientOp
}

// Route binds the reply to the committing Experience's declared channel.
func (c clientControl) Route() cinnabarext.Route {
	return cinnabarext.Route{Bundle: c.bundle, Channel: c.op.Channel, Schema: c.op.Schema}
}

// Encode supplies the committed record; SendControl validates and owns its retained copy.
func (c clientControl) Encode() (cinnabarext.Record, error) { return c.op.Payload, nil }

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

// sendCurrent binds deferred output to its original session and authoritative admission epoch.
func (h *Host) sendCurrent(d *dispatcher, ev event, op *SendClientOp) {
	if ev.actor == nil {
		return
	}
	h.players.mu.Lock()
	state := h.players.items[ev.actor]
	h.players.mu.Unlock()

	if !h.playerCurrent(ev) {
		return
	}
	if h.channels != nil && !h.channels.Declares(d.id, op.Channel, op.Schema) {
		if n, ok := d.undeclared.add(); ok {
			h.log.Warn("client messages on undeclared channels dropped", "experience", d.id,
				"channel", op.Channel, "schema", op.Schema, "dropped", n)
		}
		return
	}
	sent := false
	if state != nil && state.connection != nil {
		if op.Control {
			sent = state.connection.SendControlAt(ev.epoch, clientControl{d.id, op}) == nil
		} else {
			sent = state.connection.SendRecordAt(ev.epoch, d.id, op.Channel, op.Schema, op.Payload)
		}
	} else if h.channels != nil {
		if op.Control {
			sent = h.channels.SendControl(ev.actor.UUID(), clientControl{d.id, op}) == nil
		} else {
			sent = h.channels.SendRecord(ev.actor.UUID(), d.id, op.Channel, op.Schema, op.Payload)
		}
	}
	if !sent {
		if n, ok := d.unsent.add(); ok {
			h.log.Debug("client messages not sent", "experience", d.id, "dropped", n)
		}
	}
}

// admitPlayerWork captures admission and its epoch together, before any possible departure.
func (h *Host) admitPlayerWork(handle *world.EntityHandle) (uint64, bool) {
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	state := h.players.items[handle]
	if state == nil {
		return 0, true
	}
	return state.location.Epoch, !state.retiring && !state.transferring
}

// deferEpoch retains control acknowledgments received during destination publication.
func (h *Host) deferEpoch(handle *world.EntityHandle, exp string) bool {
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	state := h.players.items[handle]
	if state == nil || state.retiring || !state.publishing {
		return false
	}
	if state.pendingEpochs == nil {
		state.pendingEpochs = make(map[string]bool)
	}
	state.pendingEpochs[exp] = true
	return true
}
