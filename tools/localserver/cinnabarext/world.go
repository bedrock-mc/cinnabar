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
// the destination world transaction after moving the player. Notices bypass data credits.
// False means there is no usable v2 session or the world ID is empty.
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
		return true
	}
	next := max(conn.epoch, conn.requestedEpoch) + 1
	if next == 0 {
		conn.fallBack(errors.New("world epoch exhausted"))
		return false
	}
	// Establish the fence before writing; acknowledgments take the same connection lock.
	conn.requestedEpoch, conn.worldID, conn.worldNoticePending = next, worldID, true
	data, err := Encode(Control{Epoch: &Epoch{Session: conn.session, WorldEpoch: next}})
	if err != nil {
		conn.fallBack(err)
		return false
	}
	if err := conn.Conn.WritePacket(&packet.ScriptMessage{Identifier: Carrier, Data: data}); err != nil {
		conn.fallBack(err)
		return false
	}
	return true
}
