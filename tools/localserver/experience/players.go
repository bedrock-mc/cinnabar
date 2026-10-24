package experience

import (
	"context"
	"errors"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/cinnabar-mc/cinnabar/tools/localserver/cinnabarext"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// playerState tracks one session; a departure fences work even when it returns to the same world.
type playerState struct {
	connection  cinnabarext.PlayerSession
	reservation *worldReservation
	location    PlayerLocation
	w           *world.World
	// transferring lasts only for the scoped authoritative relocation, independently of client-part fences.
	transferring  bool
	retiring      bool
	synchronizing bool
	publishing    bool
	pendingEpochs map[string]bool
	lastTick      uint64
}

// playerLocations owns session epochs and ordered lifecycle observations.
type playerLocations struct {
	mu    sync.Mutex
	items map[*world.EntityHandle]*playerState
	// leaves retains final observations during shutdown, outside the bounded callback queues.
	leaves []event
}

// positionOf converts a position read under the player's world transaction.
func positionOf(p mgl64.Vec3) Position { return Position{X: p[0], Y: p[1], Z: p[2]} }

// vec converts an already validated protocol position to Dragonfly coordinates.
func (p Position) vec() mgl64.Vec3 { return mgl64.Vec3{p.X, p.Y, p.Z} }

// PlayerJoined admits a session and queues join before any movement observations. Call in its transaction.
func (h *Host) PlayerJoined(p *player.Player) {
	_, ok := h.worlds.of(p.Tx().World())
	if !ok || h.closed.Load() {
		return
	}
	h.players.mu.Lock()
	if h.closed.Load() {
		h.players.mu.Unlock()
		return
	}
	if h.players.items == nil {
		h.players.items = make(map[*world.EntityHandle]*playerState)
	}
	if _, exists := h.players.items[p.H()]; exists {
		h.players.mu.Unlock()
		return
	}
	state := &playerState{}
	if h.channels != nil {
		state.connection = h.channels.PlayerSession(p.UUID())
	}
	h.players.items[p.H()] = state
	h.players.mu.Unlock()
	if err := h.changeWorld(p.H(), cinnabarext.InitialPlacement, func(admit func(*player.Player), _ *PlayerLocation) error { admit(p); return nil }); err != nil {
		h.log.Error("player admission failed", "error", err)
	}
}

// PlayerDisconnected retires only this session, preserving an overlapping reconnect's authority.
func (h *Host) PlayerDisconnected(p *player.Player) {
	h.players.mu.Lock()
	state := h.players.items[p.H()]
	if state == nil || state.retiring {
		h.players.mu.Unlock()
		return
	}
	state.retiring = true
	h.players.mu.Unlock()
	if state.connection != nil {
		state.connection.Retire()
	}
	h.reconcilePlayer(p, false)
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	if h.players.items[p.H()] != state {
		return
	}
	h.flushFocus(p.UUID())
	if instance, ok := h.worlds.of(p.Tx().World()); ok {
		loc := state.location
		loc.Position = positionOf(p.Position())
		h.observeLocked(p, instance, PlayerEvent{Type: "leave", Location: &loc})
	}
	delete(h.players.items, p.H())
	state.reservation.Release()
}

// PlayerSession identifies the transport admitted with this handle, without resolving a newer login by UUID.
func (h *Host) PlayerSession(handle *world.EntityHandle) cinnabarext.PlayerSession {
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	if state := h.players.items[handle]; state != nil && !state.retiring {
		return state.connection
	}
	return nil
}

// epoch returns the current admission epoch, or zero for a session not admitted to this host.
func (h *Host) epoch(handle *world.EntityHandle) uint64 {
	if handle == nil {
		return 0
	}
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	if state := h.players.items[handle]; state != nil {
		return state.location.Epoch
	}
	return 0
}

// playerCurrent rejects work across departures, including a return to the original live instance.
func (h *Host) playerCurrent(ev event) bool {
	if ev.actor == nil {
		return true
	}
	if h.closed.Load() {
		return false
	}
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	state := h.players.items[ev.actor]
	if state == nil {
		return ev.epoch == 0
	}
	return !state.retiring && !state.transferring && state.location.Epoch == ev.epoch
}

// observeLocked captures immutable observations and inventory in the owning transaction.
// Callers hold players.mu; leave events retain no live inventory authority.
func (h *Host) observeLocked(p *player.Player, instance worldInstance, observation PlayerEvent) {
	handle, w := p.H(), p.Tx().World()
	loc := observation.Location
	if loc == nil {
		loc = observation.After
	}
	ev := event{w: w, instance: instance, tick: uint64(p.Tx().CurrentTick()), epoch: loc.Epoch, actor: handle, call: Call{Player: &PlayerCall{Player: handle.UUID().String(), Event: observation}}}
	if observation.Type == "leave" {
		ev.actor = nil
		if h.closed.Load() {
			h.players.leaves = append(h.players.leaves, ev)
			return
		}
	}
	for _, d := range h.dispatchers {
		if d.sup.registered.Worlds == nil || h.closed.Load() {
			continue
		}
		queued := ev
		if queued.actor != nil {
			queued.inventory, queued.slots = inventoryOf(d.id, p)
		}
		select {
		case d.events <- queued:
		default:
			// Losing a lifecycle observation would violate ordering. Suspend this mod instead.
			d.sup.quarantined.Store(true)
			h.log.Error("player event queue full; experience suspended", "experience", d.id)
		}
	}
}

// PollPlayers samples committed positions once per host tick, independent of pauseable world clocks.
func (h *Host) PollPlayers(ctx context.Context, tick uint64) error {
	h.players.mu.Lock()
	handles := make([]*world.EntityHandle, 0, len(h.players.items))
	for handle := range h.players.items {
		handles = append(handles, handle)
	}
	h.players.mu.Unlock()
	slices.SortFunc(handles, func(a, b *world.EntityHandle) int { return strings.Compare(a.UUID().String(), b.UUID().String()) })
	for _, handle := range handles {
		if err := await(ctx, handle.Do(func(tx *world.Tx, e world.Entity) {
			h.players.mu.Lock()
			state := h.players.items[handle]
			if state == nil || state.retiring || state.transferring || state.lastTick >= tick {
				h.players.mu.Unlock()
				return
			}
			state.lastTick = tick
			h.players.mu.Unlock()
			h.reconcilePlayer(e.(*player.Player), true)
		})); err != nil && !errors.Is(err, world.ErrEntityClosed) {
			return err
		}
	}
	return nil
}

// reconcilePlayer routes native pointer changes through admission and coalesces ordinary movement.
// It runs in the player's transaction without holding the observation registry across transport callbacks.
func (h *Host) reconcilePlayer(p *player.Player, movement bool) {
	instance, ok := h.worlds.of(p.Tx().World())
	if !ok {
		return
	}
	h.players.mu.Lock()
	state := h.players.items[p.H()]
	if state == nil || state.transferring && !state.retiring {
		h.players.mu.Unlock()
		return
	}
	if state.w != p.Tx().World() {
		h.players.mu.Unlock()
		if err := h.changeWorld(p.H(), cinnabarext.NativeChange, func(admit func(*player.Player), _ *PlayerLocation) error { admit(p); return nil }); err != nil {
			h.log.Error("native world admission failed", "error", err)
		}
		return
	}
	if !movement {
		h.players.mu.Unlock()
		return
	}
	before := state.location
	after := before
	after.Position = positionOf(p.Position())
	state.location = after
	if before != after {
		h.observeLocked(p, instance, PlayerEvent{Type: "move", Before: &before, After: &after})
	}
	h.players.mu.Unlock()
}

// runPlayerTicks is the clock edge for position sampling; domain tests supply tick numbers directly.
func (h *Host) runPlayerTicks(ctx context.Context) {
	ticker := time.NewTicker(tickDuration)
	defer ticker.Stop()
	var tick uint64
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			tick++
			if err := h.PollPlayers(ctx, tick); err != nil && ctx.Err() == nil {
				h.log.Debug("player observation stopped", "error", err)
			}
		}
	}
}

// worldContext reads authorized state and the actor's location under its world transaction.
func (h *Host) worldContext(tx *world.Tx, d *dispatcher, ev event, actor *string) *WorldContext {
	context := &WorldContext{Current: ev.instance.handle, Instances: h.worlds.handles(d.id), Stores: h.store.readStates(d.id, stateAddresses(ev.instance.handle.ID, ev.storePrincipal(actor)))}
	if ev.actor != nil {
		if e, ok := ev.actor.Entity(tx); ok {
			context.Player = &PlayerLocation{World: ev.instance.handle, Epoch: ev.epoch, Position: positionOf(e.Position())}
		}
	}
	return context
}

// observationSnapshot preserves event order even after a player leaves or its world unloads.
func (h *Host) observationSnapshot(d *dispatcher, ev event) snapshot {
	call := ev.call.Player
	loc := call.Event.Location
	if loc == nil {
		loc = call.Event.After
	}
	var actor *string
	if ev.actor != nil {
		id := call.Player
		actor = &id
	}
	context := &WorldContext{Current: loc.World, Instances: h.worlds.handles(d.id), Stores: h.store.readStates(d.id, stateAddresses(loc.World.ID, ev.storePrincipal(actor)))}
	if actor != nil {
		copy := *loc
		context.Player = &copy
	}
	info := newInfo(h.worldID, ev.instance.id, ev.tick, d.seq)
	info.Context = context
	return snapshot{req: CallbackRequest{Info: info, Actor: actor, DataBudget: h.store.Budget(d.id), Snapshot: []Cell{}, Call: ev.call, Inventory: ev.inventory}, slots: ev.slots}
}

// storePrincipal retains an observed player's private record after its live actor leaves.
func (ev event) storePrincipal(actor *string) *string {
	if ev.call.Player != nil {
		return &ev.call.Player.Player
	}
	return actor
}
