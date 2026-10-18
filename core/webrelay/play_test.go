package webrelay

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"testing"
	"testing/synctest"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

func TestResumeSupersedesControllerBeforeDetach(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		client := netip.MustParseAddr("192.0.2.1")
		controller, oldDetach, err := relay.openControl(client, "", time.Now())
		if err != nil {
			t.Fatal(err)
		}
		oldControl := controller.ctx
		oldGeneration, token := controller.generation, controller.session.token
		controller.session.mu.Lock()
		controller.session.setAuthLocked(control.AuthV1{State: control.AuthSignedIn, Gamertag: "Test Player"})
		controller.session.mu.Unlock()
		if _, err := controller.session.beginPlay(t.Context(), oldControl); err != nil {
			t.Fatal(err)
		}
		defer controller.session.endPlay()
		resumed, detach, err := relay.openControl(client, token, time.Now())
		if err != nil {
			t.Fatal("valid resume refused before the old controller detached", err)
		}
		defer func() { detach(time.Now()) }()
		if resumed.session != controller.session || resumed.generation <= oldGeneration || oldControl.Err() == nil {
			t.Fatal("resume did not retire the previous controller on the same account")
		}
		if state, _ := resumed.session.Auth(); state.State != control.AuthSignedIn {
			t.Fatal("takeover discarded the account")
		}
		newControl := resumed.ctx
		if _, err := relay.welcome(controller, true, time.Now()); !errors.Is(err, net.ErrClosed) {
			t.Fatal("retired controller published a capability")
		}
		if relay.acknowledge(controller, resumed.session.token, time.Now()) {
			t.Fatal("retired controller acknowledged the replacement capability")
		}
		if err := relay.applyAccount(controller, client, bridgecontract.RelayOpSignOut, time.Now()); !errors.Is(err, net.ErrClosed) {
			t.Fatal("retired controller changed the account", err)
		}
		if _, _, err := relay.playSession(controller, hello{Generation: resumed.generation}, time.Now()); !errors.Is(err, errResume) {
			t.Fatal("retired transport claimed the replacement play generation", err)
		}
		oldDetach(time.Now())
		oldDetach(time.Now())
		if newControl.Err() != nil {
			t.Fatal("late old detach cancelled the replacement controller")
		}
		if _, _, err := relay.playSession(nil, hello{Resume: token, Generation: oldGeneration}, time.Now()); !errors.Is(err, errResume) {
			t.Fatal("old play generation survived takeover", err)
		}
		done := make(chan error, 1)
		go func() {
			_, err := resumed.session.beginPlay(t.Context(), newControl)
			done <- err
		}()
		synctest.Wait()
		select {
		case err := <-done:
			t.Fatal("new play did not wait for old play teardown", err)
		default:
		}
		controller.session.endPlay()
		if err := <-done; err != nil {
			t.Fatal("new play could not claim the released handler", err)
		}
	})
}

func TestResumedPlayWaitsForCancelledHandler(t *testing.T) {
	for _, boundTransport := range []bool{false, true} {
		t.Run(map[bool]string{false: "websocket", true: "webtransport"}[boundTransport], func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				relay := testRelay(t)
				client := netip.MustParseAddr("192.0.2.1")
				controller, detach, err := relay.openControl(client, "", time.Now())
				if err != nil {
					t.Fatal(err)
				}
				token := controller.session.token
				oldControl := controller.ctx
				if _, err := controller.session.beginPlay(t.Context(), oldControl); err != nil {
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
				welcome := testWelcome(t, relay, resumed, false, time.Now())
				var bound *sessionController
				if boundTransport {
					bound = resumed
				}
				selected, controlCtx, err := relay.playSession(bound, hello{Resume: welcome.Resume, Generation: welcome.Generation}, time.Now())
				if err != nil || selected != controller.session {
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
				controller.session.endPlay()
				if err := <-done; err != nil {
					t.Fatal("resumed play could not restart after teardown", err)
				}
				if _, err := controller.session.beginPlay(t.Context(), controlCtx); !errors.Is(err, errSessionBusy) {
					t.Fatal("another active play connection was admitted", err)
				}
				controller.session.endPlay()
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
				controller, detach, err := relay.openControl(client, "", time.Now())
				if err != nil {
					t.Fatal(err)
				}
				token := controller.session.token
				if _, err := controller.session.beginPlay(t.Context(), controller.ctx); err != nil {
					t.Fatal(err)
				}
				defer controller.session.endPlay()
				detach(time.Now())
				resumed, detach, err := relay.openControl(client, token, time.Now())
				if err != nil {
					t.Fatal(err)
				}
				defer func() { detach(time.Now()) }()
				ctx, cancel := context.WithCancel(t.Context())
				defer cancel()
				controlCtx := resumed.ctx
				done := make(chan error, 1)
				go func() {
					_, err := resumed.session.beginPlay(ctx, controlCtx)
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
	controller, detach, err := relay.openControl(client, "", time.Now())
	if err != nil {
		t.Fatal(err)
	}
	token, oldControl := controller.session.token, controller.ctx
	detach(time.Now())
	resumed, detach, err := relay.openControl(client, token, time.Now())
	if err != nil {
		t.Fatal(err)
	}
	defer func() { detach(time.Now()) }()
	if _, err := controller.session.beginPlay(t.Context(), oldControl); !errors.Is(err, context.Canceled) {
		t.Fatal("late old-generation play claimed the resumed session", err)
	}
	if _, err := resumed.session.beginPlay(t.Context(), resumed.ctx); err != nil {
		t.Fatal("late old-generation play blocked the new generation", err)
	}
	defer controller.session.endPlay()
}
