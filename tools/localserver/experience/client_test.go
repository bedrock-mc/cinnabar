package experience

import (
	"context"
	"reflect"
	"slices"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/google/uuid"
)

// More probe behaviors, selected like those in helpers_test.go by the x of the interacted block.
const (
	// probeSend counts like probeCount, sends the count on probeCounterChannel, schema 1, and
	// tells "count <n> <outcome of the send>".
	probeSend = 19
	// probeCounterChannel is the client channel that probeSend and probeTrap send on.
	probeCounterChannel = "probe.counter"
)

// declaredChannel is a channel that a client part declares to the client.
type declaredChannel struct {
	exp, channel string
	schema       uint16
}

// sentMessage is one call of fakeChannels.Send. seen is what observe returned at the time.
type sentMessage struct {
	player       uuid.UUID
	exp, channel string
	schema       uint16
	payload      []Scalar
	sent         bool
	seen         int
}

// fakeChannels is a ClientChannels whose client parts declare exactly declared, and are active
// when active is set. It records every Send.
type fakeChannels struct {
	declared []declaredChannel
	active   bool
	// observe, when set, runs at each Send, which records its result.
	observe func() int

	mu       sync.Mutex
	messages []sentMessage
}

func (c *fakeChannels) Declares(exp, channel string, schema uint16) bool {
	return slices.Contains(c.declared, declaredChannel{exp, channel, schema})
}

func (c *fakeChannels) Send(player uuid.UUID, exp, channel string, schema uint16, payload []Scalar) bool {
	m := sentMessage{player: player, exp: exp, channel: channel, schema: schema, payload: payload, sent: c.active}
	if c.observe != nil {
		m.seen = c.observe()
	}
	c.mu.Lock()
	defer c.mu.Unlock()
	c.messages = append(c.messages, m)
	return c.active
}

// all returns every Send so far.
func (c *fakeChannels) all() []sentMessage {
	c.mu.Lock()
	defer c.mu.Unlock()
	return slices.Clone(c.messages)
}

// waitSent waits until Send has been called n times and returns those calls.
func (c *fakeChannels) waitSent(t *testing.T, n int) []sentMessage {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for {
		if got := c.all(); len(got) >= n {
			if len(got) > n {
				t.Fatalf("%d sends, want %d: %+v", len(got), n, got)
			}
			return got
		}
		if time.Now().After(deadline) {
			t.Fatalf("sends after 5s = %+v, want %d", c.all(), n)
		}
		time.Sleep(5 * time.Millisecond)
	}
}

func integer(v int64) Scalar { return Scalar{Integer: &v} }

// assertMessage checks one Send of payload on channel, schema, to the fixture's actor.
func (f *hostFixture) assertMessage(m sentMessage, channel string, schema uint16, payload []Scalar) {
	f.t.Helper()
	if m.player != f.actor.UUID() || m.exp != "probe" || m.channel != channel || m.schema != schema ||
		!reflect.DeepEqual(m.payload, payload) {
		f.t.Fatalf("sent %s %s %s %d %s, want %s probe %s %d %s", m.player, m.exp, m.channel, m.schema,
			jsonOf(m.payload), f.actor.UUID(), channel, schema, jsonOf(payload))
	}
}

// dropCountOf reads a dispatcher counter.
func dropCountOf(c *dropCount) uint64 {
	return c.n.Load()
}

// A staged client message goes out only once its result has committed, tells included, to the
// actor and on the channel the guest named.
func TestStagedSendGoesOutAfterCommit(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	channels := &fakeChannels{declared: []declaredChannel{{"probe", probeCounterChannel, 1}}, active: true}
	f := newClientFixture(t, log, map[string]*Supervisor{"probe": sup}, channels)
	channels.observe = func() int { return len(f.tells.all()) }
	pos := probePos(probeSend)
	f.place(probeCounter, pos, nil)
	f.activate(pos, pos)
	f.waitTells(5*time.Second, "count 1 ok", "count 2 ok")
	for i, m := range channels.waitSent(t, 2) {
		f.assertMessage(m, probeCounterChannel, 1, []Scalar{integer(int64(i + 1))})
		if m.seen != i+1 {
			t.Fatalf("message %d went out after %d tells, want after its own commit's", i+1, m.seen)
		}
	}
	f.assertData("probe", pos, counter(2))
}

// A trapped callback's staged message is never sent; the next callback's is.
func TestTrapSendsNothing(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	channels := &fakeChannels{declared: []declaredChannel{{"probe", probeCounterChannel, 1}}, active: true}
	f := newClientFixture(t, log, map[string]*Supervisor{"probe": sup}, channels)
	trap, send := probePos(probeTrap), probePos(probeSend)
	f.place(probeCounter, trap, nil)
	f.place(probeCounter, send, nil)
	f.activate(trap, send)
	f.waitTells(5*time.Second, "count 1 ok")
	f.assertMessage(channels.waitSent(t, 1)[0], probeCounterChannel, 1, []Scalar{integer(1)})
}

// A stale result sends nothing.
func TestStaleResultSendsNothing(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startProbe(t, log)
	channels := &fakeChannels{declared: []declaredChannel{{"probe", probeCounterChannel, 1}}, active: true}
	f := newClientFixture(t, log, map[string]*Supervisor{"probe": sup}, channels)
	pos := probePos(probeSend)
	f.place(probeCounter, pos, nil)
	f.host.afterCall = func() {
		if err := f.store.SetData("probe", overworldKey(pos), []byte{9}, true); err != nil {
			t.Errorf("SetData: %v", err)
		}
	}
	f.activate(pos)
	waitStale(t, logs, "changed")
	if sent := channels.all(); len(sent) != 0 {
		t.Fatalf("a stale result sent %+v", sent)
	}
}

// A message on a channel that the Experience's client part does not declare is dropped and
// counted; the rest of the result commits.
func TestUndeclaredChannelIsDroppedAndCounted(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startProbe(t, log)
	channels := &fakeChannels{declared: []declaredChannel{{"probe", probeCounterChannel, 2}}, active: true}
	f := newClientFixture(t, log, map[string]*Supervisor{"probe": sup}, channels)
	pos := probePos(probeSend)
	f.place(probeCounter, pos, nil)
	f.activate(pos)
	f.waitTells(5*time.Second, "count 1 ok")
	waitForRecord(t, logs, func(r map[string]any) bool {
		return r["msg"] == "client messages on undeclared channels dropped" && r["experience"] == "probe"
	})
	if sent := channels.all(); len(sent) != 0 {
		t.Fatalf("sent %+v on an undeclared channel", sent)
	}
	if n := dropCountOf(&f.host.dispatchers["probe"].undeclared); n != 1 {
		t.Fatalf("undeclared = %d, want 1", n)
	}
	f.assertData("probe", pos, counter(1))
}

// A message to a player without an active client part, or without any server half, is dropped
// and counted; the rest of the result commits.
func TestUnsentMessageIsCounted(t *testing.T) {
	inactive := &fakeChannels{declared: []declaredChannel{{"probe", probeCounterChannel, 1}}}
	for name, channels := range map[string]ClientChannels{"inactive": inactive, "no server half": nil} {
		t.Run(name, func(t *testing.T) {
			log, _ := testLog(t)
			sup, _ := startProbe(t, log)
			f := newClientFixture(t, log, map[string]*Supervisor{"probe": sup}, channels)
			pos := probePos(probeSend)
			f.place(probeCounter, pos, nil)
			f.activate(pos)
			f.waitTells(5*time.Second, "count 1 ok")
			d := f.host.dispatchers["probe"]
			deadline := time.Now().Add(5 * time.Second)
			for dropCountOf(&d.unsent) == 0 {
				if time.Now().After(deadline) {
					t.Fatal("no unsent message counted within 5s")
				}
				time.Sleep(5 * time.Millisecond)
			}
			if n := dropCountOf(&d.unsent); n != 1 {
				t.Fatalf("unsent = %d, want 1", n)
			}
			f.assertData("probe", pos, counter(1))
		})
	}
}

// The commit check holds client messages to the runtime's rules again: only to the actor, and at
// most maxClientSends. A result that breaks them commits and sends nothing; exactly the cap
// commits.
func TestInvalidSendsDiscardResult(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	channels := &fakeChannels{declared: []declaredChannel{{"probe", probeCounterChannel, 1}}, active: true}
	f := newIdleClientFixture(t, log, map[string]*Supervisor{"probe": sup}, channels)
	t.Cleanup(func() { f.host.Close() })
	pos := probePos(probeCount)
	f.place(probeCounter, pos, nil)
	actor := f.actorID()
	d := f.host.dispatchers["probe"]
	// commit commits ops as the result of the actor's interaction with pos; commits finish
	// before they return.
	commit := func(ops []Op) {
		ev := event{w: f.w, dim: dimension{num: 0, id: "overworld"}, anchor: pos.cube(), actor: f.actor,
			call: Call{Interact: &InteractCall{Player: actor, Pos: pos, Face: FaceUp}}}
		snap, err := f.host.snapshot(context.Background(), d, &ev)
		if err != nil {
			t.Fatalf("snapshot: %v", err)
		}
		f.host.commit(context.Background(), d, ev, snap, ops)
	}
	invalid := func() int {
		n := 0
		for _, r := range logs.records(t) {
			err, _ := r["error"].(string)
			if r["msg"] == "invalid result discarded" && strings.Contains(err, "client message") {
				n++
			}
		}
		return n
	}
	send := func(player string) Op {
		return Op{SendClient: &SendClientOp{Player: player, Channel: probeCounterChannel, Schema: 1}}
	}
	tell := Op{Tell: &TellOp{Player: actor, Text: "committed"}}
	cases := [][]Op{
		{tell, send("00000000-0000-0000-0000-000000000000")},
		append([]Op{tell}, slices.Repeat([]Op{send(actor)}, maxClientSends+1)...),
	}
	for i, ops := range cases {
		commit(ops)
		if n := invalid(); n != i+1 {
			t.Fatalf("case %d: %d invalid results discarded, want %d", i, n, i+1)
		}
	}
	if tells, sent := f.tells.all(), channels.all(); len(tells) != 0 || len(sent) != 0 {
		t.Fatalf("invalid results told %+v and sent %+v", tells, sent)
	}
	commit(slices.Repeat([]Op{send(actor)}, maxClientSends))
	channels.waitSent(t, maxClientSends)
}

// A client message reaches the guest's client-message on the sender's world, without access to
// any block, and what it stages commits like any result: here a tell and an echo.
func TestClientMessageReachesGuest(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startProbe(t, log)
	channels := &fakeChannels{declared: []declaredChannel{{"probe", "probe.echo", 7}}, active: true}
	f := newClientFixture(t, log, map[string]*Supervisor{"probe": sup}, channels)
	yes, text, choice := true, "ack", uint16(3)
	payload := []Scalar{{Bool: &yes}, integer(-42), {Text: &text}, {Choice: &choice}}
	if !f.host.DeliverClientMessage(f.actor, "probe", "probe.echo", 7, payload) {
		t.Fatal("the client message was not queued")
	}
	f.waitTells(5*time.Second, "client probe.echo 7 4 read denied write denied echo ok")
	f.assertMessage(channels.waitSent(t, 1)[0], "probe.echo", 7, payload)
}

// A client message for an Experience the Host does not run is refused, and so is one after Close.
func TestClientMessageRefusedWithoutExperience(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	if f.host.DeliverClientMessage(f.actor, "other", "other.echo", 1, nil) {
		t.Fatal("a message for an Experience the Host does not run was queued")
	}
	f.host.Close()
	if f.host.DeliverClientMessage(f.actor, "probe", "probe.echo", 1, nil) {
		t.Fatal("a message was queued after Close")
	}
}

// A client message whose sender leaves the world before the commit is stale: no tell, no send.
func TestClientMessageSenderMustStay(t *testing.T) {
	log, logs := testLog(t)
	sup, _ := startProbe(t, log)
	channels := &fakeChannels{declared: []declaredChannel{{"probe", "probe.echo", 1}}, active: true}
	f := newClientFixture(t, log, map[string]*Supervisor{"probe": sup}, channels)
	f.host.afterCall = func() {
		f.do(func(tx *world.Tx) { tx.RemoveEntity(f.player(tx)) })
	}
	if !f.host.DeliverClientMessage(f.actor, "probe", "probe.echo", 1, nil) {
		t.Fatal("the client message was not queued")
	}
	waitStale(t, logs, "is not in the world")
	if tells, sent := f.tells.all(), channels.all(); len(tells) != 0 || len(sent) != 0 {
		t.Fatalf("a stale client message told %+v and sent %+v", tells, sent)
	}
}
