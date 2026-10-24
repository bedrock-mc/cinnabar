package worldfence

import (
	"fmt"
	"math"
	"math/rand/v2"
	"slices"
	"testing"
)

// admission keeps the reference model's complete history, including retired notices.
type admission struct {
	notice Notice
	notify bool
}

// reference derives its barrier from an append-only admission history and an ack cursor.
// It never deletes admissions or maintains an implementation-style pending notice queue.
type reference struct {
	status                       string
	epoch, token, preparing, ack uint64
	world                        string
	notify, changed              bool
	history                      []admission
}

// outstanding derives unacknowledged notices from history rather than cached fence flags.
func (m *reference) outstanding() []Notice {
	var result []Notice
	for _, a := range m.history {
		if a.notify && a.notice.Revision > m.ack {
			result = append(result, a.notice)
		}
	}
	return result
}

// latestRequest finds the greatest published request even after it was acknowledged.
func (m *reference) latestRequest() uint64 {
	var latest uint64
	for _, a := range m.history {
		latest = max(latest, a.notice.RequestedEpoch)
	}
	return latest
}

// publish assigns requests to admissions that were committed before activation.
func (m *reference) publish() []Notice {
	var notices []Notice
	if m.status == "active" {
		for i, a := range m.history {
			if a.notify && a.notice.RequestedEpoch == 0 {
				m.history[i].notice.RequestedEpoch = max(m.epoch, m.latestRequest()) + 1
				notices = append(notices, m.history[i].notice)
			}
		}
	}
	return notices
}

// allows derives application authority from the reference history and preparation.
func (m *reference) allows() bool {
	return m.status == "active" && m.preparing == 0 && len(m.outstanding()) == 0
}

// event supplies a deterministic state-machine operation and its arguments.
type event struct {
	kind         string
	token, epoch uint64
	world        string
}

// apply executes the small specification over history and returns observable outputs.
func (m *reference) apply(e event) (notices []Notice, err bool) {
	switch e.kind {
	case "prepare", "initial":
		if m.status == "disconnected" || m.preparing != 0 {
			return nil, true
		}
		m.token++
		m.preparing, m.notify = m.token, e.kind != "initial"
	case "commit", "change", "publish_fails":
		notify := m.notify
		if e.kind != "change" {
			if m.preparing == 0 || e.token != m.preparing {
				return nil, true
			}
			m.preparing, m.notify = 0, false
		} else {
			if m.status != "active" || m.preparing != 0 {
				return nil, true
			}
			notify = true
		}
		if e.world == "" {
			return nil, true
		}
		if e.kind == "change" && e.world == m.world {
			return nil, false
		}
		m.world = e.world
		notify = notify && m.status != "disabled"
		m.history = append(m.history, admission{notice: Notice{Revision: uint64(len(m.history)) + 1}, notify: notify})
		if notify {
			m.changed = false
		}
		notices = m.publish()
		if e.kind == "publish_fails" {
			m.apply(event{kind: "disable"})
			return nil, false
		}
		return notices, false
	case "abort":
		if e.token == m.preparing {
			m.preparing, m.notify = 0, false
		}
	case "activate":
		if m.status != "pending" {
			return nil, true
		}
		m.status, m.epoch = "active", e.epoch
		return m.publish(), false
	case "ack":
		if m.status != "active" || e.token == 0 || e.token > m.latestRequest() || e.epoch < e.token {
			return nil, true
		}
		for _, a := range m.history {
			if a.notify && a.notice.RequestedEpoch == e.token {
				if a.notice.Revision <= m.ack {
					return nil, false
				}
				if e.epoch <= m.epoch {
					return nil, true
				}
				m.ack, m.epoch, m.changed = a.notice.Revision, e.epoch, true
				return nil, false
			}
		}
		// Requests below an acknowledged cursor are harmless stale controls,
		// even when their number fell between two issued requests.
		for _, a := range m.history {
			if a.notice.Revision <= m.ack && a.notice.RequestedEpoch >= e.token {
				return nil, false
			}
		}
		return nil, true
	case "advance":
		if m.status != "active" || e.epoch <= m.epoch {
			return nil, true
		}
		m.epoch, m.changed = e.epoch, true
	case "disable":
		if m.status != "disconnected" {
			m.status, m.changed = "disabled", false
			for i := range m.history {
				m.history[i].notify = false
			}
		}
	case "disconnect":
		*m = reference{status: "disconnected"}
	case "envelope", "tick", "timer":
	default:
		panic(e.kind)
	}
	return nil, false
}

// comparison drives the real machine and a separately represented specification.
type comparison struct {
	fence Fence[string]
	model reference
	trace []event
}

// step compares outputs, preparation ownership and traffic authority after every event.
func (c *comparison) step(t *testing.T, e event) {
	t.Helper()
	c.trace = append(c.trace, e)
	var notices []Notice
	var err error
	switch e.kind {
	case "prepare", "initial":
		var token uint64
		token, err = c.fence.Prepare(e.kind != "initial")
		if err == nil && token != c.model.token+1 {
			t.Fatalf("preparation token %d after %+v", token, c.trace)
		}
	case "commit":
		notices, err = c.fence.Commit(e.token, e.world)
	case "publish_fails":
		_, err = c.fence.Commit(e.token, e.world)
		if err == nil {
			c.fence.Disable()
		}
	case "change":
		notices, err = c.fence.Change(e.world)
	case "abort":
		c.fence.Abort(e.token)
	case "activate":
		notices, err = c.fence.Activate(e.epoch)
	case "ack":
		err = c.fence.Acknowledge(e.token, e.epoch)
	case "advance":
		err = c.fence.Advance(e.epoch)
	case "disable":
		c.fence.Disable()
	case "disconnect":
		c.fence.Disconnect()
	case "envelope", "tick", "timer":
		if got, want := c.fence.Accepts(e.epoch), c.model.allows() && e.epoch == c.model.epoch; got != want {
			t.Fatalf("envelope accepted=%t want=%t after %+v", got, want, c.trace)
		}
	}
	wantNotices, wantError := c.model.apply(e)
	if (err != nil) != wantError || !slices.Equal(notices, wantNotices) {
		t.Fatalf("notices=%v err=%v, want %v error=%t after %+v", notices, err, wantNotices, wantError, c.trace)
	}
	m := &c.model
	if c.fence.Epoch() != m.epoch || c.fence.AllowsTraffic() != m.allows() || c.fence.HasPreparation() != (m.preparing != 0) ||
		c.fence.RequiresNotices() != (len(m.outstanding()) != 0 || m.preparing != 0 && m.notify) {
		t.Fatalf("barrier disagrees with reference %+v after %+v", m, c.trace)
	}
	for token := uint64(0); token <= m.token+1; token++ {
		if c.fence.Preparing(token) != (token != 0 && token == m.preparing) {
			t.Fatalf("preparation ownership disagrees after %+v", c.trace)
		}
	}
	for epoch := uint64(0); epoch <= max(m.epoch, m.latestRequest())+1; epoch++ {
		outstanding := slices.ContainsFunc(m.outstanding(), func(n Notice) bool { return n.RequestedEpoch == epoch && epoch != 0 })
		if c.fence.Outstanding(epoch) != outstanding || c.fence.Accepts(epoch) != (m.allows() && epoch == m.epoch) {
			t.Fatalf("epoch %d authority disagrees after %+v", epoch, c.trace)
		}
	}
	changed := m.changed && m.allows()
	if c.fence.TakeChanged() != changed || c.fence.TakeChanged() {
		t.Fatalf("epoch callback disagrees after %+v", c.trace)
	}
	if changed {
		m.changed = false
	}
}

// selectEvent constructs valid and stale operations from reference history only.
func (c *comparison) selectEvent(kind int, value uint64) event {
	m := &c.model
	e := event{token: m.preparing, epoch: m.epoch + 1, world: fmt.Sprintf("world:%d", value%3)}
	switch kind {
	case 0:
		e.kind = "prepare"
	case 1:
		e.kind = "initial"
	case 2:
		e.kind = "commit"
	case 3:
		e.kind = "abort"
	case 4:
		e.kind, e.epoch = "activate", value%13
	case 5, 6:
		e.kind = "ack"
		notices := m.outstanding()
		if len(notices) != 0 {
			i := 0
			if kind == 6 {
				i = len(notices) - 1
			}
			e.token = notices[i].RequestedEpoch
		}
		e.epoch = max(m.epoch, e.token) + 1
	case 7:
		e.kind = "ack"
		for _, a := range m.history {
			if a.notice.Revision <= m.ack && a.notice.RequestedEpoch != 0 {
				e.token, e.epoch = a.notice.RequestedEpoch, a.notice.RequestedEpoch
				break
			}
		}
	case 8:
		e.kind, e.token = "ack", m.latestRequest()+1
		e.epoch = max(m.epoch, e.token) + 1
	case 9:
		e.kind, e.epoch = "envelope", value%(m.epoch+2)
	case 10:
		e.kind = "advance"
	case 11:
		e.kind = "change"
	case 12:
		e.kind = "publish_fails"
	case 13:
		e.kind = "disable"
	case 14:
		e.kind = "disconnect"
	case 15:
		e.kind, e.epoch = "tick", value%(m.epoch+2)
	case 16:
		e.kind, e.epoch = "timer", value%(m.epoch+2)
	}
	return e
}

func TestFenceRandomSequences(t *testing.T) {
	rng := rand.New(rand.NewPCG(0x5e7e2, 0xfece))
	for sequence := range 4096 {
		t.Run(fmt.Sprint(sequence), func(t *testing.T) {
			c := comparison{model: reference{status: "pending"}}
			for step := range 96 {
				kind := rng.IntN(15)
				if kind >= 13 {
					kind += 2
				}
				if step >= 72 {
					kind = rng.IntN(17)
				}
				e := c.selectEvent(kind, rng.Uint64())
				if (e.kind == "abort" || e.kind == "commit") && rng.IntN(4) == 0 {
					e.token = uint64(rng.IntN(int(c.model.token) + 2))
				}
				if e.kind == "commit" && rng.IntN(32) == 0 {
					e.world = ""
				}
				c.step(t, e)
			}
		})
	}
}

func TestFenceAllShortSequences(t *testing.T) {
	// Enumerate every length-four sequence over the core lifecycle events.
	for sequence := range 15 * 15 * 15 * 15 {
		c := comparison{model: reference{status: "pending"}}
		choices := sequence
		for range 4 {
			kind := choices % 15
			if kind >= 13 {
				kind += 2
			}
			e := c.selectEvent(kind, 1)
			choices /= 15
			c.step(t, e)
		}
	}
}

func TestFenceExhaustion(t *testing.T) {
	for _, exhausted := range []string{"request", "revision", "preparation"} {
		t.Run(exhausted, func(t *testing.T) {
			var f Fence[string]
			epoch := uint64(1)
			if exhausted == "request" {
				epoch = math.MaxUint64
			}
			if _, err := f.Activate(epoch); err != nil {
				t.Fatal(err)
			}
			if exhausted == "preparation" {
				f.preparationID = math.MaxUint64
				if _, err := f.Prepare(true); err == nil {
					t.Fatal("preparation token wrapped")
				}
				return
			}
			if exhausted == "revision" {
				f.revision = math.MaxUint64
			}
			token, err := f.Prepare(true)
			if err != nil {
				t.Fatal(err)
			}
			if _, err := f.Commit(token, "world"); err == nil {
				t.Fatal("exhausted admission succeeded")
			}
			f.Disable()
			if f.Accepts(epoch) {
				t.Fatal("exhausted optional part retained authority")
			}
		})
	}
}
