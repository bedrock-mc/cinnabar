package cinnabarext

import (
	"bytes"
	"fmt"
	"testing"
	"time"

	"github.com/google/uuid"
)

// worldAcknowledgment echoes the specific server request while naming the client's new epoch.
func worldAcknowledgment(notice Control, epoch uint64) Control {
	return Control{WorldAck: &WorldAck{
		Session: notice.Epoch.Session, WorldEpoch: epoch, RequestedEpoch: notice.Epoch.WorldEpoch,
	}}
}

// lastWorldNotice decodes the latest carrier frame and requires a server world request.
func (f *connFixture) lastWorldNotice() Control {
	f.t.Helper()
	frames := f.carrier()
	var notice Control
	if err := Decode(frames[len(frames)-1], &notice); err != nil || notice.Epoch == nil {
		f.t.Fatalf("world notice: %+v, %v", notice, err)
	}
	return notice
}

func TestNativeEpochCannotAcknowledgeServerWorldNotice(t *testing.T) {
	f, accept := readyFixture(t)
	epochs := 0
	f.s.OnEpoch(func(uuid.UUID, PlayerSession, string) { epochs++ })
	if !f.s.WorldChanged(f.player, "sample:destination") {
		t.Fatal("world notice rejected")
	}
	// This native update was already in flight before the client saw the notice.
	f.deliver(f.encode(Control{Epoch: &Epoch{accept.Session, testEpoch + 1}}))
	if epochs != 0 || f.sendToClient(1) {
		t.Fatal("native epoch acknowledged the unseen world notice")
	}
	f.deliver(f.envelope(accept, 1, func(e *Envelope) { e.WorldEpoch++ }))
	if len(f.messages()) != 0 {
		t.Fatal("old-world record crossed the pending world fence")
	}
	f.deliver(f.encode(worldAcknowledgment(f.lastWorldNotice(), testEpoch+2)))
	if epochs != 1 || !f.sendToClient(2) {
		t.Fatal("correlated world acknowledgment did not reopen the fence")
	}
	f.deliver(f.envelope(accept, 2, func(e *Envelope) { e.WorldEpoch += 2 }))
	if len(f.messages()) != 1 {
		t.Fatal("destination record was not delivered after acknowledgment")
	}
}

func TestWorldNoticesAndAcknowledgmentsBypassExhaustedDataBudgets(t *testing.T) {
	for _, budget := range []string{"messages", "bytes"} {
		t.Run(budget, func(t *testing.T) {
			f, accept := readyFixture(t)
			conn := f.conn.(*Conn)
			count, size := MaxMessagesPerSecond, 0
			if budget == "bytes" {
				count, size = 1, MaxBytesPerSecond
			}
			for range count {
				for _, rate := range []*RateLimit{&conn.egress, &conn.ingress.rate} {
					if err := rate.Charge(size, uint64(f.clock.Now().UnixMilli())); err != nil {
						t.Fatal(err)
					}
				}
			}
			for i, world := range []WorldID{"sample:first", "sample:second", "sample:third"} {
				before := len(f.carrier())
				if !f.s.WorldChanged(f.player, world) || len(f.carrier()) != before+1 {
					t.Fatal("data budget prevented immediate world notice delivery")
				}
				notice := f.lastWorldNotice()
				if notice.Epoch.WorldEpoch != testEpoch+uint64(i)+1 || f.sendToClient(1) {
					t.Fatal("transfer did not establish a fresh world fence")
				}
				f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
			}
			if f.sendToClient(1) {
				t.Fatal("world controls refilled the exhausted data budget")
			}
			f.clock.Advance(time.Second)
			if !f.sendToClient(2) {
				t.Fatal("completed transfers left stale state after data credits refilled")
			}
			f.deliver(f.envelope(accept, 1, func(e *Envelope) { e.WorldEpoch += 3 }))
			if len(f.messages()) != 1 {
				t.Fatal("destination traffic did not resume after the latest acknowledgment")
			}
		})
	}
}

func TestWorldControlsPreserveDataCredits(t *testing.T) {
	f, accept := readyFixture(t)
	if !f.s.WorldChanged(f.player, "sample:destination") {
		t.Fatal("world notice rejected")
	}
	notice := f.lastWorldNotice()
	f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
	for sequence := uint64(1); sequence <= MaxMessagesPerSecond; sequence++ {
		if !f.sendToClient(1) {
			t.Fatal("world control spent outbound data credit")
		}
		f.deliver(f.envelope(accept, sequence, func(e *Envelope) { e.WorldEpoch++ }))
	}
	if len(f.messages()) != MaxMessagesPerSecond {
		t.Fatal("world acknowledgment spent inbound data credit")
	}
}

func TestReplayedWorldAcknowledgmentsConsumeDataBudgets(t *testing.T) {
	for _, outstanding := range []bool{false, true} {
		for _, budget := range []string{"messages", "bytes"} {
			t.Run(fmt.Sprintf("outstanding=%t/%s", outstanding, budget), func(t *testing.T) {
				f, _ := readyFixture(t)
				f.s.WorldChanged(f.player, "sample:first")
				notice := f.lastWorldNotice()
				ack := f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch))
				f.deliver(ack)
				if outstanding {
					f.s.WorldChanged(f.player, "sample:second")
				}
				count := MaxMessagesPerSecond
				if budget == "bytes" {
					ack = append(ack, bytes.Repeat([]byte(" "), MaxEnvelopeBytes-len(ack))...)
					count = MaxBytesPerSecond / len(ack)
				}
				for range count {
					f.deliver(ack)
					if f.conn.(*Conn).phase != active {
						t.Fatal("replayed acknowledgment ended the client part within its budget")
					}
				}
				f.deliver(ack)
				if f.conn.(*Conn).phase != fallenBack {
					t.Fatal("replayed acknowledgments bypassed the data budget")
				}
				f.fellBack()
			})
		}
	}
}

func TestWorldNoticeWriteFailureEndsTheClientPart(t *testing.T) {
	f, _ := readyFixture(t)
	before := len(f.carrier())
	if err := f.inner.Close(); err != nil {
		t.Fatal(err)
	}
	if f.s.WorldChanged(f.player, "sample:destination") || len(f.carrier()) != before {
		t.Fatal("world notice written to a closed connection")
	}
	if f.conn.(*Conn).phase != fallenBack || f.sendToClient(1) || f.s.WorldChanged(f.player, "sample:next") {
		t.Fatal("failed world notice left the optional session usable")
	}
}

func TestWorldAcknowledgmentsRejectInvalidNotices(t *testing.T) {
	for _, invalid := range []string{"no request", "future request", "below request", "another session"} {
		t.Run(invalid, func(t *testing.T) {
			f, accept := readyFixture(t)
			if invalid != "no request" {
				f.s.WorldChanged(f.player, "sample:destination")
			}
			ack := &WorldAck{Session: accept.Session, WorldEpoch: testEpoch + 1, RequestedEpoch: testEpoch + 1}
			switch invalid {
			case "future request":
				ack.RequestedEpoch++
			case "below request":
				ack.WorldEpoch--
			case "another session":
				ack.Session = "another session"
			}
			f.deliver(f.encode(Control{WorldAck: ack}))
			f.fellBack()
		})
	}
}

// TestWorldChangesPublishEveryAdmission keeps traffic fenced until the queued revisions are acknowledged.
func TestWorldChangesPublishEveryAdmission(t *testing.T) {
	for _, worlds := range [][]WorldID{
		{"sample:first"},
		{"sample:first", "sample:second"},
		{"sample:first", "sample:second", "sample:third"},
		{"sample:first", "sample:second", "sample:first"},
	} {
		for _, cumulative := range []bool{false, true} {
			t.Run(fmt.Sprintf("%v/cumulative=%t", worlds, cumulative), func(t *testing.T) {
				f, accept := readyFixture(t)
				notified := 0
				f.s.OnEpoch(func(uuid.UUID, PlayerSession, string) { notified++ })
				before := len(f.carrier())
				var notices []Control
				for i, world := range worlds {
					if !f.s.WorldChanged(f.player, world) || len(f.carrier()) != before+i+1 {
						t.Fatal("committed transfer did not publish its notice")
					}
					notices = append(notices, f.lastWorldNotice())
					if !f.s.WorldChanged(f.player, world) || len(f.carrier()) != before+i+1 {
						t.Fatal("duplicate world allocated another notice")
					}
					if f.sendToClient(1) {
						t.Fatal("transfer opened sends before acknowledgment")
					}
				}
				sequence, epoch := uint64(1), uint64(testEpoch)
				for i, notice := range notices {
					f.deliver(f.envelope(accept, sequence, func(e *Envelope) { e.WorldEpoch = epoch }))
					sequence++
					if len(f.messages()) != 0 {
						t.Fatal("application input crossed a pending admission")
					}
					if cumulative && i != len(notices)-1 {
						continue
					}
					// Native world changes may advance the client beyond the requested epoch.
					epoch = max(epoch+1, notice.Epoch.WorldEpoch) + 1
					ack := f.encode(worldAcknowledgment(notice, epoch))
					f.deliver(ack, ack)
					if i != len(notices)-1 && (notified != 0 || f.sendToClient(2)) {
						t.Fatal("intermediate acknowledgment released a later admission")
					}
				}
				if notified != 1 || !f.sendToClient(3) {
					t.Fatal("latest acknowledgment did not resume application traffic")
				}
				frames := f.carrier()
				var sent Envelope
				if err := Decode(frames[len(frames)-1], &sent); err != nil || sent.WorldEpoch != epoch {
					t.Fatalf("outbound epoch = %d, want %d: %v", sent.WorldEpoch, epoch, err)
				}
				f.deliver(f.envelope(accept, sequence, func(e *Envelope) { e.WorldEpoch = epoch }))
				if len(f.messages()) != 1 {
					t.Fatal("acknowledged destination input was not delivered")
				}
				if !f.s.WorldChanged(f.player, "sample:next") || f.sendToClient(4) {
					t.Fatal("later travel did not establish a fresh fence")
				}
				next := f.lastWorldNotice()
				if next.Epoch.WorldEpoch <= epoch {
					t.Fatal("new request did not exceed the acknowledged epoch")
				}
				f.deliver(f.encode(worldAcknowledgment(next, next.Epoch.WorldEpoch)))
				if notified != 2 || !f.sendToClient(5) {
					t.Fatal("later acknowledgment did not release its fence")
				}
			})
		}
	}
}
