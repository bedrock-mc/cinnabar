// Package worldfence owns the world-notice barrier independently of the transport.
package worldfence

import (
	"errors"
	"math"
	"slices"
)

// activation distinguishes a pending handshake, a running part and terminal states.
type activation uint8

const (
	pending activation = iota
	active
	disabled
	disconnected
)

// Notice binds a committed admission revision to its published wire request.
// RequestedEpoch is zero until activation publishes the notice.
type Notice struct {
	Revision       uint64
	RequestedEpoch uint64
}

// preparation reserves one placement; initial placement needs no travel notice.
type preparation struct {
	id     uint64
	notify bool
}

// Fence serializes world admissions and acknowledgments for one client part.
// Its zero value awaits activation. Callers serialize access and deliver returned
// notices in order; a failed delivery must disable the optional part.
type Fence[World comparable] struct {
	state               activation
	epoch               uint64
	world               World
	revision            uint64
	preparationID       uint64
	preparing           *preparation
	notices             []Notice
	lastRequest         uint64
	acknowledgedRequest uint64
	changed             bool
}

// Prepare reserves one placement and returns a token that cannot be reused.
// Disabled parts still track host placements without producing notices.
func (f *Fence[World]) Prepare(notify bool) (uint64, error) {
	if f.state == disconnected || f.preparing != nil || f.preparationID == math.MaxUint64 {
		return 0, errors.New("world preparation unavailable")
	}
	f.preparationID++
	f.preparing = &preparation{id: f.preparationID, notify: notify}
	return f.preparationID, nil
}

// Commit consumes exactly the matching preparation and appends its admission notice.
func (f *Fence[World]) Commit(token uint64, world World) ([]Notice, error) {
	if f.preparing == nil || token != f.preparing.id {
		return nil, errors.New("world preparation is no longer current")
	}
	notices, err := f.commit(world, f.preparing.notify)
	f.preparing = nil
	return notices, err
}

// Preparing reports whether token still owns the in-progress placement.
func (f *Fence[World]) Preparing(token uint64) bool {
	return f.preparing != nil && f.preparing.id == token
}

// HasPreparation reports whether an uncommitted placement currently blocks traffic.
func (f *Fence[World]) HasPreparation() bool { return f.preparing != nil }

// Change commits an unprepared active transfer; repeated world IDs are idempotent.
func (f *Fence[World]) Change(world World) ([]Notice, error) {
	var empty World
	if f.state != active || f.preparing != nil || world == empty {
		return nil, errors.New("world change unavailable")
	}
	if world == f.world {
		return nil, nil
	}
	return f.commit(world, true)
}

// commit records one valid placement and publishes its notice when already active.
func (f *Fence[World]) commit(world World, notify bool) ([]Notice, error) {
	var empty World
	if world == empty || f.revision == math.MaxUint64 {
		return nil, errors.New("invalid world admission or exhausted revisions")
	}
	f.revision++
	f.world = world
	if notify && f.state != disabled {
		f.notices = append(f.notices, Notice{Revision: f.revision})
		f.changed = false
	}
	return f.publish()
}

// Abort discards only the matching preparation, preserving every committed notice.
func (f *Fence[World]) Abort(token uint64) {
	if f.preparing != nil && f.preparing.id == token {
		f.preparing = nil
	}
}

// RequiresNotices reports whether activation needs world-transfer support.
func (f *Fence[World]) RequiresNotices() bool {
	return len(f.notices) != 0 || f.preparing != nil && f.preparing.notify
}

// Activate installs Ready's acknowledged epoch and publishes all queued notices,
// including when a different placement is currently being prepared.
func (f *Fence[World]) Activate(epoch uint64) ([]Notice, error) {
	if f.state != pending {
		return nil, errors.New("client part cannot activate")
	}
	f.state, f.epoch = active, epoch
	return f.publish()
}

// publish assigns increasing request epochs to every unpublished committed notice.
func (f *Fence[World]) publish() ([]Notice, error) {
	if f.state != active {
		return nil, nil
	}
	var published []Notice
	for i := range f.notices {
		if f.notices[i].RequestedEpoch != 0 {
			continue
		}
		next := max(f.epoch, f.lastRequest)
		if next == math.MaxUint64 {
			return nil, errors.New("world epoch exhausted")
		}
		f.lastRequest = next + 1
		f.notices[i].RequestedEpoch = f.lastRequest
		published = append(published, f.notices[i])
	}
	return published, nil
}

// Outstanding reports whether an acknowledgment names a published queued notice.
func (f *Fence[World]) Outstanding(request uint64) bool {
	return request != 0 && slices.ContainsFunc(f.notices, func(n Notice) bool { return n.RequestedEpoch == request })
}

// Acknowledge removes notices through the named revision and advances the epoch.
// Replayed acknowledgments do not change state or release later admissions.
func (f *Fence[World]) Acknowledge(request, epoch uint64) error {
	if f.state != active || request == 0 || request > f.lastRequest || epoch < request {
		return errors.New("acknowledgment for an invalid world notice")
	}
	if request <= f.acknowledgedRequest {
		return nil
	}
	i := slices.IndexFunc(f.notices, func(n Notice) bool { return n.RequestedEpoch == request })
	if i < 0 || epoch <= f.epoch {
		return errors.New("acknowledgment for an unknown notice or old epoch")
	}
	f.notices = slices.Delete(f.notices, 0, i+1)
	f.acknowledgedRequest, f.epoch, f.changed = request, epoch, true
	return nil
}

// Advance records a native epoch change without acknowledging any server notice.
func (f *Fence[World]) Advance(epoch uint64) error {
	if f.state != active || epoch <= f.epoch {
		return errors.New("world epoch did not advance")
	}
	f.epoch, f.changed = epoch, true
	return nil
}

// Epoch returns the client's current acknowledged epoch.
func (f *Fence[World]) Epoch() uint64 { return f.epoch }

// AllowsTraffic requires activation, no preparation and no unacknowledged notice.
func (f *Fence[World]) AllowsTraffic() bool {
	return f.state == active && f.preparing == nil && len(f.notices) == 0
}

// Accepts checks application authority against the acknowledged epoch and barrier.
func (f *Fence[World]) Accepts(epoch uint64) bool {
	return f.AllowsTraffic() && epoch == f.epoch
}

// TakeChanged consumes an epoch callback only once every barrier is open.
func (f *Fence[World]) TakeChanged() bool {
	if !f.changed || !f.AllowsTraffic() {
		return false
	}
	f.changed = false
	return true
}

// Disable ends optional authority but lets an existing host placement finish.
func (f *Fence[World]) Disable() {
	if f.state != disconnected {
		f.state, f.notices, f.changed = disabled, nil, false
	}
}

// Disconnect destroys all authority, queued notices and preparation tokens.
func (f *Fence[World]) Disconnect() { *f = Fence[World]{state: disconnected} }
