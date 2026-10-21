package webrelay

import (
	"context"
	"net"
	"net/netip"
	"testing"
	"testing/synctest"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

func TestBusySignOutRetriesFitControlLimiter(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		client := netip.MustParseAddr("192.0.2.1")
		controller, detach, err := relay.openControl(client, "", time.Now())
		if err != nil {
			t.Fatal(err)
		}
		defer func() { detach(time.Now()) }()
		controller.session.mu.Lock()
		controller.session.setAuthLocked(control.AuthV1{State: control.AuthSignedIn, Gamertag: "Fixture"})
		controller.session.mu.Unlock()
		if _, err := controller.session.beginPlay(t.Context(), controller.ctx, 0); err != nil {
			t.Fatal(err)
		}
		playing := true
		defer func() {
			if playing {
				controller.session.endPlay()
			}
		}()
		server, browser := net.Pipe()
		defer server.Close()
		defer browser.Close()
		ctx, cancel := context.WithCancel(relay.ctx)
		defer cancel()
		done := make(chan struct{})
		go func() { defer close(done); relay.runControl(ctx, server, client, controller) }()
		for range 2 {
			if err := readJSON(browser, new(controlEvent)); err != nil {
				t.Fatal(err)
			}
		}
		// Leave only one burst token for the initial sign-out, as earlier account traffic can.
		for range controlRequestBurst - 1 {
			if err := writeJSON(ctx, browser, controlRequest{Op: bridgecontract.RelayOpPong}); err != nil {
				t.Fatal(err)
			}
		}
		for attempt := range 7 {
			if attempt > 0 {
				<-time.After(time.Duration(bridgecontract.RelayControlRefillMillis) * time.Millisecond)
			}
			if attempt == 6 {
				playing = false
				controller.session.endPlay()
			}
			if err := writeJSON(ctx, browser, controlRequest{Op: bridgecontract.RelayOpSignOut}); err != nil {
				t.Fatal("sign-out lost control", err)
			}
			var event controlEvent
			if err := readJSON(browser, &event); err != nil {
				t.Fatal("retry exhausted the request budget", err)
			}
			if attempt < 6 {
				if event.Error != bridgecontract.RelayRefusalSessionBusy {
					t.Fatalf("expected busy while play retires: %+v", event)
				}
				if account, _ := controller.session.Auth(); account.State != control.AuthSignedIn {
					t.Fatal("busy refusal lost the account")
				}
			} else if event.Account == nil || event.Account.State != control.AuthSignedOut {
				t.Fatalf("sign-out was not confirmed after play retired: %+v", event)
			}
		}
		cancel()
		<-done
	})
}
