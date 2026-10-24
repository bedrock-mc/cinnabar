package cinnabarext

import (
	"testing"

	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

// TestPreparedWorldChangeFencesAndAborts preserves ordinary traffic without a committed transfer.
func TestPreparedWorldChangeFencesAndAborts(t *testing.T) {
	f, accept := readyFixture(t)
	change, err := f.s.PrepareWorldChange(f.player)
	if err != nil {
		t.Fatal(err)
	}
	if f.sendToClient(1) {
		t.Fatal("prepared connection sent application data")
	}
	f.deliver(f.envelope(accept, 1, nil))
	if len(f.messages()) != 0 {
		t.Fatal("prepared connection delivered old-world input")
	}
	change.Abort()
	if !f.sendToClient(1) {
		t.Fatal("aborted preparation did not restore sends")
	}
	f.deliver(f.envelope(accept, 2, nil))
	if len(f.messages()) != 1 {
		t.Fatal("aborted preparation did not restore input")
	}
	if change.Commit("sample:late") {
		t.Fatal("aborted transfer committed")
	}
}

// TestPreparedWorldChangeCommitsThroughExistingAck requires a correlated notice-specific reply.
func TestPreparedWorldChangeCommitsThroughExistingAck(t *testing.T) {
	f, _ := readyFixture(t)
	epochs := 0
	f.s.OnEpoch(func(uuid.UUID, PlayerSession, string) { epochs++ })
	change, err := f.s.PrepareWorldChange(f.player)
	if err != nil {
		t.Fatal(err)
	}
	if !change.Commit("sample:destination") || f.sendToClient(1) {
		t.Fatal("commit did not retain its acknowledgment fence")
	}
	if change.Commit("sample:duplicate") {
		t.Fatal("prepared transfer committed twice")
	}
	change.Abort()
	if f.sendToClient(1) {
		t.Fatal("abort cleared a committed world notice")
	}
	notice := f.lastWorldNotice()
	f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
	if epochs != 1 || !f.sendToClient(1) {
		t.Fatal("correlated acknowledgment did not publish the new epoch")
	}
}

// TestAbortedPreparationPreservesEarlierNotice never opens an older transfer's pending fence.
func TestAbortedPreparationPreservesEarlierNotice(t *testing.T) {
	f, _ := readyFixture(t)
	if !f.s.WorldChanged(f.player, "sample:first") {
		t.Fatal("first notice failed")
	}
	change, err := f.s.PrepareWorldChange(f.player)
	if err != nil {
		t.Fatal(err)
	}
	change.Abort()
	if f.sendToClient(1) {
		t.Fatal("abort opened an earlier pending notice")
	}
	notice := f.lastWorldNotice()
	f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
	if !f.sendToClient(1) {
		t.Fatal("earlier notice could not complete")
	}
}

// TestAbortedPreparationReplaysReadyEpoch delivers acknowledgments received during preparation.
func TestAbortedPreparationReplaysReadyEpoch(t *testing.T) {
	f, _ := readyFixture(t)
	epochs := 0
	f.s.OnEpoch(func(uuid.UUID, PlayerSession, string) { epochs++ })
	if !f.s.WorldChanged(f.player, "sample:first") {
		t.Fatal("first notice failed")
	}
	notice := f.lastWorldNotice()
	change, err := f.s.PrepareWorldChange(f.player)
	if err != nil {
		t.Fatal(err)
	}
	f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
	if epochs != 0 || f.sendToClient(1) {
		t.Fatal("acknowledgment bypassed preparation")
	}
	change.Abort()
	if epochs != 1 || !f.sendToClient(1) {
		t.Fatal("abort lost the acknowledged source epoch")
	}
}

// TestPreparedWorldChangeWithoutClientPartNeedsNoControl preserves ordinary clients.
func TestPreparedWorldChangeWithoutClientPartNeedsNoControl(t *testing.T) {
	f, _ := readyFixture(t)
	change, err := f.s.PrepareWorldChange(uuid.New())
	if err != nil || !change.Commit("sample:ordinary") {
		t.Fatal("ordinary client required an extension notice")
	}
	change.Abort()
}

// TestPreparedWorldChangeRejectsV1 refuses unsupported optional runtimes before detach.
func TestPreparedWorldChangeRejectsV1(t *testing.T) {
	f, _ := readyFixture(t)
	conn := f.conn.(*Conn)
	conn.mu.Lock()
	conn.wire = v1Wire
	conn.mu.Unlock()
	if _, err := f.s.PrepareWorldChange(f.player); err == nil {
		t.Fatal("v1 part prepared a v2-only transition")
	}
}

// TestHandshakeDuringTransfer joins the same fence even when preparation preceded Ready.
func TestHandshakeDuringTransfer(t *testing.T) {
	for _, when := range []string{"before_prepare", "after_prepare", "after_commit"} {
		t.Run(when, func(t *testing.T) {
			f := newConn(t, nil)
			accept := f.accept(f.hello())
			ready := func() { r := f.ready(accept); f.deliver(f.encode(Control{Ready: &r})) }
			if when == "before_prepare" {
				ready()
			}
			change, err := f.s.PrepareWorldChange(f.player)
			if err != nil {
				t.Fatal(err)
			}
			if when == "after_prepare" {
				ready()
			}
			if when != "after_commit" {
				f.deliver(f.envelope(accept, 1, nil))
				if f.sendToClient(1) || len(f.messages()) != 0 {
					t.Fatal("activation escaped preparation")
				}
			}
			if !change.Commit("sample:destination") {
				t.Fatal("commit failed")
			}
			if when == "after_commit" {
				ready()
			}
			sequence := uint64(2)
			if when == "after_commit" {
				sequence = 1
			}
			f.deliver(f.envelope(accept, sequence, nil))
			if f.sendToClient(1) || len(f.messages()) != 0 {
				t.Fatal("old epoch escaped destination fence")
			}
			notice := f.lastWorldNotice()
			f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
			if !f.sendToClient(1) {
				t.Fatal("destination acknowledgment did not restore traffic")
			}
		})
	}
}

// TestReconnectAfterCommittedTransferRetiresWorldState admits wire v1 in a new connection for the same player.
func TestReconnectAfterCommittedTransferRetiresWorldState(t *testing.T) {
	f, _ := readyFixture(t)
	change, err := f.s.PrepareWorldChange(f.player)
	if err != nil || !change.Commit("sample:destination") {
		t.Fatalf("commit: %v", err)
	}
	if err := f.conn.Close(); err != nil {
		t.Fatal(err)
	}
	f.inner = &fakeConn{identity: f.inner.identity}
	f.listener.conns = append(f.listener.conns, f.inner)
	wrapped := &listener{Listener: f.listener, s: f.s}
	f.conn, err = wrapped.Accept()
	if err != nil {
		t.Fatal(err)
	}
	if err := f.conn.WritePacket(&packet.ItemRegistry{}); err != nil {
		t.Fatal(err)
	}
	hello := f.hello()
	hello.Wire = nil
	accept := f.accept(hello)
	ready := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &ready}))
	if !f.sendToClient(1) {
		t.Fatal("new wire-v1 session inherited the previous transfer")
	}
}

// TestOldSessionCannotRetireReconnect keeps connection ownership exact across overlapping logins.
func TestOldSessionCannotRetireReconnect(t *testing.T) {
	f, _ := readyFixture(t)
	old := f.conn.(*Conn)
	change, err := old.PrepareWorldChange(CoordinatedChange)
	if err != nil {
		t.Fatal(err)
	}
	f.inner = &fakeConn{identity: f.inner.identity}
	f.listener.conns = append(f.listener.conns, f.inner)
	wrapped := &listener{Listener: f.listener, s: f.s}
	f.conn, err = wrapped.Accept()
	if err != nil {
		t.Fatal(err)
	}
	if err := f.conn.WritePacket(&packet.ItemRegistry{}); err != nil {
		t.Fatal(err)
	}
	hello := f.hello()
	hello.Wire = nil
	accept := f.accept(hello)
	ready := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &ready}))
	old.Retire()
	if change.Commit("sample:retired") {
		t.Fatal("retired session committed into its reconnect")
	}
	if f.s.PlayerSession(f.player) != f.conn.(*Conn) || !f.sendToClient(1) {
		t.Fatal("old disconnect removed the new connection")
	}
}

// TestSessionSendRejectsRetiredHostEpoch rejects deferred sends even after the destination fence opens.
func TestSessionSendRejectsRetiredHostEpoch(t *testing.T) {
	f, _ := readyFixture(t)
	c := f.conn.(*Conn)
	change, err := c.PrepareWorldChange(CoordinatedChange)
	if err != nil || !change.CommitAt("sample:destination", 2) {
		t.Fatalf("commit: %v", err)
	}
	notice := f.lastWorldNotice()
	f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
	channel := f.channel(ToClient)
	payload := []Scalar{integer(1)}
	if c.SendRecordAt(1, f.manifest.ID, channel.ID, channel.Schema, payload) {
		t.Fatal("source host epoch sent after destination acknowledgment")
	}
	if !c.SendRecordAt(2, f.manifest.ID, channel.ID, channel.Schema, payload) {
		t.Fatal("destination host epoch could not send")
	}
}

// TestInitialPlacementKeepsV1Handshake permits late wire-v1 activation in a fresh login.
func TestInitialPlacementKeepsV1Handshake(t *testing.T) {
	for _, when := range []string{"prepared", "committed"} {
		t.Run(when, func(t *testing.T) {
			f := newConn(t, nil)
			c := f.conn.(*Conn)
			change, err := c.PrepareWorldChange(InitialPlacement)
			if err != nil {
				t.Fatal(err)
			}
			if when == "committed" && !change.CommitAt("sample:restored", 1) {
				t.Fatal("placement failed")
			}
			hello := f.hello()
			hello.Wire = nil
			accept := f.accept(hello)
			ready := f.ready(accept)
			f.deliver(f.encode(Control{Ready: &ready}))
			if when == "prepared" && !change.CommitAt("sample:restored", 1) {
				t.Fatal("prepared placement failed")
			}
			if !f.sendToClient(1) {
				t.Fatal("initial placement imposed a previous-session world fence")
			}
		})
	}
}

// TestPendingAdmissionRequiresLatestRevision keeps two placements into one instance distinct.
func TestPendingAdmissionRequiresLatestRevision(t *testing.T) {
	f, accept := readyFixture(t)
	epochs := 0
	f.s.OnEpoch(func(uuid.UUID, PlayerSession, string) { epochs++ })
	for _, id := range []WorldID{"sample:destination", "sample:destination"} {
		change, err := f.s.PrepareWorldChange(f.player)
		if err != nil || !change.Commit(id) {
			t.Fatalf("commit: %v", err)
		}
	}
	first := f.lastWorldNotice()
	ack := f.encode(worldAcknowledgment(first, first.Epoch.WorldEpoch))
	f.deliver(ack)
	if epochs != 0 || f.sendToClient(1) {
		t.Fatal("earlier admission acknowledgment opened the latest placement")
	}
	f.deliver(f.envelope(accept, 1, func(e *Envelope) { e.WorldEpoch = first.Epoch.WorldEpoch }))
	if len(f.messages()) != 0 {
		t.Fatal("intermediate placement input acquired latest admission authority")
	}
	latest := f.lastWorldNotice()
	if latest.Epoch.WorldEpoch <= first.Epoch.WorldEpoch {
		t.Fatal("latest admission did not receive a fresh notice")
	}
	f.deliver(ack)
	if epochs != 0 || f.sendToClient(1) {
		t.Fatal("replayed acknowledgment opened latest admission")
	}
	f.deliver(f.encode(worldAcknowledgment(latest, latest.Epoch.WorldEpoch)))
	if epochs != 1 || !f.sendToClient(1) {
		t.Fatal("latest admission acknowledgment did not restore traffic")
	}
}
