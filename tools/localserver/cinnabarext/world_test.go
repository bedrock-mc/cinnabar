package cinnabarext

import (
	"testing"
	"testing/synctest"
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
	f.s.OnEpoch(func(uuid.UUID, string) { epochs++ })
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

func TestWorldNoticeAdmissionFailureKeepsBothDirectionsFenced(t *testing.T) {
	f, accept := readyFixture(t)
	var retry func()
	f.s.deferWorldNotice = func(callback func()) { retry = callback }
	for f.sendToClient(1) {
	}
	before := len(f.carrier())
	if f.s.WorldChanged(f.player, "sample:destination") {
		t.Fatal("world notice exceeded the exhausted egress budget")
	}
	f.deliver(f.envelope(accept, 1, nil))
	if len(f.messages()) != 0 {
		t.Fatal("old-world record reached the destination after notice admission failed")
	}
	f.clock.Advance(time.Second)
	if f.sendToClient(2) {
		t.Fatal("old-world sends resumed when credits refilled")
	}
	if retry == nil || len(f.carrier()) != before {
		t.Fatal("failed notice was not deferred without sending old-world data")
	}
	retry()
	notice := f.lastWorldNotice()
	if len(f.carrier()) != before+1 || notice.Epoch.WorldEpoch != testEpoch+1 {
		t.Fatal("pending world notice was not delivered after rate refill")
	}
	if f.sendToClient(3) {
		t.Fatal("delivery reopened the fence before acknowledgment")
	}
	f.deliver(f.envelope(accept, 2, nil))
	if len(f.messages()) != 0 {
		t.Fatal("old-world inbound traffic resumed before acknowledgment")
	}
	f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
	if !f.sendToClient(4) {
		t.Fatal("acknowledged retry did not reopen sends")
	}
	f.deliver(f.envelope(accept, 3, func(e *Envelope) { e.WorldEpoch++ }))
	if len(f.messages()) != 1 {
		t.Fatal("destination record was not delivered after the retried notice")
	}
}

func TestWorldNoticeRetryEndsOnlyTheClientPartIfDeliveryStillFails(t *testing.T) {
	for _, reason := range []string{"no refill", "write failure", "expired", "disconnected"} {
		t.Run(reason, func(t *testing.T) {
			f, _ := readyFixture(t)
			var retry func()
			f.s.deferWorldNotice = func(callback func()) { retry = callback }
			for f.sendToClient(1) {
			}
			f.s.WorldChanged(f.player, "sample:destination")
			before := len(f.carrier())
			switch reason {
			case "write failure":
				f.clock.Advance(time.Second)
				f.inner.closed = true
			case "expired":
				f.clock.Advance(24 * time.Hour)
			case "disconnected":
				f.conn.(*Conn).end("disconnected")
			}
			retry()
			if len(f.carrier()) != before || f.conn.(*Conn).phase != fallenBack || f.sendToClient(2) {
				t.Fatal("failed retry left the optional session usable")
			}
			if reason != "write failure" {
				f.connected()
			}
		})
	}
}

func TestWorldNoticeRetryDeliversTheLatestCommittedWorld(t *testing.T) {
	f, accept := readyFixture(t)
	var retries []func()
	f.s.deferWorldNotice = func(callback func()) { retries = append(retries, callback) }
	for f.sendToClient(1) {
	}
	for _, world := range []WorldID{"sample:first", "sample:first", "sample:second"} {
		if f.s.WorldChanged(f.player, world) {
			t.Fatal("world notice bypassed exhausted credits")
		}
	}
	if len(retries) != 1 {
		t.Fatalf("scheduled %d retries, want one", len(retries))
	}
	f.clock.Advance(time.Second)
	retries[0]()
	notice := f.lastWorldNotice()
	if notice.Epoch.WorldEpoch != testEpoch+2 {
		t.Fatal("retry delivered a superseded world notice")
	}
	f.deliver(f.encode(worldAcknowledgment(notice, testEpoch+2)))
	if !f.sendToClient(2) {
		t.Fatal("latest world acknowledgment did not reopen sends")
	}
	f.deliver(f.envelope(accept, 1, func(e *Envelope) { e.WorldEpoch += 2 }))
	if len(f.messages()) != 1 {
		t.Fatal("latest-world record was not delivered")
	}
}

func TestWorldNoticeRetriesWithoutFurtherPacketTraffic(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		f, _ := readyFixture(t)
		done := make(chan struct{})
		schedule := f.s.deferWorldNotice
		f.s.deferWorldNotice = func(retry func()) {
			schedule(func() {
				f.clock.Advance(time.Second)
				retry()
				close(done)
			})
		}
		for f.sendToClient(1) {
		}
		before := len(f.carrier())
		if f.s.WorldChanged(f.player, "sample:destination") {
			t.Fatal("world notice bypassed exhausted credits")
		}
		<-done
		if len(f.carrier()) != before+1 || f.lastWorldNotice().Epoch.WorldEpoch != testEpoch+1 {
			t.Fatal("idle connection did not retry its pending world notice")
		}
	})
}

func TestWorldAcknowledgmentsRejectUnsentOrInvalidNotices(t *testing.T) {
	for _, invalid := range []string{"no request", "unsent", "future request", "below request", "another session"} {
		t.Run(invalid, func(t *testing.T) {
			f, accept := readyFixture(t)
			f.s.deferWorldNotice = func(func()) {}
			if invalid == "unsent" {
				for f.sendToClient(1) {
				}
			}
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
