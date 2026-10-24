package cinnabarext

import (
	"errors"
	"maps"
	"slices"

	"github.com/google/uuid"
)

// worldTransferState keeps a player's preparation and latest committed instance under transferMu.
type worldTransferState struct {
	pending *WorldChange
	id      WorldID
}

// WorldChange fences every connection that activates before this transfer commits.
// A nil value remains a no-op for hosts without client parts.
type WorldChange struct {
	server *Server
	player uuid.UUID
	conns  []*Conn
}

// worldState returns the player's coordinator state. The caller holds transferMu.
func (s *Server) worldState(player uuid.UUID) *worldTransferState {
	if s.worlds == nil {
		s.worlds = make(map[uuid.UUID]*worldTransferState)
	}
	state := s.worlds[player]
	if state == nil {
		state = &worldTransferState{}
		s.worlds[player] = state
	}
	return state
}

// PrepareWorldChange drains earlier application callbacks and reserves a fence even before Ready.
// Call outside world transactions and packet callbacks. Abort restores the source on failure.
func (s *Server) PrepareWorldChange(player uuid.UUID) (*WorldChange, error) {
	s.transferMu.Lock()
	defer s.transferMu.Unlock()
	state := s.worldState(player)
	if state.pending != nil {
		return nil, errors.New("a world transfer is already prepared")
	}
	change := &WorldChange{server: s, player: player}
	s.mu.Lock()
	c := s.active[player]
	s.mu.Unlock()
	if c != nil {
		c.deliveryMu.Lock()
		defer c.deliveryMu.Unlock()
		c.mu.Lock()
		defer c.mu.Unlock()
		if c.phase == active && uint64(s.now().Unix()) >= c.expires {
			c.fallBack(errors.New("the grant expired"))
		}
		if c.phase == active {
			if c.wire.Version == WireVersion {
				return nil, errors.New("world transfers require client-part wire v2")
			}
			c.prepared = change
			change.conns = append(change.conns, c)
		}
	}
	state.pending = change
	return change, nil
}

// Commit publishes the destination exactly once and initializes subsequent handshakes there.
// No application data passes until its correlated acknowledgment.
func (w *WorldChange) Commit(id WorldID) bool {
	if w == nil || w.server == nil {
		return true
	}
	s := w.server
	s.transferMu.Lock()
	defer s.transferMu.Unlock()
	state := s.worldState(w.player)
	if state.pending != w {
		return false
	}
	state.pending, state.id = nil, id
	notified := id != ""
	for _, c := range w.conns {
		c.mu.Lock()
		ok := c.commitWorld(w, id)
		c.mu.Unlock()
		notified = ok && notified
	}
	return notified
}

// commitWorld publishes a prepared connection's desired instance while holding c.mu.
func (c *Conn) commitWorld(w *WorldChange, id WorldID) bool {
	if c.prepared != w {
		return false
	}
	c.prepared, c.epochChanged = nil, false
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

// Abort releases only this preparation and preserves any earlier world-notice fence.
func (w *WorldChange) Abort() {
	if w == nil || w.server == nil {
		return
	}
	s := w.server
	s.transferMu.Lock()
	defer s.transferMu.Unlock()
	state := s.worldState(w.player)
	if state.pending != w {
		return
	}
	state.pending = nil
	for _, c := range w.conns {
		c.deliveryMu.Lock()
		c.mu.Lock()
		if c.prepared == w {
			c.prepared = nil
		}
		changed := c.takeEpochCallbacks()
		c.mu.Unlock()
		c.deliverEpochCallbacks(changed)
		c.deliveryMu.Unlock()
	}
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
