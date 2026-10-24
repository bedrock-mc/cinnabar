package cinnabarext

import (
	"errors"
	"strings"
	"testing"
	"testing/synctest"
	"time"
)

// controlRecord supplies an arbitrary valid record on a fixture's declared route.
type controlRecord struct {
	route   Route
	payload Record
}

// Route identifies the fixture channel.
func (m controlRecord) Route() Route { return m.route }

// Encode returns the record to validate and send.
func (m controlRecord) Encode() (Record, error) { return m.payload, nil }

// TestImpossibleControlDoesNotBlockData rejects batches that cannot ever acquire credit.
func TestImpossibleControlDoesNotBlockData(t *testing.T) {
	f, _ := readyFixture(t)
	c := f.conn.(*Conn)
	c.wire.Limits.MaxFragmentBytes = 128
	ch := Channel{ID: f.manifest.ID + ".large", Schema: 1, Direction: ToClient,
		Fields: []Field{TextField{MaxBytes: uint16(MaxMessageBytes)}}}
	recipient := c.grant.Recipients[f.manifest.ID]
	recipient.Channels = append(recipient.Channels, ch)
	c.grant.Recipients[f.manifest.ID] = recipient
	message := controlRecord{Route{f.manifest.ID, ch.ID, ch.Schema}, Record{text(strings.Repeat(`"`, 24000))}}
	// A retained impossible reply would also start the timer worker; suppress it in this test.
	c.retryingControls = true
	var burst *BurstExceeded
	if err := f.s.SendControl(f.player, message); !errors.As(err, &burst) {
		t.Fatalf("want a permanent burst error, got %v", err)
	}
	if c.rejectedControls != 1 {
		t.Fatal("oversized control was not counted")
	}
	if len(c.controls) != 0 || !f.sendToClient(7) {
		t.Fatal("an impossible control blocked ordinary data")
	}
}

// TestControlRetriesReplaceAndExpire covers both rate ceilings and stale reply retirement.
func TestControlRetriesReplaceAndExpire(t *testing.T) {
	for _, retirement := range []string{"sent", "epoch", "world notice", "prepared world", "closed", "expired"} {
		for _, bytes := range []bool{false, true} {
			t.Run(retirement+map[bool]string{true: "/bytes", false: "/messages"}[bytes], func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					defer func() {
						done := make(chan struct{})
						time.AfterFunc(time.Second, func() { close(done) })
						<-done
						synctest.Wait()
					}()
					f, accept := readyFixture(t)
					ch := f.channel(ToClient)
					message := testMessage{Route{f.manifest.ID, ch.ID, ch.Schema}, 7}
					c := f.conn.(*Conn)
					c.mu.Lock()
					if bytes {
						c.egress.bytes = 0
					} else {
						c.egress.messages = 0
					}
					c.mu.Unlock()
					if err := f.s.SendControl(f.player, message); err != nil {
						t.Fatal("control not retained")
					}
					message.number = 8
					if err := f.s.SendControl(f.player, message); err != nil {
						t.Fatal("replacement not retained")
					}
					invalid := message
					invalid.number = -1
					if err := f.s.SendControl(f.player, invalid); err == nil {
						t.Fatal("invalid replacement accepted")
					}
					synctest.Wait()
					switch retirement {
					case "epoch":
						f.deliver(f.encode(Control{Epoch: &Epoch{Session: accept.Session, WorldEpoch: testEpoch + 1}}))
					case "world notice":
						if !f.s.WorldChanged(f.player, "sample:next") {
							t.Fatal("world notice rejected")
						}
					case "prepared world":
						change, err := c.PrepareWorldChange(CoordinatedChange)
						if err != nil || !change.Commit("sample:next") {
							t.Fatalf("world commit rejected: %v", err)
						}
						notice := f.lastWorldNotice()
						f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
					case "closed":
						if err := f.conn.Close(); err != nil {
							t.Fatal(err)
						}
					case "expired":
						f.clock.Advance(time.Duration(offerLifetimeSecs) * time.Second)
					}
					before := len(f.carrier())
					f.clock.Advance(time.Second)
					c.mu.Lock()
					if !c.retryControlTick() || len(c.controls) != 0 {
						t.Fatal("pending replies survived")
					}
					c.mu.Unlock()
					out := f.carrier()[before:]
					if retirement != "sent" {
						if len(out) != 0 {
							t.Fatal("obsolete reply was sent")
						}
						return
					}
					if len(out) != 1 {
						t.Fatalf("sent %d replies, want one", len(out))
					}
					var envelope Envelope
					if err := Decode(out[0], &envelope); err != nil || *envelope.Payload[0].Integer != message.number {
						t.Fatalf("reply: %+v, %v", envelope, err)
					}
				})
			})
		}
	}
}

// TestRetainedControlSurvivesAbortedPreparation keeps accepted replies across a temporary fence.
func TestRetainedControlSurvivesAbortedPreparation(t *testing.T) {
	for _, retry := range []string{"tick", "data send"} {
		for _, budget := range []string{"messages", "bytes"} {
			t.Run(retry+"/"+budget, func(t *testing.T) {
				f, _ := readyFixture(t)
				c := f.conn.(*Conn)
				// Drive retries directly so each attempt occurs at a known fence state.
				c.retryingControls = true
				if budget == "messages" {
					c.egress.messages = 0
				} else {
					c.egress.bytes = 0
				}
				ch := f.channel(ToClient)
				message := testMessage{Route{f.manifest.ID, ch.ID, ch.Schema}, 7}
				if err := f.s.SendControl(f.player, message); err != nil {
					t.Fatalf("control not retained: %v", err)
				}
				change, err := c.PrepareWorldChange(CoordinatedChange)
				if err != nil {
					t.Fatal(err)
				}
				before := len(f.carrier())
				f.clock.Advance(time.Second)
				if retry == "tick" {
					c.mu.Lock()
					done := c.retryControlTick()
					c.mu.Unlock()
					if done {
						t.Fatal("temporary preparation discarded the retained reply")
					}
				} else if f.sendToClient(1) {
					t.Fatal("data crossed the preparation fence")
				}
				if len(c.controls) != 1 || len(f.carrier()) != before {
					t.Fatal("preparation sent or discarded the retained reply")
				}
				change.Abort()
				c.mu.Lock()
				done := c.retryControlTick()
				c.mu.Unlock()
				if !done || len(c.controls) != 0 || len(f.carrier()) != before+1 {
					t.Fatal("aborting did not resume retained control delivery")
				}
				var envelope Envelope
				if err := Decode(f.carrier()[before], &envelope); err != nil || *envelope.Payload[0].Integer != message.number {
					t.Fatalf("reply: %+v, %v", envelope, err)
				}
			})
		}
	}
}

// TestControlReplyRejectsEpochChangedDuringEncoding rechecks authority after user encoding.
func TestControlReplyRejectsEpochChangedDuringEncoding(t *testing.T) {
	f, _ := readyFixture(t)
	c := f.conn.(*Conn)
	c.hostEpoch = 1
	ch := f.channel(ToClient)
	message := epochControl{testMessage{Route{f.manifest.ID, ch.ID, ch.Schema}, 7}, func() {
		change, err := c.PrepareWorldChange(CoordinatedChange)
		if err != nil || !change.CommitAt("sample:next", 2) {
			t.Fatalf("commit: %v", err)
		}
		notice := f.lastWorldNotice()
		f.deliver(f.encode(worldAcknowledgment(notice, notice.Epoch.WorldEpoch)))
	}}
	before := len(f.carrier())
	if err := c.SendControlAt(1, message); err == nil {
		t.Fatal("old-world control reply crossed the new admission epoch")
	}
	if len(c.controls) != 0 || len(f.carrier()) != before+1 {
		t.Fatal("stale control was retained or sent after the world notice")
	}
	if err := c.SendControlAt(2, message.testMessage); err != nil {
		t.Fatalf("current epoch rejected: %v", err)
	}
}

// epochControl changes admission while encoding a reply to expose a stale send race.
type epochControl struct {
	testMessage
	encode func()
}

// Encode advances the scripted lifecycle before returning the reply record.
func (m epochControl) Encode() (Record, error) {
	m.encode()
	return m.testMessage.Encode()
}
