package experience

import (
	"context"
	"encoding/binary"
	"encoding/hex"
	"fmt"
	"log/slog"
	"slices"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// More probe behaviors, selected like those in helpers_test.go by the x of the interacted block.
const (
	// probeLoop spins until the runtime stops it.
	probeLoop = 2
	// probeSetUp sets the block above to a probe counter and tells the id it reads there.
	probeSetUp = 5
)

// staleRecord is the message of the record that a discarded stale result logs.
const staleRecord = "stale result discarded"

// waitStale waits for a stale discard whose reason contains why.
func waitStale(t *testing.T, logs *logBuffer, why string) {
	t.Helper()
	waitForRecord(t, logs, func(r map[string]any) bool {
		reason, _ := r["reason"].(string)
		return r["msg"] == staleRecord && strings.Contains(reason, why)
	})
}

// tellRecorder is a teller that records every tell instead of sending it.
type tellRecorder struct {
	mu    sync.Mutex
	tells []recordedTell
}

// recordedTell is a tell to the player with the UUID.
type recordedTell struct{ player, text string }

func (r *tellRecorder) tell(p world.Entity, text string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	r.tells = append(r.tells, recordedTell{player: p.H().UUID().String(), text: text})
}

// all returns every tell so far.
func (r *tellRecorder) all() []recordedTell {
	r.mu.Lock()
	defer r.mu.Unlock()
	return slices.Clone(r.tells)
}

// hostFixture is a running Host over a world of the registered blocks, holding a player without
// a session who acts in every event, and a tellRecorder in place of player messages.
type hostFixture struct {
	t     *testing.T
	store *Store
	host  *Host
	w     *world.World
	actor *world.EntityHandle
	tells *tellRecorder
}

// newHostFixture runs a Host of sups without a server half for client parts, logging to log. It
// closes the Host, and so sups, at cleanup.
func newHostFixture(t *testing.T, log *slog.Logger, sups map[string]*Supervisor) *hostFixture {
	t.Helper()
	return newClientFixture(t, log, sups, nil)
}

// newClientFixture is newHostFixture with channels as the server half for client parts.
func newClientFixture(
	t *testing.T, log *slog.Logger, sups map[string]*Supervisor, channels ClientChannels,
) *hostFixture {
	t.Helper()
	f := newIdleClientFixture(t, log, sups, channels)
	ctx, cancel := context.WithCancel(context.Background())
	done := make(chan struct{})
	go func() {
		defer close(done)
		f.host.Run(ctx)
	}()
	// Cleanups run last first: the Host closes, which ends Run, before the world closes.
	t.Cleanup(func() {
		if err := f.host.Close(); err != nil {
			t.Errorf("Host.Close: %v", err)
		}
		cancel()
		<-done
	})
	return f
}

// newIdleFixture is newHostFixture without running the Host's workers.
func newIdleFixture(t *testing.T, log *slog.Logger, sups map[string]*Supervisor) *hostFixture {
	t.Helper()
	return newIdleClientFixture(t, log, sups, nil)
}

// newIdleClientFixture is newClientFixture without running the Host's workers.
func newIdleClientFixture(
	t *testing.T, log *slog.Logger, sups map[string]*Supervisor, channels ClientChannels,
) *hostFixture {
	t.Helper()
	reg := registered(t)
	w := world.Config{Log: slog.New(slog.DiscardHandler)}.New()
	t.Cleanup(func() { w.Close() })
	f := &hostFixture{t: t, store: openTestStore(t, t.TempDir()), w: w, tells: &tellRecorder{}}
	f.host = NewHost(reg, f.store, sups, "world", channels, log)
	f.host.tell = f.tells
	f.actor = world.EntitySpawnOpts{Position: mgl64.Vec3{0.5, 100, 40.5}}.
		New(player.Type, player.Config{Name: "Actor"})
	f.do(func(tx *world.Tx) { tx.AddEntity(f.actor) })
	return f
}

// do runs fn in a world transaction and waits for it.
func (f *hostFixture) do(fn func(tx *world.Tx)) {
	f.t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := f.w.Do(fn).Wait(ctx); err != nil {
		f.t.Fatalf("world task: %v", err)
	}
}

// player returns the actor's entity in tx.
func (f *hostFixture) player(tx *world.Tx) *player.Player {
	f.t.Helper()
	e, ok := f.actor.Entity(tx)
	if !ok {
		f.t.Fatal("the actor is not in the world")
	}
	return e.(*player.Player)
}

// actorID is the actor's player id.
func (f *hostFixture) actorID() string {
	return f.actor.UUID().String()
}

// place sets the block id at pos and records the placement, with data unless it is nil.
func (f *hostFixture) place(id string, pos BlockPos, data []byte) {
	f.t.Helper()
	b, ok := registered(f.t).Lookup(id)
	if !ok {
		f.t.Fatalf("no registered block %q", id)
	}
	f.do(func(tx *world.Tx) { tx.SetBlock(pos.cube(), b, nil) })
	f.store.Place(b.t.exp, overworldKey(pos))
	if data != nil {
		if err := f.store.SetData(b.t.exp, overworldKey(pos), data, true); err != nil {
			f.t.Fatalf("SetData: %v", err)
		}
	}
}

// activate has the actor interact with the Experience block at each pos, in one transaction.
func (f *hostFixture) activate(pos ...BlockPos) {
	f.t.Helper()
	f.do(func(tx *world.Tx) {
		p := f.player(tx)
		for _, at := range pos {
			tx.Block(at.cube()).(Block).Activate(at.cube(), cube.FaceUp, tx, p, nil)
		}
	})
}

// waitTells waits until the actor has been told want, in order, and nothing else.
func (f *hostFixture) waitTells(within time.Duration, want ...string) {
	f.t.Helper()
	deadline := time.Now().Add(within)
	for {
		got := f.tells.all()
		if len(got) >= len(want) {
			var texts []string
			for _, tell := range got {
				if tell.player != f.actorID() {
					f.t.Fatalf("told %s %q; only the actor %s may be told", tell.player, tell.text, f.actorID())
				}
				texts = append(texts, tell.text)
			}
			if !slices.Equal(texts, want) {
				f.t.Fatalf("tells = %q, want %q", texts, want)
			}
			return
		}
		if time.Now().After(deadline) {
			f.t.Fatalf("tells after %v = %+v, want %q", within, got, want)
		}
		time.Sleep(5 * time.Millisecond)
	}
}

// assertData checks the data that the store holds for exp at pos.
func (f *hostFixture) assertData(exp string, pos BlockPos, want []byte) {
	f.t.Helper()
	got, ok := f.store.Data(exp, overworldKey(pos))
	if want == nil && ok {
		f.t.Fatalf("data at %v = %s, want none", pos, dataText(got))
	}
	if want != nil && (!ok || !slices.Equal(got, want)) {
		f.t.Fatalf("data at %v = %s (present %v), want %s", pos, dataText(got), ok, dataText(want))
	}
}

// dataText renders data for a failure message: its hex, or only its length when that is long.
func dataText(data []byte) string {
	if len(data) > 16 {
		return fmt.Sprintf("%d bytes", len(data))
	}
	return fmt.Sprintf("%x", data)
}

// overworldKey is the store key of pos in the overworld.
func overworldKey(pos BlockPos) Key {
	return Key{Dim: 0, X: pos.X, Y: pos.Y, Z: pos.Z}
}

// counter is the probe counter's data holding n.
func counter(n uint32) []byte {
	return binary.LittleEndian.AppendUint32(nil, n)
}

// Each interaction runs on a snapshot that the previous commit changed, so the counter counts.
func TestInteractIncrementsCounterAcrossCalls(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	f.activate(pos, pos)
	f.waitTells(5*time.Second, "count 1", "count 2")
	f.assertData("probe", pos, counter(2))
}

// A trapped callback stages a data write and a tell, and neither reaches the world.
func TestTrapPublishesNothing(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	trap, count := probePos(probeTrap), probePos(probeCount)
	f.place(probeCounter, trap, nil)
	f.place(probeCounter, count, nil)
	// One worker runs the Experience's events in order, so once the second has committed the
	// first has been handled.
	f.activate(trap, count)
	f.waitTells(5*time.Second, "count 1")
	waitForRecord(t, logs, func(r map[string]any) bool {
		return r["msg"] == "callback failed" && r["kind"] == string(FailTrap)
	})
	f.assertData("probe", trap, nil)
}

// A data write between the snapshot and the commit changes the anchor's token, so the result
// is discarded whole.
func TestStaleSnapshotDiscardsResult(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	f.host.afterCall = func() {
		if err := f.store.SetData("probe", overworldKey(pos), []byte{9}, true); err != nil {
			t.Errorf("SetData: %v", err)
		}
	}
	f.activate(pos)
	waitStale(t, logs, "changed")
	f.assertData("probe", pos, []byte{9})
	if tells := f.tells.all(); len(tells) != 0 {
		t.Fatalf("a stale result told %+v", tells)
	}
}

// Hooks only enqueue: with the helper hanging and the queue full, 300 interactions on the world
// goroutine finish within 50 ms, and the overflow is dropped and logged.
func TestHooksNeverBlockWhenHelperHangs(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startFake(t, "hang", log, startOptions{})
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	var elapsed time.Duration
	f.do(func(tx *world.Tx) {
		p, b := f.player(tx), tx.Block(pos.cube()).(Block)
		start := time.Now()
		for range 300 {
			b.Activate(pos.cube(), cube.FaceUp, tx, p, nil)
		}
		elapsed = time.Since(start)
	})
	if elapsed > 50*time.Millisecond {
		t.Fatalf("300 interactions took %v on the world goroutine, want at most 50ms", elapsed)
	}
	waitForRecord(t, logs, func(r map[string]any) bool {
		return r["msg"] == "events dropped" && r["experience"] == "probe"
	})
	// The fake ignores shutdown, so closing it reports a kill.
	f.host.Close()
}

// An Experience whose callbacks spin until stopped does not hold up another's.
func TestHealthyExperienceRespondsBesideHostile(t *testing.T) {
	log, _ := testLog(t)
	probe, _ := startProbe(t, log)
	// "other" runs the probe guest too, so its block at x=2 loops.
	other, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": probe, "other": other})
	hostile, healthy := probePos(probeLoop), probePos(probeCount)
	f.place(otherCounter, hostile, nil)
	f.place(probeCounter, healthy, nil)
	f.activate(slices.Repeat([]BlockPos{hostile}, 20)...)
	f.activate(healthy)
	f.waitTells(3*time.Second, "count 1")
	f.assertData("probe", healthy, counter(1))
}

// A player break deletes the block's data at once and calls on-break with the old data.
func TestBreakDeletesDataAndSendsPrevious(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeCount)
	f.place(probeCounter, pos, []byte{1, 2, 3, 4})
	f.do(func(tx *world.Tx) {
		// As a player break does: the block goes, then its BreakHandler runs.
		b := tx.Block(pos.cube()).(Block)
		tx.SetBlock(pos.cube(), nil, nil)
		b.BreakInfo().BreakHandler(pos.cube(), tx, f.player(tx))
	})
	if tok, ok := f.store.Token("probe", overworldKey(pos)); ok {
		t.Fatalf("token %+v survived the break", tok)
	}
	f.assertData("probe", pos, nil)
	f.waitTells(5*time.Second, "broke 4")
}

// Setting an own block over an owned one starts a new generation without data.
func TestReplacementClearsData(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeSetUp)
	up := BlockPos{X: pos.X, Y: pos.Y + 1, Z: pos.Z}
	f.place(probeCounter, pos, nil)
	f.place(probeCounter, up, []byte{7})
	before, _ := f.store.Token("probe", overworldKey(up))
	f.activate(pos)
	f.waitTells(5*time.Second, probeCounter)
	after, ok := f.store.Token("probe", overworldKey(up))
	if !ok || after.Generation == before.Generation {
		t.Fatalf("token at up = %+v (present %v), want a generation after %+v", after, ok, before)
	}
	f.assertData("probe", up, nil)
	var id string
	f.do(func(tx *world.Tx) { id = blockID(tx.Block(up.cube())) })
	if id != probeCounter {
		t.Fatalf("block at up = %s, want %s", id, probeCounter)
	}
}

// Pause holds an event before its snapshot and a result before its commit; resuming commits it.
func TestPauseDefersCommit(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	var calls atomic.Int32
	f.host.afterCall = func() {
		calls.Add(1)
		f.host.Pause(true)
	}
	f.host.Pause(true)
	f.activate(pos)
	time.Sleep(200 * time.Millisecond)
	if n := calls.Load(); n != 0 {
		t.Fatalf("%d callbacks ran while paused", n)
	}

	f.host.Pause(false)
	deadline := time.Now().Add(5 * time.Second)
	for calls.Load() == 0 {
		if time.Now().After(deadline) {
			t.Fatal("no callback ran after resuming")
		}
		time.Sleep(5 * time.Millisecond)
	}
	time.Sleep(200 * time.Millisecond)
	if tells := f.tells.all(); len(tells) != 0 {
		t.Fatalf("told %+v while paused", tells)
	}
	f.assertData("probe", pos, nil)

	f.host.Pause(false)
	f.waitTells(5*time.Second, "count 1")
	f.assertData("probe", pos, counter(1))
}

// An actor who leaves the world before the commit makes the result stale: no tell and no write.
func TestTellOnlyToConnectedActor(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startProbe(t, log)
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	f.do(func(tx *world.Tx) {
		p := f.player(tx)
		tx.Block(pos.cube()).(Block).Activate(pos.cube(), cube.FaceUp, tx, p, nil)
		tx.RemoveEntity(p)
	})
	waitStale(t, logs, "is not in the world")
	if tells := f.tells.all(); len(tells) != 0 {
		t.Fatalf("told %+v after the actor left", tells)
	}
	f.assertData("probe", pos, nil)
}

// A clicked face comes from the client unchecked. One out of range consumes the interaction and
// queues nothing, instead of panicking on the world goroutine.
func TestActivateIgnoresFaceOutOfRange(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	f.do(func(tx *world.Tx) {
		p, b := f.player(tx), tx.Block(pos.cube()).(Block)
		for _, face := range []cube.Face{-1, cube.Face(len(cube.Faces()))} {
			if !b.Activate(pos.cube(), face, tx, p, nil) {
				t.Errorf("Activate with face %d did not consume the interaction", face)
			}
		}
	})
	if n := len(f.host.dispatchers["probe"].events); n != 0 {
		t.Fatalf("queued %d events for faces out of range", n)
	}
}

// A quarantined Experience's events are dropped before their snapshot: they need no world task,
// so they are dropped even while the world goroutine is busy.
func TestQuarantinedEventsTakeNoSnapshot(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newHostFixture(t, log, map[string]*Supervisor{"probe": sup})
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	f.host.Pause(true)
	f.activate(pos)
	sup.mu.Lock()
	sup.quarantine("test")
	sup.mu.Unlock()

	started, release := make(chan struct{}), make(chan struct{})
	defer close(release)
	f.w.Do(func(*world.Tx) {
		close(started)
		<-release
	})
	<-started
	f.host.Pause(false)
	waitForRecord(t, logs, func(r map[string]any) bool {
		return r["msg"] == "event of a quarantined experience dropped"
	})
}

// Neighbor events of one tick are deduplicated by position and capped per Experience.
func TestNeighborEventsDedupedAndCapped(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	b, _ := registered(t).Lookup(probeCounter)
	f.do(func(tx *world.Tx) {
		for i := range maxNeighborEventsPerTick + 10 {
			pos := cube.Pos{i, 64, 0}
			b.NeighbourUpdateTick(pos, pos.Side(cube.FaceUp), tx)
			b.NeighbourUpdateTick(pos, pos.Side(cube.FaceDown), tx)
		}
	})
	queue := f.host.dispatchers["probe"].events
	if len(queue) != maxNeighborEventsPerTick {
		t.Fatalf("queued %d neighbor events in one tick, want %d", len(queue), maxNeighborEventsPerTick)
	}
	seen := map[BlockPos]bool{}
	for range maxNeighborEventsPerTick {
		ev := <-queue
		if seen[ev.call.Neighbor.Pos] {
			t.Fatalf("two neighbor events for %v in one tick", ev.call.Neighbor.Pos)
		}
		seen[ev.call.Neighbor.Pos] = true
	}
}

// Each tick's neighbor updates run in a transaction of their own, which starts a fresh window
// even when the world's CurrentTick stands still, as it does in the nether and the end, whose
// tick the overworld keeps.
func TestNeighborCapResetsEachTickTransaction(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	b, _ := registered(t).Lookup(probeCounter)
	// A viewerless world still runs its first tick, which moves CurrentTick from 0 to 1; after
	// that its tick stands still.
	for deadline := time.Now().Add(2 * time.Second); ; time.Sleep(5 * time.Millisecond) {
		var tick int64
		f.do(func(tx *world.Tx) { tick = tx.CurrentTick() })
		if tick >= 1 {
			break
		}
		if time.Now().After(deadline) {
			t.Fatal("the world ran no first tick within 2s")
		}
	}
	var ticks []int64
	batch := func() {
		f.do(func(tx *world.Tx) {
			ticks = append(ticks, tx.CurrentTick())
			for i := range maxNeighborEventsPerTick {
				pos := cube.Pos{i, 64, 0}
				b.NeighbourUpdateTick(pos, pos.Side(cube.FaceUp), tx)
			}
		})
	}
	batch()
	batch()
	if ticks[0] != ticks[1] {
		t.Fatalf("CurrentTick moved from %d to %d; the test needs a world whose tick stands still",
			ticks[0], ticks[1])
	}
	if got, want := len(f.host.dispatchers["probe"].events), 2*maxNeighborEventsPerTick; got != want {
		t.Fatalf("queued %d neighbor events in two batches, want %d", got, want)
	}
}

// The quota bounds a result's net data, as the runtime checks it, not each write on its way: a
// result that grows one block past the quota while it shrinks another commits, and one whose net
// exceeds the quota by a byte commits nothing.
func TestQuotaCheckedOnNetData(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	full := make([]byte, maxBlockDataBytes)
	a, b := probePos(probeCount), BlockPos{X: 1, Y: 64}
	f.place(probeCounter, a, nil)
	f.place(probeCounter, b, full)
	// Blocks elsewhere fill the rest of the quota.
	for i := range dataQuota/maxBlockDataBytes - 1 {
		k := Key{X: 1000 + int32(i), Y: 64}
		f.store.Place("probe", k)
		if err := f.store.SetData("probe", k, full, true); err != nil {
			t.Fatalf("filling the quota: %v", err)
		}
	}
	if budget := f.store.Budget("probe"); budget != 0 {
		t.Fatalf("budget = %d, want the quota full", budget)
	}
	d := f.host.dispatchers["probe"]
	ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: a.cube(), call: Call{
		Neighbor: &NeighborCall{Pos: a, Neighbor: b},
	}}
	snap, err := f.host.snapshot(context.Background(), d, &ev)
	if err != nil {
		t.Fatalf("snapshot: %v", err)
	}
	grow := hex.EncodeToString(full)
	commit := func(shrunk []byte) {
		s := hex.EncodeToString(shrunk)
		f.host.commit(context.Background(), d, ev, snap, []Op{
			{SetBlockData: &SetBlockDataOp{Pos: a, Data: &grow}},
			{SetBlockData: &SetBlockDataOp{Pos: b, Data: &s}},
		})
	}

	commit([]byte{1})
	waitForRecord(t, logs, func(r map[string]any) bool {
		err, _ := r["error"].(string)
		return r["msg"] == "invalid result discarded" && strings.Contains(err, "quota")
	})
	f.assertData("probe", a, nil)
	f.assertData("probe", b, full)

	commit([]byte{})
	f.assertData("probe", a, full)
	f.assertData("probe", b, []byte{})
}
