package cinnabarext

import (
	"errors"
	"maps"
	"slices"

	"github.com/google/uuid"
)

// WorldChange pins a connection and fences its application traffic through a server transfer.
// A nil or zero value represents a player without an active client part.
type WorldChange struct{ conn *Conn }

// PrepareWorldChange drains earlier application callbacks and fences both message directions.
// Call outside world transactions and packet callbacks. Commit after destination admission;
// Abort restores the previous fence when admission fails. Active v1 parts cannot travel.
func (s *Server) PrepareWorldChange(player uuid.UUID) (*WorldChange, error) {
	s.mu.Lock()
	c := s.active[player]
	s.mu.Unlock()
	if c == nil {
		return nil, nil
	}
	c.deliveryMu.Lock()
	defer c.deliveryMu.Unlock()
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.phase != active {
		return nil, nil
	}
	if uint64(s.now().Unix()) >= c.expires {
		c.fallBack(errors.New("the grant expired"))
		return nil, nil
	}
	if c.wire.Version == WireVersion {
		return nil, errors.New("world transfers require client-part wire v2")
	}
	if c.prepared != nil {
		return nil, errors.New("a world transfer is already prepared")
	}
	change := &WorldChange{conn: c}
	c.prepared = change
	return change, nil
}

// Commit publishes a world notice through the existing acknowledgment contract exactly once.
// No application data passes until its acknowledgment; a failed publication stays unavailable.
func (w *WorldChange) Commit(id WorldID) bool {
	if w == nil || w.conn == nil {
		return true
	}
	c := w.conn
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.prepared != w {
		return false
	}
	c.prepared = nil
	c.epochChanged = false
	if c.phase != active {
		return false
	}
	if id == "" {
		c.fallBack(errors.New("empty destination world identity"))
		return false
	}
	if uint64(c.s.now().Unix()) >= c.expires {
		c.fallBack(errors.New("the grant expired"))
		return false
	}
	c.worldID = id
	if c.worldNoticePending {
		return true
	}
	if err := c.requestWorldEpoch(); err != nil {
		c.fallBack(err)
		return false
	}
	return true
}

// Abort releases only this preparation and keeps any earlier world-notice fence intact.
func (w *WorldChange) Abort() {
	if w == nil || w.conn == nil {
		return
	}
	c := w.conn
	c.deliveryMu.Lock()
	defer c.deliveryMu.Unlock()
	c.mu.Lock()
	if c.prepared != w {
		c.mu.Unlock()
		return
	}
	c.prepared = nil
	changed := c.takeEpochCallbacks()
	c.mu.Unlock()
	c.deliverEpochCallbacks(changed)
}

// takeEpochCallbacks consumes a completed acknowledgment only after every world fence opens.
// The caller holds c.mu and serializes application delivery.
func (c *Conn) takeEpochCallbacks() []string {
	if !c.epochChanged || c.prepared != nil || c.worldNoticePending || c.phase != active {
		return nil
	}
	c.epochChanged = false
	var changed []string
	for _, exp := range slices.Sorted(maps.Keys(c.grant.Recipients)) {
		if c.grant.Recipients[exp].Permissions.Has(PermissionMessaging) {
			changed = append(changed, exp)
		}
	}
	return changed
}

// deliverEpochCallbacks forwards acknowledged state requests without holding the connection lock.
func (c *Conn) deliverEpochCallbacks(changed []string) {
	if c.s.epoch != nil {
		for _, exp := range changed {
			c.s.epoch(c.player, exp)
		}
	}
}
