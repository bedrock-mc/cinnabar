package webrelay

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"testing"
	"testing/synctest"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
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
				if _, err := session.beginPlay(t.Context(), oldControl, 0); err != nil {
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
					_, err := selected.beginPlay(t.Context(), controlCtx, 0)
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
				if _, err := session.beginPlay(t.Context(), controlCtx, 0); !errors.Is(err, errSessionBusy) {
					t.Fatal("another active play connection was admitted", err)
				}
				session.endPlay()
			})
		})
	}
}

func TestReplacementPlayWaitStopsOnCancellation(t *testing.T) {
	for _, sameController := range []bool{false, true} {
		for _, cancellation := range []string{"caller", "control", "relay"} {
			name := map[bool]string{false: "resumed", true: "same-controller"}[sameController] + "/" + cancellation
			t.Run(name, func(t *testing.T) {
				synctest.Test(t, func(t *testing.T) {
					relay := testRelay(t)
					client := netip.MustParseAddr("192.0.2.1")
					session, detach, err := relay.openControl(client, "", time.Now())
					if err != nil {
						t.Fatal(err)
					}
					token := session.token
					if _, err := session.beginPlay(t.Context(), session.controlCtx, 1); err != nil {
						t.Fatal(err)
					}
					if !session.claimPackJoin(session.controlCtx, 1) {
						t.Fatal("first join refused")
					}
					defer session.endPlay()
					if !sameController {
						detach(time.Now())
						session, detach, err = relay.openControl(client, token, time.Now())
						if err != nil {
							t.Fatal(err)
						}
					}
					defer func() { detach(time.Now()) }()
					ctx, cancel := context.WithCancel(t.Context())
					defer cancel()
					controlCtx := session.controlCtx
					done := make(chan error, 1)
					go func() {
						_, err := session.beginPlay(ctx, controlCtx, 2)
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
	if _, err := session.beginPlay(t.Context(), oldControl, 0); !errors.Is(err, context.Canceled) {
		t.Fatal("late old-generation play claimed the resumed session", err)
	}
	if _, err := resumed.beginPlay(t.Context(), resumed.controlCtx, 0); err != nil {
		t.Fatal("late old-generation play blocked the new generation", err)
	}
	defer session.endPlay()
}

func TestSequentialPlayWaitsForSameControllerTeardown(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		relay := testRelay(t)
		session, detach, err := relay.openControl(netip.MustParseAddr("192.0.2.1"), "", time.Now())
		if err != nil {
			t.Fatal(err)
		}
		defer func() { detach(time.Now()) }()
		if _, err := session.beginPlay(t.Context(), session.controlCtx, 1); err != nil {
			t.Fatal(err)
		}
		if !session.claimPackJoin(session.controlCtx, 1) {
			t.Fatal("first join refused")
		}
		if _, err := session.beginPlay(t.Context(), session.controlCtx, 1); !errors.Is(err, errSessionBusy) {
			t.Fatal("duplicate active join admitted", err)
		}
		previous := session.play
		done := make(chan error, 1)
		// A replacement connection may arrive before the old handler sees its stream reset.
		go func() {
			conn, peer := net.Pipe()
			defer peer.Close()
			read := make(chan error, 1)
			go func() {
				framed := streamnet.NewFramedConn(peer)
				if _, err := framed.Write(append([]byte{bridgecontract.SessionKindConnect}, []byte("{}")...)); err != nil {
					read <- err
					return
				}
				frame, err := framed.ReadPacket()
				if err == nil && (len(frame) == 0 || frame[0] != bridgecontract.SessionKindDisconnect) {
					err = errors.New("missing core refusal")
				}
				read <- err
			}()
			relay.runPlay(t.Context(), conn, session.client, session, hello{Generation: session.generation, Join: 2})
			done <- <-read
		}()
		synctest.Wait()
		select {
		case err := <-done:
			t.Fatal("replacement ended before teardown", err)
		default:
		}
		if session.play != previous {
			t.Fatal("old handler lost its slot before teardown")
		}
		session.endPlay()
		if err := <-done; err != nil {
			t.Fatal("replacement failed after teardown", err)
		}
	})
}
