package experience

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"log/slog"
	"maps"
	"slices"
	"sync"
	"sync/atomic"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/go-gl/mathgl/mgl64"
)

// airID is the id of air, the one block that an Experience may set besides its own.
const airID = "minecraft:air"

// Host runs the events of every Experience. Its hooks queue them on the world goroutines without
// blocking, and one worker per Experience runs them one at a time: a snapshot in one world task,
// the callback outside any task, then a commit in another task that checks the snapshot is still
// current and the result valid before it applies anything. Client messages that a commit stages
// go out through channels afterwards, and client messages that players send queue like events.
type Host struct {
	reg     *Registry
	store   *Store
	log     *slog.Logger
	worldID string
	// channels is the server half of the client parts; nil when the server has none, which
	// drops every staged client message.
	channels ClientChannels
	// dispatchers holds each Experience's queue and supervisor by Experience id. It never
	// changes after NewHost.
	dispatchers map[string]*dispatcher
	// holder is the Host as the installed hook sink.
	holder *sinkHolder

	// tell sends a commit's tells. Tests record them instead.
	tell teller
	// afterCall, when set, runs on the worker after each callback that returns an outcome,
	// before its commit. It is a test seam.
	afterCall func()

	// closed stops admission: once set, hooks queue nothing.
	closed atomic.Bool
	// stop is closed by Close, which ends Run.
	stop chan struct{}
	// workers counts the running workers.
	workers sync.WaitGroup

	// mu guards the fields below.
	mu sync.Mutex
	// resume is closed while the Host runs and open while it is paused.
	resume chan struct{}
	// running is set by Run, and closing by Close.
	running, closing bool
}

// dispatcher is the queue and supervisor of one Experience.
type dispatcher struct {
	id     string
	sup    *Supervisor
	events chan event
	// seq numbers the events that the worker runs. Only the worker uses it.
	seq uint64
	// dropped counts the events dropped for a full queue or the neighbor cap.
	dropped dropCount
	// undeclared counts committed client messages on channels that the Experience's client part
	// does not declare, and unsent those that the server half did not send.
	undeclared, unsent dropCount
	// neighborMu guards neighbors, the neighbor events admitted in the current tick of each
	// world, whose goroutines all admit them.
	neighborMu sync.Mutex
	neighbors  map[*world.World]*neighborTick
}

// neighborTick holds the positions of the neighbor events that one Experience admitted in one
// tick of a world. A tick runs its neighbour updates in a transaction of its own, which
// identifies the tick: a world's CurrentTick does not, since the nether and the end share the
// overworld's, which stands still while the overworld has no viewers. Holding tx keeps a later
// transaction from taking its address.
type neighborTick struct {
	tx   *world.Tx
	seen map[cube.Pos]struct{}
}

// event is one hook's callback, queued for its Experience's worker. A client message's event has
// no world until its snapshot finds the actor's.
type event struct {
	w   *world.World
	dim dimension
	// anchor is the block the callback is for. It bounds the snapshot and the writes.
	anchor cube.Pos
	// actor is the player who caused the event; nil for a neighbor event.
	actor *world.EntityHandle
	call  Call
}

// dimension is a world's dimension as the store and the guest name it.
type dimension struct {
	num int
	id  string
}

// storeKey is the store key of pos in the dimension.
func (d dimension) storeKey(pos cube.Pos) Key {
	return Key{Dim: d.num, X: int32(pos[0]), Y: int32(pos[1]), Z: int32(pos[2])}
}

// dimensionOf is the dimension of tx's world, and false for a dimension that has no id.
func dimensionOf(tx *world.Tx) (dimension, bool) {
	switch tx.World().Dimension() {
	case world.Overworld:
		return dimension{num: 0, id: "overworld"}, true
	case world.Nether:
		return dimension{num: 1, id: "nether"}, true
	case world.End:
		return dimension{num: 2, id: "end"}, true
	}
	return dimension{}, false
}

// NewHost returns a Host for the Experiences in sups, by Experience id, and installs it as the
// hook sink of every Experience block. worldID is the base name of the world folder, which
// callbacks see as their world id. channels is the server half of the client parts, or nil. A
// block of an Experience missing from sups keeps its data in the store but queues no events. The
// Host closes the supervisors when it closes.
func NewHost(
	reg *Registry, store *Store, sups map[string]*Supervisor, worldID string, channels ClientChannels,
	log *slog.Logger,
) *Host {
	h := &Host{
		reg:         reg,
		store:       store,
		log:         log,
		worldID:     worldID,
		channels:    channels,
		dispatchers: make(map[string]*dispatcher, len(sups)),
		tell:        messageTeller{},
		stop:        make(chan struct{}),
		resume:      make(chan struct{}),
	}
	close(h.resume)
	for id, sup := range sups {
		h.dispatchers[id] = &dispatcher{
			id:        id,
			sup:       sup,
			events:    make(chan event, eventQueueCap),
			neighbors: make(map[*world.World]*neighborTick),
		}
	}
	h.holder = &sinkHolder{h}
	installedHooks.Store(h.holder)
	return h
}

// Run runs one worker per Experience until ctx is done or the Host closes, and returns once
// every worker has stopped. Only the first call runs workers.
func (h *Host) Run(ctx context.Context) {
	ctx, cancel := context.WithCancel(ctx)
	defer cancel()
	h.mu.Lock()
	if h.running || h.closing {
		h.mu.Unlock()
		return
	}
	h.running = true
	for _, d := range h.dispatchers {
		h.workers.Go(func() { h.work(ctx, d) })
	}
	h.mu.Unlock()
	select {
	case <-ctx.Done():
	case <-h.stop:
	}
	cancel()
	h.workers.Wait()
}

// Pause stops the workers before their next snapshot or commit while paused is true; Pause(false)
// lets them continue, and a commit's check catches a snapshot that went stale meanwhile.
func (h *Host) Pause(paused bool) {
	h.mu.Lock()
	defer h.mu.Unlock()
	select {
	case <-h.resume:
		if paused {
			h.resume = make(chan struct{})
		}
	default:
		if !paused {
			close(h.resume)
		}
	}
}

// Reload restarts the helper of the Experience id and clears its quarantine.
func (h *Host) Reload(id string) error {
	d, ok := h.dispatchers[id]
	if !ok {
		return fmt.Errorf("no experience %q", id)
	}
	return d.sup.Reload()
}

// Close stops admitting events, cancels the workers and waits for them, uninstalls the Host's
// hooks, flushes the store and closes every supervisor. Calls after the first return nil.
func (h *Host) Close() error {
	h.mu.Lock()
	if h.closing {
		h.mu.Unlock()
		return nil
	}
	h.closing = true
	h.closed.Store(true)
	close(h.stop)
	h.mu.Unlock()
	h.workers.Wait()
	installedHooks.CompareAndSwap(h.holder, nil)

	var errs []error
	if err := h.store.Flush(); err != nil {
		errs = append(errs, fmt.Errorf("flushing experience data: %w", err))
	}
	for _, id := range slices.Sorted(maps.Keys(h.dispatchers)) {
		if err := h.dispatchers[id].sup.Close(); err != nil {
			errs = append(errs, err)
		}
	}
	return errors.Join(errs...)
}

// resumed waits until the Host is not paused, and reports false if ctx ends first.
func (h *Host) resumed(ctx context.Context) bool {
	h.mu.Lock()
	resume := h.resume
	h.mu.Unlock()
	select {
	case <-resume:
		return true
	case <-ctx.Done():
		return false
	}
}

// useOnBlock places b like a block item: on the clicked block if b may replace it, else beside
// it, through the user's PlaceBlock. If b is then there, it starts a new generation in the store
// and queues on-place.
func (h *Host) useOnBlock(
	b Block, pos cube.Pos, face cube.Face, _ mgl64.Vec3, tx *world.Tx, user item.User,
	ctx *item.UseContext,
) bool {
	placer, ok := user.(block.Placer)
	if !ok {
		return false
	}
	target, ok := replaceableTarget(tx, pos, face, b)
	if !ok {
		return false
	}
	before := blockID(tx.Block(target))
	placer.PlaceBlock(target, b, ctx)
	if tx.Block(target) != world.Block(b) {
		return false
	}
	dim, ok := dimensionOf(tx)
	if !ok {
		return true
	}
	h.store.Place(b.t.exp, dim.storeKey(target))
	actor := user.H()
	id := actor.UUID().String()
	h.enqueue(b.t.exp, event{w: tx.World(), dim: dim, anchor: target, actor: actor, call: Call{
		Place: &PlaceCall{Change: Change{
			Pos: blockPos(target), Actor: &id, Cause: CausePlayer, BeforeID: before, AfterID: b.t.id,
		}},
	}})
	return true
}

// activate queues on-interact for the user's interaction. The clicked face comes from the
// client unchecked, so a face out of range is ignored; the interaction stays consumed.
func (h *Host) activate(
	b Block, pos cube.Pos, clickedFace cube.Face, tx *world.Tx, u item.User, _ *item.UseContext,
) {
	dim, ok := dimensionOf(tx)
	if u == nil || !ok {
		return
	}
	if clickedFace < 0 || int(clickedFace) >= len(faces) {
		h.log.Debug("interaction with an invalid face ignored", "experience", b.t.exp, "face", int(clickedFace))
		return
	}
	actor := u.H()
	h.enqueue(b.t.exp, event{w: tx.World(), dim: dim, anchor: pos, actor: actor, call: Call{
		Interact: &InteractCall{
			Player: actor.UUID().String(), Pos: blockPos(pos), Face: faces[clickedFace],
		},
	}})
}

// neighbourUpdateTick queues on-neighbor-changed, at most once per position and
// maxNeighborEventsPerTick times per tick of the world for each Experience.
func (h *Host) neighbourUpdateTick(b Block, pos, changedNeighbour cube.Pos, tx *world.Tx) {
	d, ok := h.dispatchers[b.t.exp]
	if !ok || h.closed.Load() {
		return
	}
	dim, ok := dimensionOf(tx)
	if !ok || !h.admitNeighbor(d, tx, pos) {
		return
	}
	h.enqueue(b.t.exp, event{w: tx.World(), dim: dim, anchor: pos, call: Call{
		Neighbor: &NeighborCall{Pos: blockPos(pos), Neighbor: blockPos(changedNeighbour)},
	}})
}

// breakHandler deletes the broken block's data. A player's break then queues on-break with that
// data; a break without a user, such as an explosion, notifies nothing.
func (h *Host) breakHandler(b Block, pos cube.Pos, tx *world.Tx, u item.User) {
	dim, ok := dimensionOf(tx)
	if !ok {
		return
	}
	prev, had := h.store.Remove(b.t.exp, dim.storeKey(pos))
	if u == nil {
		return
	}
	var previous *string
	if had {
		s := hex.EncodeToString(prev)
		previous = &s
	}
	actor := u.H()
	id := actor.UUID().String()
	h.enqueue(b.t.exp, event{w: tx.World(), dim: dim, anchor: pos, actor: actor, call: Call{
		Break: &BreakCall{Change: Change{
			Pos: blockPos(pos), Actor: &id, Cause: CausePlayer, BeforeID: b.t.id,
			AfterID: blockID(tx.Block(pos)), PreviousData: previous,
		}},
	}})
}

// admitNeighbor reports whether a neighbor event at pos may be queued in this tick of tx's
// world: one per position, and maxNeighborEventsPerTick in all. One over the cap is dropped.
// NeighbourUpdateTick runs only in a tick's transaction, so tx identifies the tick.
func (h *Host) admitNeighbor(d *dispatcher, tx *world.Tx, pos cube.Pos) bool {
	d.neighborMu.Lock()
	window := d.neighbors[tx.World()]
	if window == nil {
		window = &neighborTick{seen: make(map[cube.Pos]struct{})}
		d.neighbors[tx.World()] = window
	}
	if window.tx != tx {
		window.tx = tx
		clear(window.seen)
	}
	_, seen := window.seen[pos]
	full := len(window.seen) >= maxNeighborEventsPerTick
	if !seen && !full {
		window.seen[pos] = struct{}{}
	}
	d.neighborMu.Unlock()
	if !seen && full {
		h.drop(d)
	}
	return !seen && !full
}

// enqueue queues ev for the Experience exp without blocking, dropping it when the queue is full.
func (h *Host) enqueue(exp string, ev event) {
	d, ok := h.dispatchers[exp]
	if !ok || h.closed.Load() {
		return
	}
	select {
	case d.events <- ev:
	default:
		h.drop(d)
	}
}

// drop counts a dropped event and logs the count at most once per dropLogInterval.
func (h *Host) drop(d *dispatcher) {
	if n, ok := d.dropped.add(); ok {
		h.log.Warn("events dropped", "experience", d.id, "dropped", n)
	}
}

// work runs the Experience's events one at a time until ctx is done.
func (h *Host) work(ctx context.Context, d *dispatcher) {
	for {
		select {
		case <-ctx.Done():
			return
		case ev := <-d.events:
			h.dispatch(ctx, d, ev)
		}
	}
}

// dispatch runs one event: its snapshot, its callback and, for a committed outcome, its commit.
// A failed or rejected callback or an error publishes nothing; the supervisor has counted a
// failure already.
func (h *Host) dispatch(ctx context.Context, d *dispatcher, ev event) {
	// A cancelled worker takes no more events, though select may still pick one.
	if ctx.Err() != nil || !h.resumed(ctx) {
		return
	}
	// A quarantined Experience's events are dropped before they cost a world task.
	if d.sup.Quarantined() {
		h.log.Debug("event of a quarantined experience dropped", "experience", d.id)
		return
	}
	d.seq++
	snap, err := h.snapshot(ctx, d, &ev)
	if err != nil {
		if ctx.Err() == nil {
			h.log.Warn("snapshot failed", "experience", d.id, "error", err)
		}
		return
	}
	outcome, err := d.sup.Call(snap.req)
	switch {
	case errors.Is(err, errQuarantined):
		// Quarantined between the check above and the call.
		h.log.Debug("event of a quarantined experience dropped", "experience", d.id)
		return
	case err != nil:
		h.log.Warn("callback error", "experience", d.id, "error", err)
		return
	}
	if h.afterCall != nil {
		h.afterCall()
	}
	if r := outcome.Rejected; r != nil {
		h.log.Debug("callback rejected", "experience", d.id, "reason", r.Reason)
	}
	if outcome.Committed == nil || !h.resumed(ctx) {
		return
	}
	h.commit(ctx, d, ev, snap, outcome.Committed.Ops)
}

// snapshot is what a worker read of the world for an event: the callback request and the state
// of each cell, which the commit checks again.
type snapshot struct {
	req   CallbackRequest
	cells []cellState
}

// cellState is the state of one snapshot cell that a commit must find unchanged, and the length
// of the data that the snapshot carried for it.
type cellState struct {
	pos    cube.Pos
	loaded bool
	id     string
	// owned is set for this Experience's own block with a store entry.
	owned    bool
	token    Token
	hasToken bool
	dataLen  uint64
}

// snapshot reads the event's anchor and its loaded neighbors in a fresh task of the event's
// world. A client message has no anchor, so its snapshot holds no cell; it runs in the world its
// actor is in at the time, which becomes the event's world.
func (h *Host) snapshot(ctx context.Context, d *dispatcher, ev *event) (snapshot, error) {
	var snap snapshot
	if ev.call.ClientMessage == nil {
		if err := await(ctx, ev.w.Do(func(tx *world.Tx) { snap = h.read(tx, d, *ev) })); err != nil {
			return snapshot{}, err
		}
		return snap, nil
	}
	found := false
	task := ev.actor.Do(func(tx *world.Tx, _ world.Entity) {
		if ev.dim, found = dimensionOf(tx); found {
			ev.w = tx.World()
			snap = h.read(tx, d, *ev)
		}
	})
	if err := await(ctx, task); err != nil {
		return snapshot{}, fmt.Errorf("finding the sender of a client message: %w", err)
	}
	if !found {
		return snapshot{}, errors.New("the sender of a client message is in a world without a dimension id")
	}
	return snap, nil
}

// await waits for task until ctx ends, then cancels it. A task that has already started cannot
// be cancelled, so await waits for it to finish: nothing it does outlives the worker, and a
// commit's writes land before Close flushes the store.
func await(ctx context.Context, task *world.Task) error {
	err := task.Wait(ctx)
	if err != nil && !task.Cancel() {
		<-task.Done()
	}
	return err
}

// read builds the event's snapshot in tx: the anchor and its six neighbors within the world's
// height, loaded or not, with the data of the owned ones; nothing for a client message.
func (h *Host) read(tx *world.Tx, d *dispatcher, ev event) snapshot {
	r := tx.Range()
	snap := snapshot{
		req: CallbackRequest{
			Info: Info{
				WorldID: h.worldID, DimensionID: ev.dim.id, Tick: uint64(tx.CurrentTick()),
				EventSequence: d.seq,
			},
			WorldMinY:  int32(r.Min()),
			WorldMaxY:  int32(r.Max()),
			DataBudget: h.store.Budget(d.id),
			Snapshot:   make([]Cell, 0, 1+len(cube.Faces())),
			Call:       ev.call,
		},
		cells: make([]cellState, 0, 1+len(cube.Faces())),
	}
	if ev.actor != nil {
		id := ev.actor.UUID().String()
		snap.req.Actor = &id
	}
	var positions []cube.Pos
	if ev.call.ClientMessage == nil {
		positions = append(positions, ev.anchor)
		for _, f := range cube.Faces() {
			if side := ev.anchor.Side(f); !side.OutOfBounds(r) {
				positions = append(positions, side)
			}
		}
	}
	for _, pos := range positions {
		st := h.cellState(tx, d.id, ev.dim, pos)
		cell := Cell{Pos: blockPos(pos), Loaded: st.loaded, ID: st.id, Owned: st.owned}
		if st.owned {
			if data, ok := h.store.Data(d.id, ev.dim.storeKey(pos)); ok {
				s := hex.EncodeToString(data)
				cell.Data = &s
				st.dataLen = uint64(len(data))
			}
		}
		snap.req.Snapshot = append(snap.req.Snapshot, cell)
		snap.cells = append(snap.cells, st)
	}
	return snap
}

// cellState reads the block at pos without loading its chunk, and the Experience's store entry
// there. An unloaded cell has no id and is not owned.
func (h *Host) cellState(tx *world.Tx, exp string, dim dimension, pos cube.Pos) cellState {
	st := cellState{pos: pos}
	st.token, st.hasToken = h.store.Token(exp, dim.storeKey(pos))
	b, loaded := tx.BlockLoaded(pos)
	if !loaded {
		return st
	}
	st.loaded = true
	st.id = blockID(b)
	own, ok := b.(Block)
	st.owned = ok && own.t.exp == exp && st.hasToken
	return st
}

// replaceableTarget is where a block item used on the face of the block at pos places with:
// pos itself if with may replace it, else the block beside that face if with may replace that.
func replaceableTarget(tx *world.Tx, pos cube.Pos, face cube.Face, with world.Block) (cube.Pos, bool) {
	if replaceableWith(tx, pos, with) {
		return pos, true
	}
	side := pos.Side(face)
	return side, replaceableWith(tx, side, with)
}

// replaceableWith reports whether with may replace the block at pos, as Dragonfly's own block
// items decide.
func replaceableWith(tx *world.Tx, pos cube.Pos, with world.Block) bool {
	if pos.OutOfBounds(tx.Range()) {
		return false
	}
	b := tx.Block(pos)
	replaceable, ok := b.(block.Replaceable)
	if !ok || !replaceable.ReplaceableBy(with) || b == with {
		return false
	}
	if liquid, ok := tx.Liquid(pos); ok {
		replaceable, ok := liquid.(block.Replaceable)
		return ok && replaceable.ReplaceableBy(with)
	}
	return true
}

// blockID is the id of b, without its state.
func blockID(b world.Block) string {
	id, _ := b.EncodeBlock()
	return id
}

// blockPos is pos as the protocol writes it.
func blockPos(pos cube.Pos) BlockPos {
	return BlockPos{X: int32(pos[0]), Y: int32(pos[1]), Z: int32(pos[2])}
}

// cube is p as Dragonfly writes it.
func (p BlockPos) cube() cube.Pos {
	return cube.Pos{int(p.X), int(p.Y), int(p.Z)}
}
