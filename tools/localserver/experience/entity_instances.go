package experience

import (
	"slices"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// modEntityData remains owned by its framework handle across transactions.
type modEntityData struct {
	state    EntityState
	instance worldInstance
	revision uint64
	loader   *world.Loader
	closed   bool
}

// modEntity is a transaction-scoped server-driven actor.
type modEntity struct {
	tx   *world.Tx
	h    *world.EntityHandle
	data *world.EntityData
	typ  *modEntityType
}

// entityConfig initializes state without replacing the framework's spawn options.
type entityConfig struct {
	state    EntityState
	instance worldInstance
}

// Apply retains the typed identity alongside the framework's position and rotation.
func (c entityConfig) Apply(data *world.EntityData) {
	data.Data = &modEntityData{state: c.state, instance: c.instance, revision: 1}
}

// stateData returns the mutable record owned by the current transaction.
func (e *modEntity) stateData() *modEntityData { return e.data.Data.(*modEntityData) }

// H exposes the normal Dragonfly handle used for actor replication.
func (e *modEntity) H() *world.EntityHandle { return e.h }

// Position returns the authoritative base position.
func (e *modEntity) Position() mgl64.Vec3 { return e.data.Pos }

// Rotation returns yaw and pitch in degrees.
func (e *modEntity) Rotation() cube.Rotation { return e.data.Rot }

// Velocity supplies interpolation velocity in blocks per tick.
func (e *modEntity) Velocity() mgl64.Vec3 { return e.data.Vel }

// NameTag returns the replicated label.
func (e *modEntity) NameTag() string { return e.stateData().state.NameTag }

// Scale returns the visual and collision scale.
func (e *modEntity) Scale() float64 { return float64(e.stateData().state.Scale) }

// Variant returns the standard actor variant metadata.
func (e *modEntity) Variant() int32 { return e.stateData().state.Variant }

// EntityProperties shares declaration order with this type's property schema.
func (e *modEntity) EntityProperties() protocol.EntityProperties {
	properties := make([]protocol.FloatEntityProperty, 0, len(e.stateData().state.Properties))
	for i, p := range e.stateData().state.Properties {
		properties = append(properties, protocol.FloatEntityProperty{Index: uint32(i), Value: p.Value})
	}
	return protocol.EntityProperties{FloatProperties: properties}
}

// snapshot copies mutable state so later world work cannot change an in-flight callback.
func (e *modEntity) snapshot() EntitySnapshot {
	state := e.stateData().state
	state.Properties = slices.Clone(state.Properties)
	state.Position = positionOf(e.Position())
	state.Rotation = EntityRotation{e.Rotation().Yaw(), e.Rotation().Pitch()}
	state.Velocity = positionOf(e.Velocity())
	riders := make([]EntityRider, 0)
	for _, link := range world.RiderLinks(e) {
		if rider, ok := link.Rider.Entity(e.tx); ok {
			if p, ok := rider.(*player.Player); ok {
				riders = append(riders, EntityRider{p.UUID().String(), uint32(link.SeatIndex)})
			}
		}
	}
	return EntitySnapshot{State: state, Revision: e.stateData().revision, Riders: riders}
}

// entitySnapshot reads only this mod's live actors in the transaction's world opening.
func (h *Host) entitySnapshot(tx *world.Tx, owner string) []EntitySnapshot {
	out := make([]EntitySnapshot, 0)
	instance, ok := h.worlds.of(tx.World())
	if !ok {
		return out
	}
	for entity := range tx.Entities() {
		if e, ok := entity.(*modEntity); ok && e.typ.owner == owner && !e.stateData().closed && e.stateData().instance.handle == instance.handle {
			out = append(out, e.snapshot())
		}
	}
	slices.SortFunc(out, func(a, b EntitySnapshot) int {
		if a.State.Handle.ID < b.State.Handle.ID {
			return -1
		}
		if a.State.Handle.ID > b.State.Handle.ID {
			return 1
		}
		return 0
	})
	return out
}

// findEntity resolves a typed identity only within its owning mod and live opening.
func (h *Host) findEntity(tx *world.Tx, owner string, handle EntityHandle) (*modEntity, bool) {
	instance, ok := h.worlds.of(tx.World())
	if !ok {
		return nil, false
	}
	for entity := range tx.Entities() {
		if e, ok := entity.(*modEntity); ok && e.typ.owner == owner && e.stateData().state.Handle == handle && e.stateData().instance.handle == instance.handle && !e.stateData().closed {
			return e, true
		}
	}
	return nil, false
}

// Tick keeps a server-driven actor's current chunk resident without simulating guest code.
func (e *modEntity) Tick(tx *world.Tx, _ int64) {
	d := e.stateData()
	if d.closed {
		return
	}
	instance, ok := e.typ.host.worlds.of(tx.World())
	if !ok || instance.handle != d.instance.handle {
		_ = e.Close()
		return
	}
	e.pin()
}

// pin retains only the actor's current chunk while its instance is loaded.
func (e *modEntity) pin() {
	d := e.stateData()
	if d.loader == nil {
		d.loader = world.NewLoader(0, e.tx.World(), world.NopViewer{})
	}
	d.loader.Move(e.tx, e.Position())
	d.loader.Load(e.tx, 1)
}

// update publishes motion and metadata through Dragonfly before moving its riders.
func (e *modEntity) update(state EntityState, teleport bool) {
	viewers := e.tx.Viewers(e.Position())
	e.data.Pos, e.data.Vel, e.data.Rot = state.Position.vec(), state.Velocity.vec(), cube.Rotation{state.Rotation.Yaw, state.Rotation.Pitch}
	e.stateData().state = state
	e.stateData().state.Properties = slices.Clone(state.Properties)
	e.stateData().revision++
	for _, v := range viewers {
		v.ViewEntityVelocity(e, e.Velocity())
		if teleport {
			v.ViewEntityTeleport(e, e.Position())
		} else {
			v.ViewEntityMovement(e, e.Position(), e.Rotation(), false)
		}
		v.ViewEntityState(e)
	}
	e.tx.UpdateRiders(e)
	e.pin()
}

// Close removes the actor, its occupied seats and its chunk lease in the owning transaction.
func (e *modEntity) Close() error {
	d := e.stateData()
	if d.closed {
		return nil
	}
	d.closed = true
	if d.loader != nil {
		d.loader.Close(e.tx)
		d.loader = nil
	}
	e.tx.RemoveEntity(e)
	e.emit(nil, EntityEvent{Type: "despawned"})
	return e.h.Close()
}

// HandleRiderChange delivers committed seat membership and advances snapshot revisions.
func (e *modEntity) HandleRiderChange(rider world.Entity, seat int, mounted bool) {
	e.stateData().revision++
	p, ok := rider.(*player.Player)
	if !ok {
		return
	}
	kind := "dismounted"
	if mounted {
		kind = "mounted"
	}
	e.emit(p.H(), EntityEvent{Type: kind, Rider: &EntityRider{p.UUID().String(), uint32(seat)}})
}

// HandleRiderInput queues typed input without running guest work on the world goroutine.
func (e *modEntity) HandleRiderInput(rider world.Entity, seat int, input world.RiderInput) {
	p, ok := rider.(*player.Player)
	if !ok {
		return
	}
	e.emit(p.H(), EntityEvent{Type: "steering", Input: &EntityInput{Rider: EntityRider{p.UUID().String(), uint32(seat)}, Sideways: float32(input.Move.X()), Forward: float32(input.Move.Y()), Yaw: input.Yaw, Pitch: input.Pitch, Jump: input.Jump, Sneak: input.Sneak}})
}

// emit preserves world and player epochs for all entity observations.
func (e *modEntity) emit(actor *world.EntityHandle, observation EntityEvent) {
	h := e.typ.host
	if h == nil {
		return
	}
	instance, ok := h.worlds.of(e.tx.World())
	if !ok || instance.handle != e.stateData().instance.handle {
		if observation.Type != "despawned" {
			return
		}
		instance = e.stateData().instance
	}
	h.enqueue(e.typ.owner, event{w: e.tx.World(), instance: instance, actor: actor, call: Call{Entity: &EntityCall{Handle: e.stateData().state.Handle, Event: observation}}})
}

// EntityInteraction delivers the existing framework's main-hand interaction or attack hook.
func (h *Host) EntityInteraction(p *player.Player, target world.Entity, attacked bool) {
	e, ok := target.(*modEntity)
	if !ok || e.typ.host != h {
		return
	}
	held, _ := p.HeldItems()
	var stack *ItemStack
	if !held.Empty() {
		value := stackOf(e.typ.owner, held)
		stack = &value
	}
	kind := "interact"
	if attacked {
		kind = "attacked"
	}
	e.emit(p.H(), EntityEvent{Type: kind, Player: p.UUID().String(), Hand: "main", Item: stack})
}

// entityDespawnSnapshot retains the removal observation after its world closes, with no live actor authority.
func (h *Host) entityDespawnSnapshot(d *dispatcher, ev event) snapshot {
	context := &WorldContext{Current: ev.instance.handle, Instances: h.worlds.handles(d.id), Stores: h.store.readStates(d.id, stateAddresses(ev.instance.handle.ID, nil))}
	return snapshot{req: CallbackRequest{Info: Info{WorldID: h.worldID, DimensionID: ev.instance.id, EventSequence: d.seq, Context: context}, DataBudget: h.store.Budget(d.id), Snapshot: []Cell{}, Call: ev.call}}
}
