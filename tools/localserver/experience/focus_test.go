package experience

import (
	"context"
	"log/slog"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// focusEcho is the channel that the focus tests' client messages arrive on; the probe echoes
// them back and tells what it did.
const focusEcho = "probe.echo"

// noFocus is the probe's tell for a client message on focusEcho, schema 1, without fields, whose
// callback has no focus: its block read and data write at the origin are refused.
const noFocus = "client probe.echo 1 0 focus none read denied write denied echo ok"

// moveTo teleports the actor to at, its feet, and keeps it there: a player without a session
// otherwise falls.
func (f *hostFixture) moveTo(at mgl64.Vec3) {
	f.t.Helper()
	f.do(func(tx *world.Tx) {
		p := f.player(tx)
		p.SetImmobile()
		p.Teleport(at)
	})
}

// beside is where the actor stands next to the block at pos, well within the focus range.
func beside(pos BlockPos) mgl64.Vec3 {
	return pos.cube().Vec3Centre().Add(mgl64.Vec3{1.5, -0.5, 0})
}

// focusedOn sets the actor's focus on a fresh probe counter at pos: it places the counter, moves
// the actor beside it and has it use the counter, whose on-interact tells "count 1".
func (f *hostFixture) focusedOn(pos BlockPos) {
	f.t.Helper()
	f.place(probeCounter, pos, nil)
	f.moveTo(beside(pos))
	f.activate(pos)
	f.waitTells(5*time.Second, "count 1")
	f.tells = &tellRecorder{}
	f.host.tell = f.tells
}

// message delivers an empty client message on focusEcho, schema 1, from the actor.
func (f *hostFixture) message() {
	f.t.Helper()
	if !f.host.DeliverClientMessage(f.actor, "probe", focusEcho, 1, nil) {
		f.t.Fatal("the client message was not queued")
	}
}

// focusFixture runs the probe with the actor's focus on a probe counter at probePos(probeCount).
func focusFixture(t *testing.T) (*hostFixture, *logBuffer, BlockPos) {
	t.Helper()
	log, logs := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeCount)
	f.focusedOn(pos)
	return f, logs, pos
}

// After the actor used a probe counter, its client message gets that block: the probe reads it
// and writes its data, which commits.
func TestClientMessageGetsFocus(t *testing.T) {
	f, _, pos := focusFixture(t)
	f.message()
	f.waitTells(5*time.Second, "client probe.echo 1 0 focus 0 64 0 read probe:counter write ok echo ok")
	f.assertData("probe", pos, []byte{1})
}

// An epoch gets the focus like a client message.
func TestEpochGetsFocus(t *testing.T) {
	f, _, pos := focusFixture(t)
	if !f.host.DeliverEpoch(f.actor, "probe") {
		t.Fatal("the epoch was not queued")
	}
	f.waitTells(5*time.Second, "epoch focus 0 64 0 read probe:counter write ok send ok")
	f.assertData("probe", pos, []byte{1})
}

// A newer interaction replaces the focus.
func TestNewerInteractionReplacesFocus(t *testing.T) {
	f, _, first := focusFixture(t)
	second := BlockPos{X: first.X, Y: first.Y, Z: first.Z + 2}
	f.focusedOn(second)
	f.message()
	f.waitTells(5*time.Second, "client probe.echo 1 0 focus 0 64 2 read probe:counter write ok echo ok")
	f.assertData("probe", first, counter(1))
	f.assertData("probe", second, []byte{1})
}

// A player who used no block has no focus.
func TestNoInteractionNoFocus(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	f.message()
	f.waitTells(5*time.Second, noFocus)
}

// A focus block that was replaced, by another placement or by air, starts no snapshot: its
// generation is not the focus's.
func TestReplacedFocusIsNone(t *testing.T) {
	for _, tc := range []struct {
		name    string
		replace func(f *hostFixture, pos BlockPos)
	}{
		{"placed again", func(f *hostFixture, pos BlockPos) { f.place(probeCounter, pos, counter(7)) }},
		{"broken", func(f *hostFixture, pos BlockPos) {
			f.do(func(tx *world.Tx) { tx.SetBlock(pos.cube(), nil, nil) })
			f.store.Remove("probe", overworldKey(pos))
		}},
	} {
		t.Run(tc.name, func(t *testing.T) {
			f, _, pos := focusFixture(t)
			tc.replace(f, pos)
			f.message()
			f.waitTells(5*time.Second, noFocus)
		})
	}
}

// A focus farther than provisionalFocusRange from the actor's eyes is none, and counts again
// once the actor is back in range.
func TestFocusOutOfRangeIsNone(t *testing.T) {
	f, _, pos := focusFixture(t)
	f.moveTo(pos.cube().Vec3Centre().Add(mgl64.Vec3{provisionalFocusRange + 1, 0, 0}))
	f.message()
	f.waitTells(5*time.Second, noFocus)
	f.moveTo(beside(pos))
	f.message()
	f.waitTells(5*time.Second, noFocus,
		"client probe.echo 1 0 focus 0 64 0 read probe:counter write ok echo ok")
}

// A focus in another dimension than the actor's is none: the callback runs in the actor's world.
func TestFocusInOtherDimensionIsNone(t *testing.T) {
	f, _, _ := focusFixture(t)
	nether := world.Config{Dim: world.Nether, Log: slog.New(slog.DiscardHandler)}.New()
	t.Cleanup(func() { nether.Close() })
	f.do(func(tx *world.Tx) { tx.RemoveEntity(f.player(tx)) })
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	// The same position in the nether, beside a block that is not the focus.
	if err := nether.Do(func(tx *world.Tx) { tx.AddEntity(f.actor) }).Wait(ctx); err != nil {
		t.Fatalf("moving the actor to the nether: %v", err)
	}
	f.message()
	f.waitTells(5*time.Second, noFocus)
}

// A player who left loses their focus.
func TestPlayerLeftClearsFocus(t *testing.T) {
	f, _, _ := focusFixture(t)
	f.host.PlayerLeft(f.actor.UUID())
	f.message()
	f.waitTells(5*time.Second, noFocus)
}

// A focus block whose data changes between the snapshot and the commit makes the result stale:
// nothing of it commits, as for any other snapshot.
func TestStaleFocusDiscardsResult(t *testing.T) {
	f, logs, pos := focusFixture(t)
	f.host.afterCall = func() {
		if err := f.store.SetData("probe", overworldKey(pos), []byte{9}, true); err != nil {
			t.Errorf("SetData: %v", err)
		}
	}
	f.message()
	waitStale(t, logs, "changed")
	f.assertData("probe", pos, []byte{9})
	if tells := f.tells.all(); len(tells) != 0 {
		t.Fatalf("a stale result told %+v", tells)
	}
}

// Only an Experience whose world takes a focus gets one: an older one's client messages and
// epochs keep their empty snapshot after its player used its block. A focused snapshot holds the
// focus block and its neighbors, like on-interact's.
func TestFocusOnlyForExperiencesThatTakeIt(t *testing.T) {
	for _, takes := range []bool{false, true} {
		log, _ := testLog(t)
		loaded := fakeLoaded()
		loaded.Focus = takes
		sup, _, err := startSupervisor(fakeHelperBinary, t.TempDir(), log,
			startOptions{env: fakeEnv(t, "ok", loaded)})
		if err != nil {
			t.Fatalf("starting the fake helper: %v", err)
		}
		t.Cleanup(func() { sup.Close() })
		f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
		d := f.host.dispatchers["probe"]
		ctx := context.Background()
		pos := probePos(probeCount)
		f.place(probeCounter, pos, nil)
		f.moveTo(beside(pos))
		f.activate(pos)
		f.host.dispatch(ctx, d, <-d.events)
		for _, deliver := range []func() bool{
			func() bool { return f.host.DeliverClientMessage(f.actor, "probe", focusEcho, 1, nil) },
			func() bool { return f.host.DeliverEpoch(f.actor, "probe") },
		} {
			if !deliver() {
				t.Fatal("the callback was not queued")
			}
			ev := <-d.events
			snap, err := f.host.snapshot(ctx, d, &ev)
			if err != nil {
				t.Fatalf("snapshot: %v", err)
			}
			focus := snap.req.Call.focus()
			switch {
			case !takes && (focus != nil || len(snap.req.Snapshot) != 0):
				t.Fatalf("an Experience without focus got focus %v and snapshot %s", focus, jsonOf(snap.req.Snapshot))
			case takes && (focus == nil || *focus != pos || len(snap.req.Snapshot) != 1+len(cube.Faces())):
				t.Fatalf("an Experience with focus got focus %v and snapshot %s", focus, jsonOf(snap.req.Snapshot))
			}
		}
	}
}
