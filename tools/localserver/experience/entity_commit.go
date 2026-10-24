package experience

import (
	"fmt"
	"math"
	"slices"
	"unicode"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

// validEntityState checks mutable fields against their single registered declaration.
func validEntityState(state EntityState, def EntityDefinition) bool {
	if state.Kind != def.ID || !finitePosition(state.Position) || !finitePosition(state.Velocity) || !finite(state.Rotation.Yaw) || !finite(state.Rotation.Pitch) || !finite(float64(state.Scale)) || state.Scale <= 0 || len(state.NameTag) > maxNameBytes || len(state.Properties) != len(def.Properties) {
		return false
	}
	for _, c := range state.NameTag {
		if unicode.IsControl(c) {
			return false
		}
	}
	for i, p := range state.Properties {
		d := def.Properties[i]
		if p.ID != d.ID || !finite(float64(p.Value)) || p.Value < d.Min || p.Value > d.Max {
			return false
		}
	}
	return true
}

// validateEntities simulates every actor operation and verifies revisions before any writes.
func (h *Host) validateEntities(tx *world.Tx, owner string, ev event, snap snapshot, actor world.Entity, ops []Op) error {
	has := slices.ContainsFunc(ops, func(op Op) bool { return op.Entity != nil })
	if !has {
		return nil
	}
	current := map[EntityHandle]EntitySnapshot{}
	for _, s := range h.entitySnapshot(tx, owner) {
		current[s.State.Handle] = s
	}
	staged := map[EntityHandle]EntitySnapshot{}
	if snap.req.Info.Context != nil {
		for _, s := range snap.req.Info.Context.Entities {
			staged[s.State.Handle] = s
		}
	}
	created := map[EntityHandle]bool{}
	var riding bool
	var ride EntityHandle
	if actor != nil {
		if vehicle, _, ok := world.Riding(actor); ok {
			riding = true
			if v, ok := vehicle.Entity(tx); ok {
				if e, ok := v.(*modEntity); ok && e.typ.owner == owner {
					ride = e.stateData().state.Handle
				}
			}
		}
	}
	check := func(handle EntityHandle) (EntitySnapshot, error) {
		state, ok := staged[handle]
		if !ok {
			return EntitySnapshot{}, fmt.Errorf("%w: entity outside snapshot", errInvalid)
		}
		if !created[handle] {
			now, ok := current[handle]
			if !ok || now.Revision != state.Revision {
				return EntitySnapshot{}, fmt.Errorf("%w: entity changed", errStale)
			}
		}
		return state, nil
	}
	liveCount := len(current)
	var spawnOrdinal uint64
	for _, op := range ops {
		if op.Entity == nil {
			continue
		}
		action := op.Entity.Action
		switch action.Type {
		case "spawn":
			spawnOrdinal++
			if action.State == nil || snap.req.Info.EventSequence > (math.MaxUint64-spawnOrdinal)/maxStagedOps {
				return errInvalid
			}
			state := *action.State
			typ := h.reg.entityTypes[state.Kind]
			want := snap.req.Info.EventSequence*maxStagedOps + spawnOrdinal
			if typ == nil || typ.owner != owner || !validEntityState(state, typ.def) || state.Handle != (EntityHandle{want, ev.instance.handle.Generation}) || current[state.Handle].State.Handle.ID != 0 || created[state.Handle] {
				return errInvalid
			}
			liveCount++
			if liveCount > maxEntities {
				return errInvalid
			}
			created[state.Handle] = true
			staged[state.Handle] = EntitySnapshot{State: state, Riders: []EntityRider{}}
		case "update":
			if action.State == nil {
				return errInvalid
			}
			state := *action.State
			old, err := check(state.Handle)
			if err != nil {
				return err
			}
			typ := h.reg.entityTypes[state.Kind]
			if old.State.Kind != state.Kind || typ == nil || !validEntityState(state, typ.def) {
				return errInvalid
			}
			old.State = state
			staged[state.Handle] = old
		case "despawn":
			if _, err := check(action.Handle); err != nil {
				return err
			}
			delete(staged, action.Handle)
			liveCount--
			if ride == action.Handle {
				ride = EntityHandle{}
				riding = false
			}
		case "mount":
			entity, err := check(action.Handle)
			if err != nil {
				return err
			}
			typ := h.reg.entityTypes[entity.State.Kind]
			if actor == nil || riding || typ == nil || int(action.Seat) >= len(typ.def.Seats) || slices.ContainsFunc(entity.Riders, func(r EntityRider) bool { return r.Seat == action.Seat }) {
				return errInvalid
			}
			if _, ok := world.SeatPosition(entity.State.Position.vec(), entity.State.Rotation.Yaw, typ.Seats()[action.Seat]); !ok {
				return errInvalid
			}
			entity.Riders = append(slices.Clone(entity.Riders), EntityRider{actor.H().UUID().String(), action.Seat})
			staged[action.Handle] = entity
			riding = true
			ride = action.Handle
		case "dismount":
			if actor == nil || !riding || ride.ID == 0 {
				return errInvalid
			}
			entity, err := check(ride)
			if err != nil {
				return err
			}
			entity.Riders = slices.DeleteFunc(slices.Clone(entity.Riders), func(r EntityRider) bool { return r.Player == actor.H().UUID().String() })
			staged[ride] = entity
			riding = false
			ride = EntityHandle{}
		default:
			return errInvalid
		}
	}
	return nil
}

// applyEntities executes an already validated batch in the same world transaction.
func (h *Host) applyEntities(tx *world.Tx, owner string, ev event, actor world.Entity, ops []Op) {
	for _, op := range ops {
		if op.Entity == nil {
			continue
		}
		action := op.Entity.Action
		switch action.Type {
		case "spawn":
			state := *action.State
			state.Properties = slices.Clone(state.Properties)
			typ := h.reg.entityTypes[state.Kind]
			handle := (world.EntitySpawnOpts{Position: state.Position.vec(), Rotation: cube.Rotation{state.Rotation.Yaw, state.Rotation.Pitch}, Velocity: state.Velocity.vec()}).New(typ, entityConfig{state: state, instance: ev.instance})
			tx.AddEntity(handle).(*modEntity).pin()
		case "update":
			if e, ok := h.findEntity(tx, owner, action.State.Handle); ok {
				e.update(*action.State, action.Teleport)
			}
		case "despawn":
			if e, ok := h.findEntity(tx, owner, action.Handle); ok {
				_ = e.Close()
			}
		case "mount":
			if e, ok := h.findEntity(tx, owner, action.Handle); ok {
				if !tx.Mount(actor, e, int(action.Seat)) {
					panic("validated entity mount failed")
				}
			}
		case "dismount":
			if !tx.Dismount(actor) {
				panic("validated entity dismount failed")
			}
		}
	}
}

// closeEntities invalidates guest actors before a world opening is released.
func (h *Host) closeEntities(tx *world.Tx) {
	var entities []*modEntity
	for entity := range tx.Entities() {
		if e, ok := entity.(*modEntity); ok {
			entities = append(entities, e)
		}
	}
	for _, e := range entities {
		_ = e.Close()
	}
}
