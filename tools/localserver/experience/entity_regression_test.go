package experience

import (
	"context"
	"errors"
	"fmt"
	"math"
	"testing"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// TestEntitySpawnOrdinalsSurviveOtherOperations validates the guest's coalesced operation stream.
func TestEntitySpawnOrdinalsSurviveOtherOperations(t *testing.T) {
	for _, sequence := range []uint64{1, math.MaxUint64 / maxStagedOps} {
		t.Run(fmt.Sprint(sequence), func(t *testing.T) {
			f := newEntityFixture(t)
			ev, snap := f.request()
			snap.req.Info.EventSequence = sequence
			first := EntityState{Handle: EntityHandle{sequence*maxStagedOps + 1, ev.instance.handle.Generation}, Kind: "probe:ship", Position: Position{Y: 70}, Scale: 1, Properties: []EntityProperty{{ID: "probe:throttle"}}}
			second := first
			second.Handle.ID++
			ops := []Op{
				{Entity: &EntityCommand{Action: EntityOp{Type: "spawn", State: &first}}},
				{SetBlock: &SetBlockOp{Pos: BlockPos{Y: 70}, ID: airID}},
				{Entity: &EntityCommand{Action: EntityOp{Type: "despawn", Handle: first.Handle}}},
				{Entity: &EntityCommand{Action: EntityOp{Type: "spawn", State: &second}}},
			}
			f.do(ev.w, func(tx *world.Tx) {
				if err := f.h.validateEntities(tx, "probe", ev, snap, nil, ops); err != nil {
					t.Fatalf("valid coalesced entity batch rejected: %v", err)
				}
				bad := second
				bad.Handle.ID++
				ops[3].Entity.Action.State = &bad
				if err := f.h.validateEntities(tx, "probe", ev, snap, nil, ops); !errors.Is(err, errInvalid) {
					t.Fatalf("unreserved identity accepted: %v", err)
				}
				snap.req.Info.EventSequence = math.MaxUint64/maxStagedOps + 1
				if err := f.h.validateEntities(tx, "probe", ev, snap, nil, ops); !errors.Is(err, errInvalid) {
					t.Fatalf("overflowed reservation accepted: %v", err)
				}
			})
		})
	}
}

// motionViewer models the session's reset of actor motion during a teleport.
type motionViewer struct {
	world.NopViewer
	velocity mgl64.Vec3
	position mgl64.Vec3
}

// ViewEntityVelocity retains the latest motion received by a viewer.
func (v *motionViewer) ViewEntityVelocity(_ world.Entity, velocity mgl64.Vec3) { v.velocity = velocity }

// ViewEntityTeleport models the motion reset sent by a session for a teleport.
func (v *motionViewer) ViewEntityTeleport(_ world.Entity, position mgl64.Vec3) {
	v.position = position
	v.velocity = mgl64.Vec3{}
}

// ViewEntityMovement retains ordinary movement without resetting motion.
func (v *motionViewer) ViewEntityMovement(_ world.Entity, position mgl64.Vec3, _ cube.Rotation, _ bool) {
	v.position = position
}

func TestEntityTeleportPreservesRequestedVelocity(t *testing.T) {
	for _, teleport := range []bool{false, true} {
		t.Run(fmt.Sprint(teleport), func(t *testing.T) {
			f := newEntityFixture(t)
			v := &motionViewer{}
			f.do(f.source, func(tx *world.Tx) {
				loader := world.NewLoader(0, f.source, v)
				loader.Move(tx, mgl64.Vec3{0, 70, 0})
				loader.Load(tx, 1)
				t.Cleanup(func() {
					if err := f.source.Do(func(tx *world.Tx) { loader.Close(tx) }).Wait(context.Background()); err != nil {
						t.Error(err)
					}
				})
			})
			handle := f.spawnEntity()
			f.d.seq++
			ev, snap := f.request()
			state := snap.req.Info.Context.Entities[0].State
			state.Position.X = 2
			state.Velocity = Position{X: 0.25, Y: 0.5, Z: -0.125}
			f.do(ev.w, func(tx *world.Tx) {
				ops := []Op{{Entity: &EntityCommand{Action: EntityOp{Type: "update", State: &state, Teleport: teleport}}}}
				if _, err := f.h.apply(tx, "probe", ev, snap, ops); err != nil {
					t.Fatal(err)
				}
				actor, ok := f.h.findEntity(tx, "probe", handle)
				if !ok || actor.Velocity() != state.Velocity.vec() {
					t.Fatal("server lost requested velocity")
				}
			})
			if v.position != state.Position.vec() || v.velocity != state.Velocity.vec() {
				t.Fatalf("viewer position/motion = %v/%v, want %v/%v", v.position, v.velocity, state.Position.vec(), state.Velocity.vec())
			}
		})
	}
}
