package experience

import (
	"context"
	"errors"
	"math"
	"slices"
	"strings"
	"sync"
	"time"

	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// playerState tracks one session; a departure fences work even when it returns to the same world.
type playerState struct {
	sends         *sync.Mutex
	location      PlayerLocation
	w             *world.World
	transferring  bool
	publishing    bool
	pendingEpochs map[string]bool
	lastTick      uint64
}

// playerLocations owns session epochs and ordered lifecycle observations.
type playerLocations struct {
	mu    sync.Mutex
	items map[*world.EntityHandle]*playerState
}

// positionOf converts a position read under the player's world transaction.
func positionOf(p mgl64.Vec3) Position { return Position{X: p[0], Y: p[1], Z: p[2]} }

// vec converts an already validated protocol position to Dragonfly coordinates.
func (p Position) vec() mgl64.Vec3 { return mgl64.Vec3{p.X, p.Y, p.Z} }

// PlayerJoined admits a session and queues join before any movement observations. Call in its transaction.
func (h *Host) PlayerJoined(p *player.Player) {
	instance, ok := h.worlds.of(p.Tx().World())
	if !ok || h.closed.Load() {
		return
	}
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	if h.players.items == nil {
		h.players.items = make(map[*world.EntityHandle]*playerState)
	}
	if _, exists := h.players.items[p.H()]; exists {
		return
	}
	loc := PlayerLocation{World: instance.handle, Epoch: 1, Position: positionOf(p.Position())}
	h.players.items[p.H()] = &playerState{sends: new(sync.Mutex), location: loc, w: p.Tx().World()}
	h.observeLocked(p, instance, PlayerEvent{Type: "join", Location: &loc})
}

// PlayerDisconnected removes only this session; an overlapping reconnect keeps its own authority.
func (h *Host) PlayerDisconnected(p *player.Player) {
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	state := h.players.items[p.H()]
	if state == nil {
		return
	}
	h.flushFocus(p.UUID())
	instance, ok := h.worlds.of(p.Tx().World())
	if !ok {
		delete(h.players.items, p.H())
		return
	}
	h.reconcilePlayerLocked(p, instance, state, false)
	loc := state.location
	h.observeLocked(p, instance, PlayerEvent{Type: "leave", Location: &loc})
	delete(h.players.items, p.H())
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
	h.players.mu.Lock()
	defer h.players.mu.Unlock()
	state := h.players.items[ev.actor]
	if state == nil {
		return ev.epoch == 0
	}
	return !state.transferring && state.location.Epoch == ev.epoch
}

// observeLocked captures immutable observations and inventory in the owning transaction.
// Callers hold players.mu; leave events retain no live inventory authority.
func (h *Host) observeLocked(p *player.Player, instance worldInstance, observation PlayerEvent) {
	handle, w := p.H(), p.Tx().World()
	loc := observation.Location
	if loc == nil {
		loc = observation.After
	}
	for _, d := range h.dispatchers {
		if d.sup.registered.Worlds == nil || h.closed.Load() {
			continue
		}
		ev := event{w: w, instance: instance, epoch: loc.Epoch, actor: handle, call: Call{Player: &PlayerCall{Player: handle.UUID().String(), Event: observation}}}
		if observation.Type == "leave" {
			ev.actor = nil
		} else {
			ev.inventory, ev.slots = inventoryOf(d.id, p)
			ev.entities = h.entitySnapshot(p.Tx(), d.id)
		}
		select {
		case d.events <- ev:
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
			defer h.players.mu.Unlock()
			state := h.players.items[handle]
			instance, ok := h.worlds.of(tx.World())
			if state == nil || state.transferring || !ok || state.lastTick >= tick {
				return
			}
			state.lastTick = tick
			h.reconcilePlayerLocked(e.(*player.Player), instance, state, true)
		})); err != nil && !errors.Is(err, world.ErrEntityClosed) {
			return err
		}
	}
	return nil
}

// reconcilePlayerLocked records the transaction's current location and any unsampled world change.
// Callers hold players.mu. Disconnects update position without adding a separate move observation.
func (h *Host) reconcilePlayerLocked(p *player.Player, instance worldInstance, state *playerState, movement bool) {
	before := state.location
	after := before
	after.Position = positionOf(p.Position())
	after.World = instance.handle
	kind := "move"
	if before.World != after.World {
		if after.Epoch == math.MaxUint64 {
			return
		}
		after.Epoch++
		kind = "world_change"
		h.flushFocus(p.UUID())
	}
	state.location, state.w = after, p.Tx().World()
	if before != after && (movement || kind == "world_change") {
		h.observeLocked(p, instance, PlayerEvent{Type: kind, Before: &before, After: &after})
	}
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
	context.Entities = h.entitySnapshot(tx, d.id)
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
	context.Entities = ev.entities
	if actor != nil {
		copy := *loc
		context.Player = &copy
	}
	var simulation *SimulationContext
	if d.sup.registered.Simulation {
		simulation = h.simulationContext(d.id, ev, d.seq)
	}
	return snapshot{req: CallbackRequest{Info: Info{WorldID: h.worldID, DimensionID: ev.instance.id, EventSequence: d.seq, Context: context, Simulation: simulation}, Actor: actor, DataBudget: h.store.Budget(d.id), Snapshot: []Cell{}, Call: ev.call, Inventory: ev.inventory}, slots: ev.slots}
}

// storePrincipal retains an observed player's private record after its live actor leaves.
func (ev event) storePrincipal(actor *string) *string {
	if ev.call.Player != nil {
		return &ev.call.Player.Player
	}
	return actor
}
