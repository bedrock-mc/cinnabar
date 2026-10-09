package experience

import (
	"context"
	"fmt"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// The probe's lamp: a cube with the facing trait and one bool state.
const (
	probeLamp = "probe:lamp"
	// probeStates places a lamp above the block, lights it and reports both reads of its
	// states and the outcome of every other 0.5 call.
	probeStates = 24
	// probeToggle toggles the lamp it runs on and tells its states.
	probeToggle = 25
)

// lampStates is the text the probe tells for a lamp's states.
func lampStates(facing string, on bool) string {
	if on {
		return "minecraft:facing_direction=" + facing + ",probe:on=true"
	}
	return "minecraft:facing_direction=" + facing + ",probe:on=false"
}

// blockAt reads the Experience block at pos.
func (f *hostFixture) blockAt(pos BlockPos) Block {
	f.t.Helper()
	var b Block
	f.do(func(tx *world.Tx) {
		got, ok := tx.Block(pos.cube()).(Block)
		if !ok {
			f.t.Fatalf("%v holds %v, not an Experience block", pos, tx.Block(pos.cube()))
		}
		b = got
	})
	return b
}

// A guest's set-block-state crosses the runtime and the adapter: the lamp the probe places reads
// its default states, and the one it lights is lit in the world after the commit.
func TestBlockStatesRoundTrip(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeStates)
	f.place(probeCounter, at, nil)
	f.activate(at)
	f.waitTells(5*time.Second, "states "+lampStates("down", false)+" set ok states "+
		lampStates("down", true)+" network ok inventory ok set-slot ok drop-item ok")
	up := BlockPos{X: at.X, Y: at.Y + 1, Z: at.Z}
	lamp := f.blockAt(up)
	if lamp.t.id != probeLamp || jsonOf(lamp.states()) != jsonOf(BlockStates{
		{Name: "minecraft:facing_direction", Value: choiceValue("down")},
		{Name: "probe:on", Value: boolValue(true)},
	}) {
		t.Fatalf("the block above is %s with %s", lamp.t.id, jsonOf(lamp.states()))
	}
}

// A state change keeps the block's data and its generation in the store, and the next snapshot
// carries the new states: toggling twice turns the lamp on and off.
func TestSetBlockStateKeepsDataAndGeneration(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	at := probePos(probeToggle)
	f.place(probeLamp, at, []byte{7})
	before, _ := f.store.Token("probe", overworldKey(at))
	f.activate(at)
	f.waitTells(5*time.Second, "lamp "+lampStates("down", true))
	f.activate(at)
	f.waitTells(5*time.Second, "lamp "+lampStates("down", true), "lamp "+lampStates("down", false))
	if lamp := f.blockAt(at); lamp.t.id != probeLamp || lamp.s != 0 {
		t.Fatalf("the lamp is %s in state %d, want %s in its default", lamp.t.id, lamp.s, probeLamp)
	}
	f.assertData("probe", at, []byte{7})
	if after, _ := f.store.Token("probe", overworldKey(at)); after.Generation != before.Generation {
		t.Fatalf("generation %d became %d", before.Generation, after.Generation)
	}
}

// A player's placement sets the placement trait's state by the vanilla rule: two blocks away,
// the lamp faces the way the player looks.
func TestPlacementSetsTraitStates(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	lamp, _ := registered(t).Lookup(probeLamp)
	at := BlockPos{X: 2, Y: 100, Z: 40}
	var used bool
	var why string
	f.do(func(tx *world.Tx) {
		p := f.player(tx)
		p.Teleport(mgl64.Vec3{0.5, 100, 40.5})
		p.Move(mgl64.Vec3{}, 90, 0)
		used = f.host.useOnBlock(lamp, at.cube(), cube.FaceUp, mgl64.Vec3{0.5, 1, 0.5}, tx, p, &item.UseContext{})
		why = fmt.Sprintf("player at %v, %v; block there %v", p.Position(), p.Rotation(), tx.Block(at.cube()))
	})
	if !used {
		t.Fatalf("the lamp was not placed: %s", why)
	}
	f.waitTells(5*time.Second, "placed")
	placed := f.blockAt(at)
	if got := jsonOf(placed.states()[0]); got != jsonOf(BlockState{Name: "minecraft:facing_direction", Value: choiceValue("west")}) {
		t.Fatalf("placed lamp's first state = %s, want facing west", got)
	}
}

// The commit check refuses a state op the runtime should never have staged: a state the block
// lacks, a value it does not take, a block that is not its own, or one outside the write scope.
func TestCommitRefusesInvalidStateOps(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	at := probePos(probeToggle)
	f.place(probeLamp, at, nil)
	f.place(probeCounter, BlockPos{X: at.X, Y: at.Y + 1}, nil)
	d := f.host.dispatchers["probe"]
	ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: at.cube(), call: Call{
		Neighbor: &NeighborCall{Pos: at, Neighbor: at},
	}}
	snap, err := f.host.snapshot(context.Background(), d, &ev)
	if err != nil {
		t.Fatalf("snapshot: %v", err)
	}
	on := BlockState{Name: "probe:on", Value: boolValue(true)}
	for _, c := range []struct {
		op   SetBlockStateOp
		want string
	}{
		{SetBlockStateOp{Pos: at, States: BlockStates{{Name: "probe:dim", Value: boolValue(true)}}}, "probe:dim"},
		{SetBlockStateOp{Pos: at, States: BlockStates{{Name: "probe:on", Value: choiceValue("yes")}}}, "probe:on"},
		{SetBlockStateOp{Pos: at, States: BlockStates{on, on}}, "twice"},
		{SetBlockStateOp{Pos: at}, "no states"},
		{SetBlockStateOp{Pos: BlockPos{X: at.X, Y: at.Y + 1}, States: BlockStates{on}}, "probe:on"},
		{SetBlockStateOp{Pos: BlockPos{X: at.X + 1, Y: at.Y}, States: BlockStates{on}}, "not its own"},
		{SetBlockStateOp{Pos: BlockPos{X: at.X, Y: at.Y + 2}, States: BlockStates{on}}, "outside the write scope"},
	} {
		op := c.op
		f.host.commit(context.Background(), d, ev, snap, []Op{{SetBlockState: &op}})
		waitForRecord(t, logs, func(r map[string]any) bool {
			err, _ := r["error"].(string)
			return r["msg"] == "invalid result discarded" && strings.Contains(err, c.want)
		})
	}
	if lamp := f.blockAt(at); lamp.s != 0 {
		t.Fatalf("an invalid op changed the lamp to state %d", lamp.s)
	}
	f.host.commit(context.Background(), d, ev, snap, []Op{{SetBlockState: &SetBlockStateOp{Pos: at, States: BlockStates{on}}}})
	if lamp := f.blockAt(at); jsonOf(lamp.states()[1]) != jsonOf(on) {
		t.Fatalf("the valid op left the lamp %s", jsonOf(lamp.states()))
	}
}

// A foreign change to a block's states makes a result computed on it stale.
func TestForeignStateChangeIsStale(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	at := probePos(probeToggle)
	f.place(probeLamp, at, nil)
	d := f.host.dispatchers["probe"]
	ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: at.cube(), call: Call{
		Neighbor: &NeighborCall{Pos: at, Neighbor: at},
	}}
	snap, err := f.host.snapshot(context.Background(), d, &ev)
	if err != nil {
		t.Fatalf("snapshot: %v", err)
	}
	lit, err := f.blockAt(at).withStates(BlockStates{{Name: "probe:on", Value: boolValue(true)}})
	if err != nil {
		t.Fatal(err)
	}
	f.do(func(tx *world.Tx) { tx.SetBlock(at.cube(), lit, nil) })
	data := "01"
	f.host.commit(context.Background(), d, ev, snap, []Op{{SetBlockData: &SetBlockDataOp{Pos: at, Data: &data}}})
	waitStale(t, logs, "changed")
	f.assertData("probe", at, nil)
}
