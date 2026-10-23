package cinnabarext

import (
	"errors"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// WorldID is the application's stable world-instance identity, independent of dimension type.
type WorldID string

// WorldChanged requests a fresh client epoch after a committed world transfer, including
// transfers between worlds of the same dimension type. Duplicate world IDs are idempotent.
// Sends remain unavailable until OnEpoch reports the client's acknowledgment. Call from
// the destination world transaction after moving the player. False means no notice was sent;
// an active v2 session stays fenced and retries once after one second of rate refill.
func (s *Server) WorldChanged(player uuid.UUID, worldID WorldID) bool {
	s.mu.Lock()
	conn := s.active[player]
	s.mu.Unlock()
	if conn == nil || worldID == "" {
		return false
	}
	conn.mu.Lock()
	defer conn.mu.Unlock()
	if uint64(s.now().Unix()) >= conn.expires {
		conn.fallBack(errors.New("the grant expired"))
		return false
	}
	if conn.phase != active || conn.wire.Version == WireVersion {
		return false
	}
	if conn.worldID == worldID {
		return conn.requestedEpoch == 0 || conn.sendWorldNotice()
	}
	next := max(conn.epoch, conn.requestedEpoch) + 1
	if next == 0 {
		conn.fallBack(errors.New("world epoch exhausted"))
		return false
	}
	conn.requestedEpoch, conn.worldID, conn.worldNoticeSent = next, worldID, false
	if conn.sendWorldNotice() {
		return true
	}
	if conn.phase == active && !conn.worldRetryPending {
		conn.worldRetryPending = true
		s.deferWorldNotice(conn.retryWorldNotice)
	}
	return false
}

// sendWorldNotice admits the pending notice once, without reopening either direction.
// The caller holds c.mu. Only rate exhaustion leaves the active notice unsent.
func (c *Conn) sendWorldNotice() bool {
	if c.worldNoticeSent {
		return true
	}
	now := c.s.now()
	if uint64(now.Unix()) >= c.expires {
		c.fallBack(errors.New("the grant expired"))
		return false
	}
	data, err := Encode(Control{Epoch: &Epoch{Session: c.session, WorldEpoch: c.requestedEpoch}})
	if err != nil {
		c.fallBack(err)
		return false
	}
	if c.egress.Charge(len(data), uint64(now.UnixMilli())) != nil {
		return false
	}
	if err := c.Conn.WritePacket(&packet.ScriptMessage{Identifier: Carrier, Data: data}); err != nil {
		c.fallBack(err)
		return false
	}
	c.worldNoticeSent = true
	return true
}

// retryWorldNotice delivers the latest unsent notice after a full refill or ends the client part.
func (c *Conn) retryWorldNotice() {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.worldRetryPending = false
	if c.phase == active && c.requestedEpoch != 0 && !c.sendWorldNotice() {
		c.fallBack(errors.New("world notice could not be delivered after rate refill"))
	}
}
