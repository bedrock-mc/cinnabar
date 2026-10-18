package webrelay

import (
	"errors"
	"net"
	"net/netip"
	"testing"
	"testing/synctest"
	"time"
)

// failedControlConn fails a selected response while keeping cancellation and read deadlines real.
type failedControlConn struct {
	net.Conn
	failAt int
	writes int
}

// Write models a control connection lost before the selected response reaches the browser.
func (conn *failedControlConn) Write(data []byte) (int, error) {
	conn.writes++
	if conn.writes == conn.failAt {
		return 0, net.ErrClosed
	}
	return conn.Conn.Write(data)
}

func TestResumeSurvivesLostReplacementDelivery(t *testing.T) {
	for _, test := range []struct {
		name   string
		failAt int
	}{
		{name: "keepalive", failAt: 3},
		{name: "resumed_welcome", failAt: 1},
	} {
		t.Run(test.name, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				relay := testRelay(t)
				client := netip.MustParseAddr("192.0.2.1")
				controller, detach, err := relay.openControl(client, "", time.Now())
				if err != nil {
					t.Fatal(err)
				}
				lastKnown := controller.session.token
				if test.failAt == 1 {
					detach(time.Now())
					controller, detach, err = relay.openControl(client, lastKnown, time.Now())
					if err != nil {
						t.Fatal(err)
					}
				}
				server, browser := net.Pipe()
				defer server.Close()
				defer browser.Close()
				conn := &failedControlConn{Conn: server, failAt: test.failAt}
				done := make(chan struct{})
				go func() {
					defer close(done)
					relay.runControl(relay.ctx, conn, client, controller)
				}()
				for range test.failAt - 1 {
					if err := readJSON(browser, new(controlEvent)); err != nil {
						t.Fatal(err)
					}
				}
				<-done
				if conn.writes != test.failAt {
					t.Fatal("control ended before the replacement write failed")
				}
				detach(time.Now())
				// Further lost welcome responses must preserve the browser's same last-known token.
				for range 3 {
					resumed, detach, err := relay.openControl(client, lastKnown, time.Now())
					if err != nil || resumed.session != controller.session {
						t.Fatal("lost replacement made the retained account inaccessible", err)
					}
					if len(relay.tokens) != 2 {
						t.Fatal("resuming retained more than the current and presented bearers")
					}
					detach(time.Now())
				}
				relay.expire(time.Now().Add(idleTTL + time.Second))
				if len(relay.tokens) != 0 {
					t.Fatal("expired account retained a bearer")
				}
			})
		})
	}
}

func TestResumeOverlapKeepsOriginalExpiryAndGeneration(t *testing.T) {
	relay := testRelay(t)
	now := time.Now()
	client := netip.MustParseAddr("192.0.2.1")
	controller, detach, err := relay.openControl(client, "", now)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { detach(now) }()
	previous := controller.session.token
	discarded := testWelcome(t, relay, controller, true, now.Add(resumeTTL/2)).Resume
	oldController, oldDetach := controller, detach
	controller, detach, err = relay.openControl(client, previous, now.Add(resumeTTL/2))
	if err != nil || controller.session != oldController.session || oldController.ctx.Err() == nil {
		t.Fatal("valid previous bearer did not take over the retained account", err)
	}
	oldDetach(now)
	replacement := testWelcome(t, relay, controller, false, now.Add(resumeTTL/2))
	if _, _, err := relay.openControl(client, discarded, now.Add(resumeTTL/2)); !errors.Is(err, errResume) {
		t.Fatal("rotated-out bearer resumed control", err)
	}
	if _, _, err := relay.playSession(nil, hello{Resume: previous, Generation: controller.generation}, now.Add(resumeTTL/2)); err != nil {
		t.Fatal("overlapping bearer could not open play", err)
	}
	if _, _, err := relay.playSession(nil, hello{Resume: previous, Generation: controller.generation + 1}, now); !errors.Is(err, errResume) {
		t.Fatal("overlapping bearer bypassed the play generation", err)
	}
	if _, _, err := relay.playSession(nil, hello{Resume: previous, Generation: controller.generation}, now.Add(resumeTTL)); !errors.Is(err, errResume) {
		t.Fatal("rotation extended the previous bearer's expiry", err)
	}
	if _, _, err := relay.playSession(nil, hello{Resume: replacement.Resume, Generation: controller.generation}, now.Add(resumeTTL)); err != nil {
		t.Fatal("replacement expired with the previous bearer", err)
	}
	detach(now)
	if _, _, err := relay.openControl(client, previous, now.Add(resumeTTL)); !errors.Is(err, errResume) {
		t.Fatal("expired previous bearer resumed control", err)
	}
}

func TestInvalidResumeDoesNotRetireController(t *testing.T) {
	relay := testRelay(t)
	now := time.Now()
	client := netip.MustParseAddr("192.0.2.1")
	controller, detach, err := relay.openControl(client, "", now)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { detach(now) }()
	for _, test := range []struct {
		token string
		now   time.Time
	}{
		{token: "invalid", now: now},
		{token: controller.session.token, now: now.Add(resumeTTL)},
	} {
		if _, _, err := relay.openControl(client, test.token, test.now); !errors.Is(err, errResume) {
			t.Fatal("invalid or expired bearer resumed control", err)
		}
		if controller.ctx.Err() != nil || controller.session.controller != controller {
			t.Fatal("refused resume retired the current controller")
		}
	}
}

func TestKeepaliveOverlapDiscardsOlderBearers(t *testing.T) {
	relay := testRelay(t)
	now := time.Now()
	client := netip.MustParseAddr("192.0.2.1")
	controller, detach, err := relay.openControl(client, "", now)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { detach(now) }()
	previous := controller.session.token
	discarded := ""
	for range 3 {
		current := testWelcome(t, relay, controller, true, now).Resume
		if _, _, err := relay.playSession(nil, hello{Resume: previous, Generation: controller.generation}, now); err != nil {
			t.Fatal("most recent bearer was discarded", err)
		}
		if len(relay.tokens) != 2 {
			t.Fatal("keepalives accumulated old bearers")
		}
		if discarded != "" {
			if _, _, err := relay.playSession(nil, hello{Resume: discarded, Generation: controller.generation}, now); !errors.Is(err, errResume) {
				t.Fatal("an older bearer remained usable", err)
			}
		}
		discarded = previous
		previous = current
		if !relay.acknowledge(controller, current, now) {
			t.Fatal("keepalive acknowledgement refused")
		}
	}
}

func TestKeepaliveRetainsAcknowledgedBearerUntilReplacementArrives(t *testing.T) {
	relay := testRelay(t)
	client := netip.MustParseAddr("192.0.2.1")
	now := time.Now()
	controller, detach, err := relay.openControl(client, "", now)
	if err != nil {
		t.Fatal(err)
	}
	defer func() { detach(now) }()
	acknowledged := testWelcome(t, relay, controller, true, now.Add(keepAliveInterval))
	if !relay.acknowledge(controller, acknowledged.Resume, now.Add(keepAliveInterval+time.Second)) {
		t.Fatal("received ping was not acknowledged")
	}
	first := testWelcome(t, relay, controller, true, now.Add(2*keepAliveInterval))
	second := testWelcome(t, relay, controller, true, now.Add(3*keepAliveInterval))
	if first.Resume != second.Resume || second.ExpiresSeconds != int((resumeTTL-keepAliveInterval)/time.Second) {
		t.Fatal("an unanswered ping replaced or extended the outstanding bearer")
	}
	// A duplicate pong for the last received ping must not acknowledge the unseen replacement.
	if relay.acknowledge(controller, acknowledged.Resume, now.Add(3*keepAliveInterval)) {
		t.Fatal("a delayed pong acknowledged an unseen replacement")
	}
	detach(now.Add(3*keepAliveInterval + time.Second))
	resumed, finish, err := relay.openControl(client, acknowledged.Resume, now.Add(3*keepAliveInterval+time.Second))
	if err != nil || resumed.session != controller.session {
		t.Fatal("unanswered keepalives discarded the last acknowledged bearer", err)
	}
	defer func() { finish(now) }()
	if len(relay.tokens) != 2 {
		t.Fatal("recovery exceeded the two-bearer bound")
	}
}

func TestUnacknowledgedResumeWelcomeSurvivesRepeatedPings(t *testing.T) {
	relay := testRelay(t)
	now := time.Now()
	client := netip.MustParseAddr("192.0.2.1")
	controller, detach, err := relay.openControl(client, "", now)
	if err != nil {
		t.Fatal(err)
	}
	known := controller.session.token
	detach(now)
	controller, detach, err = relay.openControl(client, known, now.Add(time.Second))
	if err != nil {
		t.Fatal(err)
	}
	defer func() { detach(now) }()
	for i := range 3 {
		testWelcome(t, relay, controller, true, now.Add(time.Duration(i+1)*keepAliveInterval))
		if relay.resumeLocked(known, now.Add(time.Duration(i+1)*keepAliveInterval)) != controller.session || len(relay.tokens) != 2 {
			t.Fatal("an unanswered resumed welcome lost the presented bearer")
		}
	}
	if relay.acknowledge(controller, controller.session.token, now.Add(resumeTTL+time.Second)) {
		t.Fatal("an expired bearer was acknowledged")
	}
}
