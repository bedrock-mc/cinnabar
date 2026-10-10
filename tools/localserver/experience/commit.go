package experience

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"unicode"

	"github.com/df-mc/dragonfly/server/block/cube"
	"github.com/df-mc/dragonfly/server/entity"
	"github.com/df-mc/dragonfly/server/item"
	"github.com/df-mc/dragonfly/server/player"
	"github.com/df-mc/dragonfly/server/world"
)

// formattingPrefix starts a Minecraft formatting code, which a tell may not hold.
const formattingPrefix = '§'

// errStale discards a result whose snapshot no longer matches the world, or whose actor has left
// it. It is an expected race, not a fault.
var errStale = errors.New("stale result")

// errInvalid discards a result holding an op that the runtime should never have committed. It is
// a bug in the runtime or the adapter, so the Experience is not blamed.
var errInvalid = errors.New("invalid result")

// teller sends a tell to a player.
type teller interface {
	tell(p world.Entity, text string)
}

// messageTeller tells a player with a chat message.
type messageTeller struct{}

func (messageTeller) tell(p world.Entity, text string) {
	if pl, ok := p.(*player.Player); ok {
		pl.Message(text)
	}
}

// commit applies ops, the committed result of ev's callback on snap, in a fresh task of the
// event's world, unless the snapshot went stale or an op is invalid; then it applies nothing.
// Once the result has applied, its client messages go out to the actor's client part.
func (h *Host) commit(ctx context.Context, d *dispatcher, ev event, snap snapshot, ops []Op) {
	var sends []*SendClientOp
	var result error
	task := ev.w.Do(func(tx *world.Tx) { sends, result = h.apply(tx, d.id, ev, snap, ops) })
	if err := await(ctx, task); err != nil {
		if ctx.Err() == nil {
			h.log.Warn("commit failed", "experience", d.id, "error", err)
		}
		return
	}
	switch {
	case errors.Is(result, errStale):
		h.log.Info("stale result discarded", "experience", d.id, "reason", result)
	case result != nil:
		h.log.Error("invalid result discarded", "experience", d.id, "error", result)
	default:
		for _, send := range sends {
			h.sendClient(d, ev.actor.UUID(), send)
		}
	}
}

// apply checks snap against the world in tx and validates every op before it applies any. Then
// it applies the block and state ops in their order, the data writes, the slots and drops, and
// last the tells, and
// returns the client messages for commit to send. A position's data write follows its last block
// op, as validate checks, so applying the data writes after every block op changes nothing; a
// state op keeps the block's data and generation. Writes that shrink data go before those that
// grow it, so the Experience's total only falls and then rises to the net that validate checked,
// and no single write exceeds the quota.
func (h *Host) apply(tx *world.Tx, exp string, ev event, snap snapshot, ops []Op) ([]*SendClientOp, error) {
	actor, err := h.current(tx, exp, ev, snap)
	if err != nil {
		return nil, err
	}
	plan, err := h.validate(exp, ev, snap, ops)
	if err != nil {
		return nil, err
	}
	if err := slotsCurrent(actor, snap, plan.slots); err != nil {
		return nil, err
	}
	for _, op := range ops {
		if op.SetBlockState != nil {
			pos := op.SetBlockState.Pos.cube()
			b := tx.Block(pos).(Block)
			// validate checked the block and its states.
			if changed, _ := b.withStates(op.SetBlockState.States); changed != b {
				tx.SetBlock(pos, changed, nil)
			}
			continue
		}
		if op.SetBlock == nil {
			continue
		}
		pos := op.SetBlock.Pos.cube()
		if op.SetBlock.ID == airID {
			tx.SetBlock(pos, nil, nil)
			h.store.Remove(exp, ev.dim.storeKey(pos))
			continue
		}
		b, _ := h.reg.Lookup(op.SetBlock.ID)
		tx.SetBlock(pos, b, nil)
		h.store.Place(exp, ev.dim.storeKey(pos))
	}
	for _, grow := range []bool{false, true} {
		for _, w := range plan.writes {
			if (w.delta > 0) != grow {
				continue
			}
			if err := h.store.SetData(exp, ev.dim.storeKey(w.pos), w.data, w.present); err != nil {
				// validate checked ownership and the quota, so the store disagrees with it.
				h.log.Error("validated data write failed", "experience", exp, "pos", w.pos, "error", err)
			}
		}
	}
	for _, w := range plan.slots {
		inv, slot := slotHolder(actor.(*player.Player), w.slot)
		if err := inv.SetItem(slot, w.stack); err != nil {
			h.log.Error("validated slot write failed", "experience", exp, "slot", w.slot, "error", err)
		}
	}
	for _, drop := range plan.drops {
		tx.AddEntity(entity.NewItem(world.EntitySpawnOpts{Position: drop.pos.Vec3Centre()}, drop.stack))
	}
	var sends []*SendClientOp
	for _, op := range ops {
		switch {
		case op.Tell != nil:
			h.tell.tell(actor, op.Tell.Text)
		case op.SendClient != nil:
			sends = append(sends, op.SendClient)
		}
	}
	return sends, nil
}

// current validates retained cells and rejected network boundaries, then resolves the actor.
// A boundary that gains an owned network member makes the callback's topology stale.
func (h *Host) current(tx *world.Tx, exp string, ev event, snap snapshot) (world.Entity, error) {
	for _, was := range snap.cells {
		now := h.cellState(tx, exp, ev.dim, was.pos)
		if now.loaded != was.loaded || now.id != was.id || now.block != was.block ||
			now.hasToken != was.hasToken || now.token != was.token {
			return nil, fmt.Errorf("%w: the cell at %v changed", errStale, was.pos)
		}
	}
	for _, pos := range snap.networkBoundary {
		now := h.cellState(tx, exp, ev.dim, pos)
		if now.owned && now.block.t.network {
			return nil, fmt.Errorf("%w: network boundary at %v gained a member", errStale, pos)
		}
	}
	if ev.actor == nil {
		return nil, nil
	}
	for p := range tx.Players() {
		if p.H() == ev.actor {
			return p, nil
		}
	}
	return nil, fmt.Errorf("%w: actor %s is not in the world", errStale, ev.actor.UUID())
}

// simCell is a writable snapshot cell as the ops before the current one leave it. written is set
// once a data op has written it.
type simCell struct {
	id      string
	owned   bool
	dataLen uint64
	written bool
}

// slotWrite is a validated set_slot op: the actor's slot and the stack it gets, empty to clear
// it.
type slotWrite struct {
	slot  int
	stack item.Stack
}

// itemDrop is a validated drop_item op: the stack and where it drops.
type itemDrop struct {
	pos   cube.Pos
	stack item.Stack
}

// commitPlan is what validate found the ops to write, in their order.
type commitPlan struct {
	writes []dataWrite
	slots  []slotWrite
	drops  []itemDrop
}

// slotsCurrent checks that every slot the result writes still holds the stack its snapshot did.
func slotsCurrent(actor world.Entity, snap snapshot, slots []slotWrite) error {
	if len(slots) == 0 {
		return nil
	}
	p, ok := actor.(*player.Player)
	if !ok {
		return fmt.Errorf("%w: the actor has no inventory", errStale)
	}
	for _, w := range slots {
		inv, slot := slotHolder(p, w.slot)
		now, _ := inv.Item(slot)
		if was := snap.slots[w.slot]; !now.Equal(was) {
			return fmt.Errorf("%w: slot %d changed from %v to %v", errStale, w.slot, was, now)
		}
	}
	return nil
}

// dataWrite is a validated data op: the data it writes at pos, absent unless present, and how
// many bytes it adds to the Experience's total, negative when it frees some.
type dataWrite struct {
	pos     cube.Pos
	data    []byte
	present bool
	delta   int64
}

// validate checks every op against the snapshot as the ops before it change it, by the rules the
// runtime enforced: writes stay on loaded snapshot cells in the anchor's chunk column, or for data
// and states reach the members of its network, set air or an own block over air or an own block,
// write data and states only to an own block, within the size limit and among its states, set the
// actor's slots and drop items only with stacks the Experience may make, and tell and send client
// messages only to the actor, within the tell and client message limits. A position has at most one
// data op, after its last block op. Like the runtime, it holds the quota to the result's net data,
// not to each write. It returns the data writes, slots and drops in their order.
func (h *Host) validate(exp string, ev event, snap snapshot, ops []Op) (commitPlan, error) {
	if len(ops) > maxStagedOps {
		return commitPlan{}, fmt.Errorf("%w: %d ops, at most %d", errInvalid, len(ops), maxStagedOps)
	}
	column := func(pos cube.Pos) [2]int { return [2]int{pos[0] >> 4, pos[2] >> 4} }
	cells := make(map[cube.Pos]*simCell, len(snap.cells))
	for _, c := range snap.cells {
		if c.loaded {
			cells[c.pos] = &simCell{id: c.id, owned: c.owned, dataLen: c.dataLen}
		}
	}
	members := make(map[cube.Pos]bool)
	if snap.req.Network != nil {
		for _, pos := range snap.req.Network.Blocks {
			members[pos.cube()] = true
		}
	}
	// writable is the cell an op writes: in the anchor's chunk column, or for data and states a
	// member of its network.
	writable := func(i int, pos BlockPos, network bool) (*simCell, error) {
		p := pos.cube()
		if c, ok := cells[p]; ok && (column(p) == column(ev.anchor) || network && members[p]) {
			return c, nil
		}
		return nil, fmt.Errorf("%w: op %d writes %v, outside the write scope", errInvalid, i, pos)
	}
	var actorID string
	if ev.actor != nil {
		actorID = ev.actor.UUID().String()
	}
	// The snapshot cells are current, so the store's total counts their data.
	used := int64(dataQuota - h.store.Budget(exp))
	var plan commitPlan
	tells, sends := 0, 0
	for i, op := range ops {
		switch {
		case op.SetBlock != nil:
			c, err := writable(i, op.SetBlock.Pos, false)
			if err != nil {
				return commitPlan{}, err
			}
			if c.id != airID && !c.owned {
				return commitPlan{}, fmt.Errorf("%w: op %d sets a block over %s, which is not its own",
					errInvalid, i, c.id)
			}
			if c.written {
				return commitPlan{}, fmt.Errorf("%w: op %d sets a block whose data an earlier op wrote", errInvalid, i)
			}
			id := op.SetBlock.ID
			if b, ok := h.reg.Lookup(id); id != airID && (!ok || b.t.exp != exp) {
				return commitPlan{}, fmt.Errorf("%w: op %d sets %q, which is not its own block", errInvalid, i, id)
			}
			used -= int64(c.dataLen)
			*c = simCell{id: id, owned: id != airID}
		case op.SetBlockData != nil:
			c, err := writable(i, op.SetBlockData.Pos, true)
			if err != nil {
				return commitPlan{}, err
			}
			if !c.owned {
				return commitPlan{}, fmt.Errorf("%w: op %d writes data to %s, which is not its own block",
					errInvalid, i, c.id)
			}
			if c.written {
				return commitPlan{}, fmt.Errorf("%w: op %d writes data that an earlier op wrote", errInvalid, i)
			}
			w := dataWrite{pos: op.SetBlockData.Pos.cube(), present: op.SetBlockData.Data != nil}
			if w.present {
				if w.data, err = hex.DecodeString(*op.SetBlockData.Data); err != nil {
					return commitPlan{}, fmt.Errorf("%w: op %d data: %v", errInvalid, i, err)
				}
			}
			n := uint64(len(w.data))
			if n > maxBlockDataBytes {
				return commitPlan{}, fmt.Errorf("%w: op %d writes %d bytes of data, at most %d",
					errInvalid, i, n, maxBlockDataBytes)
			}
			w.delta = int64(n) - int64(c.dataLen)
			used += w.delta
			c.dataLen, c.written = n, true
			plan.writes = append(plan.writes, w)
		case op.Tell != nil:
			text := op.Tell.Text
			tells++
			switch {
			case ev.actor == nil || op.Tell.Player != actorID:
				return commitPlan{}, fmt.Errorf("%w: op %d tells %s, who is not the actor", errInvalid, i, op.Tell.Player)
			case tells > maxTells:
				return commitPlan{}, fmt.Errorf("%w: more than %d tells", errInvalid, maxTells)
			case len(text) > maxTellBytes:
				return commitPlan{}, fmt.Errorf("%w: op %d tells %d bytes, at most %d", errInvalid, i, len(text), maxTellBytes)
			case strings.ContainsFunc(text, func(r rune) bool { return unicode.IsControl(r) || r == formattingPrefix }):
				return commitPlan{}, fmt.Errorf("%w: op %d tells a control or formatting character", errInvalid, i)
			}
		case op.SendClient != nil:
			sends++
			switch {
			case ev.actor == nil || op.SendClient.Player != actorID:
				return commitPlan{}, fmt.Errorf("%w: op %d sends a client message to %s, who is not the actor",
					errInvalid, i, op.SendClient.Player)
			case sends > maxClientSends:
				return commitPlan{}, fmt.Errorf("%w: more than %d client messages", errInvalid, maxClientSends)
			}
		case op.SetBlockState != nil:
			c, err := writable(i, op.SetBlockState.Pos, true)
			if err != nil {
				return commitPlan{}, err
			}
			b, ok := h.reg.Lookup(c.id)
			if !c.owned || !ok || b.t.exp != exp {
				return commitPlan{}, fmt.Errorf("%w: op %d sets states of %s, which is not its own block",
					errInvalid, i, c.id)
			}
			if len(op.SetBlockState.States) == 0 {
				return commitPlan{}, fmt.Errorf("%w: op %d sets no states", errInvalid, i)
			}
			if _, err := b.withStates(op.SetBlockState.States); err != nil {
				return commitPlan{}, fmt.Errorf("%w: op %d: %v", errInvalid, i, err)
			}
		case op.SetSlot != nil:
			slot := int(op.SetSlot.Slot)
			if snap.slots == nil || slot >= inventorySlots {
				return commitPlan{}, fmt.Errorf("%w: op %d sets slot %d of no inventory", errInvalid, i, slot)
			}
			w := slotWrite{slot: slot}
			if op.SetSlot.Stack != nil {
				var err error
				if w.stack, err = h.reg.makeStack(exp, *op.SetSlot.Stack); err != nil {
					return commitPlan{}, fmt.Errorf("%w: op %d: %v", errInvalid, i, err)
				}
			}
			if slot == inventorySlots-1 && !canOffHand(w.stack) {
				return commitPlan{}, fmt.Errorf("%w: op %d puts an ineligible item in the offhand", errInvalid, i)
			}
			plan.slots = append(plan.slots, w)
		case op.DropItem != nil:
			if _, err := writable(i, op.DropItem.Pos, false); err != nil {
				return commitPlan{}, err
			}
			stack, err := h.reg.makeStack(exp, op.DropItem.Stack)
			if err != nil {
				return commitPlan{}, fmt.Errorf("%w: op %d: %v", errInvalid, i, err)
			}
			plan.drops = append(plan.drops, itemDrop{pos: op.DropItem.Pos.cube(), stack: stack})
		default:
			return commitPlan{}, fmt.Errorf("%w: op %d is empty", errInvalid, i)
		}
	}
	if used > dataQuota {
		return commitPlan{}, fmt.Errorf("%w: its data exceeds the quota by %d bytes", errInvalid, used-dataQuota)
	}
	return plan, nil
}
