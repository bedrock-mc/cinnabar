package experience

import (
	"context"
	"encoding/hex"
	"fmt"
	"math"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
)

// probeNode is the probe's network member, a plain cube; probeMark marks every member of a
// node's network with the member count and tells "network <n> truncated <bool> wrote <n>".
const (
	probeNode = "probe:node"
	probeMark = 26
)

// grid is a fake world for the flood: its members, each with data of a length, and positions
// that are not loaded. Every other position holds no member.
type grid struct {
	members  map[cube.Pos]int
	unloaded map[cube.Pos]bool
}

func (g grid) at(pos cube.Pos) (member, bool) {
	n, ok := g.members[pos]
	if !ok || g.unloaded[pos] {
		return member{}, false
	}
	return member{st: cellState{pos: pos, loaded: true, owned: true}, data: make([]byte, n)}, true
}

// line puts members along x from x0 to x1, inclusive, at y 64 and z.
func (g grid) line(x0, x1, z int) grid {
	for x := x0; x <= x1; x++ {
		g.members[cube.Pos{x, 64, z}] = 0
	}
	return g
}

func newGrid() grid {
	return grid{members: make(map[cube.Pos]int), unloaded: make(map[cube.Pos]bool)}
}

func positions(members []member) []cube.Pos {
	out := make([]cube.Pos, len(members))
	for i, m := range members {
		out[i] = m.st.pos
	}
	return out
}

// The flood takes every member reachable through face-adjacent members, in breadth-first order
// from its starts; two lines a bridge joins are one network.
func TestFloodMergesThroughMembers(t *testing.T) {
	g := newGrid().line(0, 3, 0).line(0, 3, 2)
	g.members[cube.Pos{3, 64, 1}] = 0 // the bridge
	g.members[cube.Pos{0, 66, 0}] = 0 // not adjacent: one block above the line's end
	members, truncated := flood([]cube.Pos{{0, 64, 0}}, g.at)
	if truncated || len(members) != 9 {
		t.Fatalf("flood found %v, truncated %v; want the 9 joined members", positions(members), truncated)
	}
	if members[0].st.pos != (cube.Pos{0, 64, 0}) || members[1].st.pos != (cube.Pos{1, 64, 0}) {
		t.Errorf("flood order %v starts elsewhere than its start", positions(members))
	}
}

// A break floods from the broken block's six neighbors: the two halves of a split line are both
// in the network, the broken block in neither, and a start without a member adds nothing.
func TestFloodFromABreakTakesBothHalves(t *testing.T) {
	g := newGrid().line(0, 6, 0)
	broken := cube.Pos{3, 64, 0}
	delete(g.members, broken)
	var starts []cube.Pos
	for _, f := range cube.Faces() {
		starts = append(starts, broken.Side(f))
	}
	members, truncated := flood(starts, g.at)
	got := positions(members)
	if truncated || len(got) != 6 || slices.Contains(got, broken) {
		t.Fatalf("flood found %v, truncated %v; want both halves", got, truncated)
	}
}

// The flood stops at unloaded positions, as an AE2 grid does, without marking the network
// truncated: the members beyond are not part of it.
func TestFloodStopsAtUnloadedPositions(t *testing.T) {
	g := newGrid().line(0, 6, 0)
	g.unloaded[cube.Pos{4, 64, 0}] = true
	members, truncated := flood([]cube.Pos{{0, 64, 0}}, g.at)
	if truncated || len(members) != 4 {
		t.Fatalf("flood found %v, truncated %v; want the 4 members before the unloaded one",
			positions(members), truncated)
	}
}

// The flood holds at most maxNetworkBlocks members and maxNetworkDataBytes of their data; a
// member past either bound marks the network truncated.
func TestFloodBounds(t *testing.T) {
	exact := newGrid().line(0, maxNetworkBlocks-1, 0)
	if members, truncated := flood([]cube.Pos{{0, 64, 0}}, exact.at); truncated || len(members) != maxNetworkBlocks {
		t.Fatalf("%d members, truncated %v; want all %d", len(members), truncated, maxNetworkBlocks)
	}
	over := newGrid().line(0, maxNetworkBlocks, 0)
	if members, truncated := flood([]cube.Pos{{0, 64, 0}}, over.at); !truncated || len(members) != maxNetworkBlocks {
		t.Fatalf("%d members, truncated %v; want %d and truncated", len(members), truncated, maxNetworkBlocks)
	}
	full := maxNetworkDataBytes / maxBlockDataBytes
	data := newGrid().line(0, full, 0)
	for pos := range data.members {
		data.members[pos] = maxBlockDataBytes
	}
	members, truncated := flood([]cube.Pos{{0, 64, 0}}, data.at)
	if !truncated || len(members) != full {
		t.Fatalf("%d members, truncated %v; want %d and truncated by data", len(members), truncated, full)
	}
	data.members[cube.Pos{full, 64, 0}] = 0
	if members, truncated := flood([]cube.Pos{{0, 64, 0}}, data.at); truncated || len(members) != full+1 {
		t.Fatalf("%d members, truncated %v; a member without data fits", len(members), truncated)
	}
}

// Networks fit beside the callback's inventory and neighbors, even with maximum-length states.
func TestNetworkAtItsBoundsFitsAFrame(t *testing.T) {
	for _, longStates := range []bool{false, true} {
		t.Run(fmt.Sprintf("long_states_%v", longStates), func(t *testing.T) {
			def := testBlock("probe:node", writeTexture(t, "block.png", 1, 1))
			def.Network = true
			for i := range 16 {
				name := fmt.Sprintf("%02d", i)
				if longStates {
					name += strings.Repeat("s", maxNameBytes-len("probe:")-len(name))
					def.States = append(def.States, choiceState(name, strings.Repeat("a", maxNameBytes), strings.Repeat("b", maxNameBytes)))
				} else {
					def.States = append(def.States, boolState(name))
				}
			}
			typ, err := newBlockType("probe", def, newAssetCache())
			if err != nil {
				t.Fatal(err)
			}
			snap := snapshot{req: probeInteract(probeMark)}
			for _, cell := range snap.req.Snapshot {
				snap.cells = append(snap.cells, cellState{pos: cell.Pos.cube()})
			}
			initial := len(snap.cells)
			var members []member
			for i := range maxNetworkBlocks {
				m := member{st: cellState{
					pos: cube.Pos{1000 + i, -64, math.MinInt32}, loaded: true, id: typ.id, owned: true, block: Block{t: typ, s: uint32(i % 2)},
				}}
				if i < maxNetworkDataBytes/maxBlockDataBytes {
					m.data, m.hasData = make([]byte, maxBlockDataBytes), true
				} else if i == maxNetworkDataBytes/maxBlockDataBytes {
					m.hasData = true
				}
				members = append(members, m)
			}
			itemData := hex.EncodeToString(make([]byte, maxItemDataBytes))
			snap.req.Inventory = &Inventory{Slots: make([]*ItemStack, inventorySlots)}
			for i := range snap.req.Inventory.Slots {
				snap.req.Inventory.Slots[i] = &ItemStack{ID: "probe:" + strings.Repeat("c", maxNameBytes-len("probe:")), Count: 1, MaxCount: 1, Data: &itemData}
			}
			appendNetwork(&snap, members, false)
			snap.req.Seq = math.MaxUint64 // The supervisor fills this after the snapshot is read.
			body, err := encodeFrame(Request{Callback: &snap.req})
			if err != nil {
				t.Fatalf("a request at the network bounds does not fit a frame: %v", err)
			}
			net := snap.req.Network
			if net.Truncated != longStates || len(net.Blocks) == 0 || (!longStates && len(net.Blocks) != maxNetworkBlocks) {
				t.Fatalf("%d members, truncated %v; long states %v", len(net.Blocks), net.Truncated, longStates)
			}
			if len(snap.cells) != initial+len(net.Blocks) || len(snap.req.Snapshot) != len(snap.cells) {
				t.Fatalf("snapshot and checked cells do not cover exactly the retained network")
			}
			for i, pos := range net.Blocks {
				if pos != blockPos(members[i].st.pos) || snap.req.Snapshot[initial+i].Pos != pos {
					t.Fatal("the retained network is not the breadth-first prefix")
				}
			}
			t.Logf("%d members, %d of %d bytes", len(net.Blocks), len(body), maxFrameBytes)
		})
	}
}

// placeLine places probe nodes at y 64 and z 0 from x0 to x1.
func (f *hostFixture) placeLine(id string, x0, x1 int32) {
	f.t.Helper()
	for x := x0; x <= x1; x++ {
		f.place(id, BlockPos{X: x, Y: 64}, nil)
	}
}

// A node's network crosses chunk columns: the probe's marks reach every member through the
// runtime and the adapter, the one in the next column too.
func TestNetworkRoundTrip(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	f.placeLine(probeNode, probeMark, probeMark+7)
	f.activate(probePos(probeMark))
	f.waitTells(5*time.Second, "network 8 truncated false wrote 8")
	for x := int32(probeMark); x <= probeMark+7; x++ {
		f.assertData("probe", BlockPos{X: x, Y: 64}, []byte{8})
	}
}

// snapshotOf reads the probe's snapshot of call, anchored at anchor in the fixture's world.
func (f *hostFixture) snapshotOf(call Call, anchor BlockPos) snapshot {
	f.t.Helper()
	d := f.host.dispatchers["probe"]
	ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: anchor.cube(), actor: f.actor, call: call}
	snap, err := f.host.snapshot(context.Background(), d, &ev)
	if err != nil {
		f.t.Fatalf("snapshot: %v", err)
	}
	return snap
}

// Every callback anchored on a member has the network: a neighbor event, a break of a member,
// which floods from its six neighbors, and an epoch whose focus is a member; one anchored
// elsewhere has none. Members join the snapshot with their data and states.
func TestSnapshotsCarryTheNetwork(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	f.placeLine(probeNode, 0, 40)
	far := BlockPos{X: 40, Y: 64}
	if err := f.store.SetData("probe", overworldKey(far), []byte{9}, true); err != nil {
		t.Fatal(err)
	}
	f.place(probeCounter, BlockPos{X: 0, Y: 65}, nil)

	anchor := BlockPos{X: 20, Y: 64}
	snap := f.snapshotOf(Call{Neighbor: &NeighborCall{Pos: anchor, Neighbor: anchor}}, anchor)
	if snap.req.Network == nil || len(snap.req.Network.Blocks) != 41 || snap.req.Network.Truncated {
		t.Fatalf("neighbor event's network = %+v", snap.req.Network)
	}
	if snap.req.Network.Blocks[0] != anchor {
		t.Errorf("the network starts at %v, not its anchor", snap.req.Network.Blocks[0])
	}
	i := slices.IndexFunc(snap.req.Snapshot, func(c Cell) bool { return c.Pos == far })
	if i < 0 || snap.req.Snapshot[i].Data == nil || *snap.req.Snapshot[i].Data != "09" {
		t.Fatalf("the far member's cell is missing or lacks its data")
	}
	if len(snap.req.Snapshot) != 7+38 || len(snap.cells) != len(snap.req.Snapshot) {
		t.Errorf("%d snapshot cells, %d checked; want the anchor's 7 and the 38 members beyond them",
			len(snap.req.Snapshot), len(snap.cells))
	}

	focus := far
	epoch := f.snapshotOf(Call{Epoch: &EpochCall{Player: f.actorID(), Focus: &focus}}, far)
	if epoch.req.Network == nil || len(epoch.req.Network.Blocks) != 41 {
		t.Errorf("a focus on a member has network %+v", epoch.req.Network)
	}

	counter := BlockPos{X: 0, Y: 65}
	if off := f.snapshotOf(Call{Neighbor: &NeighborCall{Pos: counter, Neighbor: counter}}, counter); off.req.Network != nil {
		t.Errorf("a callback off the network has network %+v", off.req.Network)
	}

	f.do(func(tx *world.Tx) { tx.SetBlock(anchor.cube(), nil, nil) })
	broken := f.snapshotOf(Call{Break: &BreakCall{Change: Change{
		Pos: anchor, Cause: CausePlayer, BeforeID: probeNode, AfterID: airID,
	}}}, anchor)
	if broken.req.Network == nil || len(broken.req.Network.Blocks) != 40 ||
		slices.Contains(broken.req.Network.Blocks, anchor) {
		t.Errorf("a broken member's network = %+v; want both halves", broken.req.Network)
	}
}

// Data and state ops reach every network member and nothing else beyond the anchor's chunk
// column; set_block stays in the column. A foreign change to a far member makes the result
// stale.
func TestNetworkCommitScope(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	f.placeLine(probeNode, 0, 40)
	outside := BlockPos{X: 40, Y: 65}
	f.place(probeCounter, outside, nil)
	anchor, far := BlockPos{X: 1, Y: 64}, BlockPos{X: 40, Y: 64}
	d := f.host.dispatchers["probe"]
	ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: anchor.cube(), call: Call{
		Neighbor: &NeighborCall{Pos: anchor, Neighbor: anchor},
	}}
	snap := f.snapshotOf(ev.call, anchor)
	one := "01"
	for _, c := range []struct {
		op   Op
		want string
	}{
		{Op{SetBlock: &SetBlockOp{Pos: far, ID: airID}}, "outside the write scope"},
		{Op{SetBlockData: &SetBlockDataOp{Pos: outside, Data: &one}}, "outside the write scope"},
	} {
		f.host.commit(context.Background(), d, ev, snap, []Op{c.op})
		waitForRecord(t, logs, func(r map[string]any) bool {
			err, _ := r["error"].(string)
			return r["msg"] == "invalid result discarded" && strings.Contains(err, c.want)
		})
	}
	f.host.commit(context.Background(), d, ev, snap, []Op{{SetBlockData: &SetBlockDataOp{Pos: far, Data: &one}}})
	f.assertData("probe", far, []byte{1})

	snap = f.snapshotOf(ev.call, anchor)
	if err := f.store.SetData("probe", overworldKey(far), []byte{2}, true); err != nil {
		t.Fatal(err)
	}
	f.host.commit(context.Background(), d, ev, snap, []Op{{SetBlockData: &SetBlockDataOp{Pos: anchor, Data: &one}}})
	waitStale(t, logs, "changed")
	f.assertData("probe", anchor, nil)
}

// A distant bridge invalidates old topology, while an unrelated non-member boundary change does not.
func TestNetworkCommitRejectsJoinedBoundary(t *testing.T) {
	for _, replacement := range []string{probeNode, probeCounter} {
		t.Run(replacement, func(t *testing.T) {
			log, _ := testLog(t)
			sup, _ := startFake(t, "ok", log, startOptions{})
			f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
			t.Cleanup(func() { f.host.Close() })
			f.placeLine(probeNode, 0, 20)
			f.placeLine(probeNode, 22, 24)
			anchor, far := BlockPos{Y: 64}, BlockPos{X: 20, Y: 64}
			call := Call{Neighbor: &NeighborCall{Pos: anchor, Neighbor: anchor}}
			ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: anchor.cube(), call: call}
			snap := f.snapshotOf(call, anchor)
			if len(snap.req.Network.Blocks) != 21 {
				t.Fatalf("unexpected original network: %+v", snap.req.Network)
			}
			f.place(replacement, BlockPos{X: 21, Y: 64}, nil)
			data := "09"
			f.host.commit(context.Background(), f.host.dispatchers["probe"], ev, snap, []Op{{SetBlockData: &SetBlockDataOp{Pos: far, Data: &data}}})
			if replacement == probeNode {
				f.assertData("probe", far, nil)
			} else {
				f.assertData("probe", far, []byte{9})
			}
		})
	}
}
