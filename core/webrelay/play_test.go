package webrelay

import (
	"context"
	"errors"
	"net/netip"
	"testing"
	"testing/synctest"
	"time"
)

func TestResumedPlayWaitsForCancelledHandler(t *testing.T) {
	for _, boundTransport := range []bool{false, true} {
		t.Run(map[bool]string{false: "websocket", true: "webtransport"}[boundTransport], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				relay := testRelay(t)
				client := netip.MustParseAddr("192.0.2.1")
				session, detach, err := relay.openControl(client, "", time.Now())
				if err != nil {
					t.Fatal(err)
				}
				token := session.token
				oldControl := session.controlCtx
				if _, err := session.beginPlay(t.Context(), oldControl); err != nil {
					t.Fatal(err)
				}
				detach(time.Now())
				if !errors.Is(oldControl.Err(), context.Canceled) {
					t.Fatal("old play controller was not cancelled")
				}
				resumed, detach, err := relay.openControl(client, token, time.Now())
				if err != nil {
					t.Fatal(err)
				}
				defer func() { detach(time.Now()) }()
				welcome := relay.welcome(resumed, false, time.Now())
				var bound *webSession
				if boundTransport {
					bound = resumed
				}
				selected, controlCtx, err := relay.playSession(bound, hello{Resume: welcome.Resume, Generation: welcome.Generation}, time.Now())
				if err != nil || selected != session {
					t.Fatal("fresh play generation was refused", err)
				}
				done := make(chan error, 1)
				go func() {
					_, err := selected.beginPlay(t.Context(), controlCtx)
					done <- err
				}()
				synctest.Wait()
				select {
				case err := <-done:
					t.Fatal("resumed play did not wait for the cancelled handler", err)
				default:
				}
				// Release only when the previous handler has completed its teardown.
				session.endPlay()
				if err := <-done; err != nil {
					t.Fatal("resumed play could not restart after teardown", err)
				}
				if _, err := session.beginPlay(t.Context(), controlCtx); !errors.Is(err, errSessionBusy) {
					t.Fatal("another active play connection was admitted", err)
				}
				session.endPlay()
			})
		})
	}
}

func TestResumedPlayWaitStopsOnCancellation(t *testing.T) {
	for _, cancellation := range []string{"caller", "control", "relay"} {
		t.Run(cancellation, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				relay := testRelay(t)
				client := netip.MustParseAddr("192.0.2.1")
				session, detach, err := relay.openControl(client, "", time.Now())
				if err != nil {
					t.Fatal(err)
				}
				token := session.token
				if _, err := session.beginPlay(t.Context(), session.controlCtx); err != nil {
					t.Fatal(err)
				}
				defer session.endPlay()
				detach(time.Now())
				resumed, detach, err := relay.openControl(client, token, time.Now())
				if err != nil {
					t.Fatal(err)
				}
				defer func() { detach(time.Now()) }()
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				controlCtx := resumed.controlCtx
				done := make(chan error, 1)
				go func() {
					_, err := resumed.beginPlay(ctx, controlCtx)
					done <- err
				}()
				synctest.Wait()
				select {
				case err := <-done:
					t.Fatal("play wait ended before cancellation", err)
				default:
				}
				switch cancellation {
				case "caller":
					cancel()
				case "control":
					detach(time.Now())
				case "relay":
					relay.Close()
				}
				if err := <-done; !errors.Is(err, context.Canceled) {
					t.Fatal("cancelled play waited for the old handler or claimed its slot", err)
				}
			})
		})
	}
}

func TestCancelledControllerCannotClaimResumedPlay(t *testing.T) {
	relay := testRelay(t)
	client := netip.MustParseAddr("192.0.2.1")
	session, detach, err := relay.openControl(client, "", time.Now())
	if err != nil {
		t.Fatal(err)
	}
	token, oldControl := session.token, session.controlCtx
	detach(time.Now())
	resumed, detach, err := relay.openControl(client, token, time.Now())
	if err != nil {
		t.Fatal(err)
	}
	defer func() { detach(time.Now()) }()
	if _, err := session.beginPlay(t.Context(), oldControl); !errors.Is(err, context.Canceled) {
		t.Fatal("late old-generation play claimed the resumed session", err)
	}
	if _, err := resumed.beginPlay(t.Context(), resumed.controlCtx); err != nil {
		t.Fatal("late old-generation play blocked the new generation", err)
	}
	defer session.endPlay()
}
