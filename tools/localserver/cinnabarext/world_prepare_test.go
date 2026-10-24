package cinnabarext

import (
	"github.com/google/uuid"
	"testing"
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
	f.s.OnEpoch(func(uuid.UUID, string) { epochs++ })
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
	f.s.OnEpoch(func(uuid.UUID, string) { epochs++ })
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
