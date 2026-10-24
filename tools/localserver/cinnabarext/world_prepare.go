package cinnabarext

import (
	"errors"
	"maps"
	"slices"

	"github.com/google/uuid"
)

// PlayerSession owns a single login's transport, world fence and authoritative send epoch.
// A host pins it at login so overlapping reconnects cannot inherit each other's authority.
type PlayerSession interface {
	// PrepareWorldChange fences this login before its next authoritative placement.
	PrepareWorldChange(kind WorldChangeKind) (*WorldChange, error)
	// SendRecordAt rejects output captured before a different host epoch was admitted.
	SendRecordAt(epoch uint64, exp, channel string, schema uint16, payload []Scalar) bool
	// Retire discards this login's connection authority and pending transfers.
	Retire()
}

// WorldChange reserves a world change in one accepted session, including late activation.
// A nil value remains a no-op for hosts without client parts.
type WorldChange struct {
	conn  *Conn
	kind  WorldChangeKind
	token uint64
}

// WorldChangeKind distinguishes initial placement from requested travel and native moves.
type WorldChangeKind uint8

const (
	CoordinatedChange WorldChangeKind = iota
	NativeChange
	InitialPlacement
)

// PrepareWorldChange reserves the current accepted session for standalone transport users.
func (s *Server) PrepareWorldChange(player uuid.UUID) (*WorldChange, error) {
	session := s.PlayerSession(player)
	if session == nil {
		return nil, nil
	}
	return session.PrepareWorldChange(CoordinatedChange)
}

// PrepareWorldChange drains earlier callbacks and fences this session before admission.
// Native moves are mandatory, so an incompatible optional part falls back instead of vetoing them.
func (c *Conn) PrepareWorldChange(kind WorldChangeKind) (*WorldChange, error) {
	c.s.transferMu.Lock()
	defer c.s.transferMu.Unlock()
	c.deliveryMu.Lock()
	defer c.deliveryMu.Unlock()
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.retired {
		if kind == NativeChange {
			return nil, nil
		}
		return nil, errors.New("player session ended")
	}
	if c.phase == active && uint64(c.s.now().Unix()) >= c.expires {
		c.fallBack(errors.New("the grant expired"))
	}
	if c.phase == active && c.wire.Version == WireVersion && kind != InitialPlacement {
		if kind != NativeChange {
			return nil, errors.New("world transfers require client-part wire v2")
		}
		c.fallBack(errors.New("native world change requires client-part wire v2"))
	}
	token, err := c.fence.Prepare(kind != InitialPlacement)
	if err != nil {
		return nil, err
	}
	return &WorldChange{conn: c, kind: kind, token: token}, nil
}

// Commit publishes a standalone transport change with a fresh authoritative epoch.
func (w *WorldChange) Commit(id WorldID) bool {
	if w == nil || w.conn == nil {
		return true
	}
	w.conn.mu.Lock()
	epoch := w.conn.hostEpoch + 1
	w.conn.mu.Unlock()
	return w.CommitAt(id, epoch)
}

// CommitAt publishes the admitted host epoch exactly once; acknowledgments release application traffic.
func (w *WorldChange) CommitAt(id WorldID, epoch uint64) bool {
	if w == nil || w.conn == nil {
		return true
	}
	c := w.conn
	c.s.transferMu.Lock()
	defer c.s.transferMu.Unlock()
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.retired || !c.fence.Preparing(w.token) || epoch == 0 {
		return false
	}
	c.hostEpoch = epoch
	notices, err := c.fence.Commit(w.token, id)
	if err != nil {
		c.fallBack(err)
		return false
	}
	if w.kind != InitialPlacement && c.phase == active && uint64(c.s.now().Unix()) >= c.expires {
		c.fallBack(errors.New("the grant expired"))
		return false
	}
	if err := c.publishWorldNotices(notices); err != nil {
		c.fallBack(err)
		return false
	}
	return true
}

// Disable ends only this connection's optional client part after publication fails.
// Ordinary player placement can continue; a new login must establish fresh authority.
func (w *WorldChange) Disable() {
	if w == nil || w.conn == nil {
		return
	}
	c := w.conn
	c.s.transferMu.Lock()
	defer c.s.transferMu.Unlock()
	c.deliveryMu.Lock()
	defer c.deliveryMu.Unlock()
	c.mu.Lock()
	defer c.mu.Unlock()
	c.fallBack(errors.New("world notice publication failed"))
}

// Abort releases only this preparation and preserves any earlier world-notice fence.
func (w *WorldChange) Abort() {
	if w == nil || w.conn == nil {
		return
	}
	c := w.conn
	c.s.transferMu.Lock()
	defer c.s.transferMu.Unlock()
	c.deliveryMu.Lock()
	defer c.deliveryMu.Unlock()
	c.mu.Lock()
	if !c.fence.Preparing(w.token) {
		c.mu.Unlock()
		return
	}
	c.fence.Abort(w.token)
	changed := c.takeEpochCallbacks()
	c.mu.Unlock()
	c.deliverEpochCallbacks(changed)
}

// takeEpochCallbacks consumes a completed acknowledgment only after every world fence opens.
// The caller holds c.mu and serializes application delivery.
func (c *Conn) takeEpochCallbacks() []string {
	if c.phase != active || !c.fence.TakeChanged() {
		return nil
	}
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
			c.s.epoch(c.player, c, exp)
		}
	}
}
