package experience

import (
	"context"
	"encoding/hex"
	"errors"
	"fmt"
	"strings"
	"unicode"

	"github.com/df-mc/dragonfly/server/block/cube"
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
// it applies the block ops in their order, the data writes, and last the tells, and returns the
// client messages for commit to send. A position's data write follows its last block op, as
// validate checks, so applying the data writes after every block op changes nothing. Writes that
// shrink data go before those that grow it, so the Experience's total only falls and then rises
// to the net that validate checked, and no single write exceeds the quota.
func (h *Host) apply(tx *world.Tx, exp string, ev event, snap snapshot, ops []Op) ([]*SendClientOp, error) {
	actor, err := h.current(tx, exp, ev, snap)
	if err != nil {
		return nil, err
	}
	writes, err := h.validate(exp, ev, snap, ops)
	if err != nil {
		return nil, err
	}
	for _, op := range ops {
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
		for _, w := range writes {
			if (w.delta > 0) != grow {
				continue
			}
			if err := h.store.SetData(exp, ev.dim.storeKey(w.pos), w.data, w.present); err != nil {
				// validate checked ownership and the quota, so the store disagrees with it.
				h.log.Error("validated data write failed", "experience", exp, "pos", w.pos, "error", err)
			}
		}
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

// current checks that every snapshot cell still has its loaded state, block id and store token,
// and that the event's actor, if any, is a player in the world. It returns the actor's entity.
func (h *Host) current(tx *world.Tx, exp string, ev event, snap snapshot) (world.Entity, error) {
	for _, was := range snap.cells {
		now := h.cellState(tx, exp, ev.dim, was.pos)
		if now.loaded != was.loaded || now.id != was.id || now.hasToken != was.hasToken ||
			now.token != was.token {
			return nil, fmt.Errorf("%w: the cell at %v changed", errStale, was.pos)
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

// dataWrite is a validated data op: the data it writes at pos, absent unless present, and how
// many bytes it adds to the Experience's total, negative when it frees some.
type dataWrite struct {
	pos     cube.Pos
	data    []byte
	present bool
	delta   int64
}

// validate checks every op against the snapshot as the ops before it change it, by the rules
// the runtime enforced: writes stay in the anchor's chunk column on loaded snapshot cells, set
// air or an own block over air or an own block, write data only to an own block within the size
// limit, and tell and send client messages only to the actor, within the tell and client message
// limits. A position has at most one data op, after its last block op. Like the runtime, it holds
// the quota to the result's net data, not to each write. It returns the data writes in their
// order.
func (h *Host) validate(exp string, ev event, snap snapshot, ops []Op) ([]dataWrite, error) {
	if len(ops) > maxStagedOps {
		return nil, fmt.Errorf("%w: %d ops, at most %d", errInvalid, len(ops), maxStagedOps)
	}
	column := func(pos cube.Pos) [2]int { return [2]int{pos[0] >> 4, pos[2] >> 4} }
	cells := make(map[cube.Pos]*simCell, len(snap.cells))
	for _, c := range snap.cells {
		if c.loaded && column(c.pos) == column(ev.anchor) {
			cells[c.pos] = &simCell{id: c.id, owned: c.owned, dataLen: c.dataLen}
		}
	}
	writable := func(i int, pos BlockPos) (*simCell, error) {
		if c, ok := cells[pos.cube()]; ok {
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
	var writes []dataWrite
	tells, sends := 0, 0
	for i, op := range ops {
		switch {
		case op.SetBlock != nil:
			c, err := writable(i, op.SetBlock.Pos)
			if err != nil {
				return nil, err
			}
			if c.id != airID && !c.owned {
				return nil, fmt.Errorf("%w: op %d sets a block over %s, which is not its own",
					errInvalid, i, c.id)
			}
			if c.written {
				return nil, fmt.Errorf("%w: op %d sets a block whose data an earlier op wrote", errInvalid, i)
			}
			id := op.SetBlock.ID
			if b, ok := h.reg.Lookup(id); id != airID && (!ok || b.t.exp != exp) {
				return nil, fmt.Errorf("%w: op %d sets %q, which is not its own block", errInvalid, i, id)
			}
			used -= int64(c.dataLen)
			*c = simCell{id: id, owned: id != airID}
		case op.SetBlockData != nil:
			c, err := writable(i, op.SetBlockData.Pos)
			if err != nil {
				return nil, err
			}
			if !c.owned {
				return nil, fmt.Errorf("%w: op %d writes data to %s, which is not its own block",
					errInvalid, i, c.id)
			}
			if c.written {
				return nil, fmt.Errorf("%w: op %d writes data that an earlier op wrote", errInvalid, i)
			}
			w := dataWrite{pos: op.SetBlockData.Pos.cube(), present: op.SetBlockData.Data != nil}
			if w.present {
				if w.data, err = hex.DecodeString(*op.SetBlockData.Data); err != nil {
					return nil, fmt.Errorf("%w: op %d data: %v", errInvalid, i, err)
				}
			}
			n := uint64(len(w.data))
			if n > maxBlockDataBytes {
				return nil, fmt.Errorf("%w: op %d writes %d bytes of data, at most %d",
					errInvalid, i, n, maxBlockDataBytes)
			}
			w.delta = int64(n) - int64(c.dataLen)
			used += w.delta
			c.dataLen, c.written = n, true
			writes = append(writes, w)
		case op.Tell != nil:
			text := op.Tell.Text
			tells++
			switch {
			case ev.actor == nil || op.Tell.Player != actorID:
				return nil, fmt.Errorf("%w: op %d tells %s, who is not the actor", errInvalid, i, op.Tell.Player)
			case tells > maxTells:
				return nil, fmt.Errorf("%w: more than %d tells", errInvalid, maxTells)
			case len(text) > maxTellBytes:
				return nil, fmt.Errorf("%w: op %d tells %d bytes, at most %d", errInvalid, i, len(text), maxTellBytes)
			case strings.ContainsFunc(text, func(r rune) bool { return unicode.IsControl(r) || r == formattingPrefix }):
				return nil, fmt.Errorf("%w: op %d tells a control or formatting character", errInvalid, i)
			}
		case op.SendClient != nil:
			sends++
			switch {
			case ev.actor == nil || op.SendClient.Player != actorID:
				return nil, fmt.Errorf("%w: op %d sends a client message to %s, who is not the actor",
					errInvalid, i, op.SendClient.Player)
			case sends > maxClientSends:
				return nil, fmt.Errorf("%w: more than %d client messages", errInvalid, maxClientSends)
			}
		default:
			return nil, fmt.Errorf("%w: op %d is empty", errInvalid, i)
		}
	}
	if used > dataQuota {
		return nil, fmt.Errorf("%w: its data exceeds the quota by %d bytes", errInvalid, used-dataQuota)
	}
	return writes, nil
}
