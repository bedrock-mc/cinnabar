package cinnabarext

import (
	"testing"
	"testing/synctest"
	"time"
)

// TestControlRetriesReplaceAndExpire covers both rate ceilings and stale reply retirement.
func TestControlRetriesReplaceAndExpire(t *testing.T) {
	for _, retirement := range []string{"sent", "epoch", "world notice", "closed", "expired"} {
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
					if !f.s.SendControl(f.player, message) {
						t.Fatal("control not retained")
					}
					message.number = 8
					if !f.s.SendControl(f.player, message) {
						t.Fatal("replacement not retained")
					}
					synctest.Wait()
					switch retirement {
					case "epoch":
						f.deliver(f.encode(Control{Epoch: &Epoch{Session: accept.Session, WorldEpoch: testEpoch + 1}}))
					case "world notice":
						if !f.s.WorldChanged(f.player, "sample:next") {
							t.Fatal("world notice rejected")
						}
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
