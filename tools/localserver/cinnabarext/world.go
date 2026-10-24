package cinnabarext

import (
	"errors"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext/internal/worldfence"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// WorldID is the application's stable world-instance identity, independent of dimension type.
type WorldID string

// WorldChanged requests a fresh client epoch after a committed world transfer, including
// transfers between worlds of the same dimension type. Duplicate world IDs are idempotent.
// Every committed transfer queues a notice; sends remain unavailable until OnEpoch reports
// acknowledgment of the latest admission. Call from the destination world transaction after
// moving the player. Notices bypass data credits.
// False means there is no usable v2 session or the world ID is empty.
func (s *Server) WorldChanged(player uuid.UUID, worldID WorldID) bool {
	s.mu.Lock()
	conn := s.connections[player]
	s.mu.Unlock()
	if conn == nil || worldID == "" {
		return false
	}
	conn.mu.Lock()
	defer conn.mu.Unlock()
	if conn.phase != active || conn.wire.Version == WireVersion || conn.fence.HasPreparation() {
		return false
	}
	if uint64(s.now().Unix()) >= conn.expires {
		conn.fallBack(errors.New("the grant expired"))
		return false
	}
	notices, err := conn.fence.Change(worldID)
	if err != nil {
		conn.fallBack(err)
		return false
	}
	if err := conn.publishWorldNotices(notices); err != nil {
		conn.fallBack(err)
		return false
	}
	return true
}

// activateWorldFence publishes all earlier admissions even during a later preparation.
// The caller holds transferMu and c.mu.
func (c *Conn) activateWorldFence(epoch uint64) error {
	if c.wire.Version == WireVersion && c.fence.RequiresNotices() {
		return errors.New("world transfers require client-part wire v2")
	}
	notices, err := c.fence.Activate(epoch)
	if err != nil {
		return err
	}
	return c.publishWorldNotices(notices)
}

// publishWorldNotices writes committed notices in admission order without data credits.
// The caller holds c.mu; the machine has already established every corresponding fence.
func (c *Conn) publishWorldNotices(notices []worldfence.Notice) error {
	if len(notices) != 0 {
		clear(c.controls)
	}
	for _, notice := range notices {
		control := Control{Epoch: &Epoch{Session: c.session, WorldEpoch: notice.RequestedEpoch}}
		data, err := Encode(control)
		if err != nil {
			return err
		}
		if err := c.egress.ChargeClass(control.RateClass(ToClient, false), []int{len(data)}, uint64(c.s.now().UnixMilli())); err != nil {
			return err
		}
		if err := c.Conn.WritePacket(&packet.ScriptMessage{Identifier: Carrier, Data: data}); err != nil {
			return err
		}
	}
	return nil
}
