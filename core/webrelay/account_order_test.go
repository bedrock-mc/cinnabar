package webrelay

import (
	"context"
	"errors"
	"net"
	"net/netip"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
	"golang.org/x/oauth2"
)

// relayOAuthSource supplies an in-memory credential read controlled by the test.
type relayOAuthSource func() (*oauth2.Token, error)

// Token returns the test's next credential response.
func (source relayOAuthSource) Token() (*oauth2.Token, error) { return source() }

func TestAccountCleanupDoesNotBlockOtherSessions(t *testing.T) {
	relay := testRelay(t)
	client := netip.MustParseAddr("192.0.2.1")
	controller, detach, err := relay.openControl(client, "", time.Now())
	if err != nil {
		t.Fatal(err)
	}
	defer func() { detach(time.Now()) }()
	token := testWelcome(t, relay, controller, false, time.Now()).Resume
	started, release := make(chan struct{}), make(chan struct{})
	unblock := sync.OnceFunc(func() { close(release) })
	defer unblock()
	initial := true
	account := authcache.NewAccount(relay.ctx, "", relayOAuthSource(func() (*oauth2.Token, error) {
		if initial {
			initial = false
		} else {
			close(started)
			<-release
		}
		return &oauth2.Token{AccessToken: "test", Expiry: time.Now().Add(time.Hour)}, nil
	}), nil)
	controller.session.account = account
	controller.session.closeAccount = func() {}
	readDone := make(chan error, 1)
	go func() { _, err := account.Token(); readDone <- err }()
	<-started
	signOutDone := make(chan error, 1)
	go func() {
		signOutDone <- relay.applyAccount(controller, client, bridgecontract.RelayOpSignOut, time.Now())
	}()
	defer func() {
		unblock()
		if err := <-readDone; !errors.Is(err, context.Canceled) {
			t.Error("OAuth read survived account cancellation", err)
		}
		if err := <-signOutDone; err != nil {
			t.Error("sign-out failed", err)
		}
	}()
	testwait.Eventually(t, testwait.DefaultTimeout, "account close", account.Closed)

	// Other operations on this same account must also wait outside the shared lock.
	takeoverDone := make(chan *sessionController, 1)
	go func() {
		resumed, detach, err := relay.openControl(client, token, time.Now())
		if err == nil {
			detach(time.Now())
		}
		takeoverDone <- resumed
	}()
	expired := make(chan struct{})
	go func() { relay.expire(time.Now()); close(expired) }()
	detached := make(chan struct{})
	go func() { detach(time.Now()); close(detached) }()

	otherDone := make(chan error, 1)
	go func() {
		other := netip.MustParseAddr("198.51.100.1")
		release, err := relay.admit(other, time.Now())
		if err != nil {
			otherDone <- err
			return
		}
		defer release()
		current, detach, err := relay.openControl(other, "", time.Now())
		if err != nil {
			otherDone <- err
			return
		}
		oldDetach := detach
		defer func() { oldDetach(time.Now()) }()
		welcome, err := relay.welcome(current, true, time.Now())
		if err == nil {
			current, detach, err = relay.openControl(other, welcome.Resume, time.Now())
			if err == nil {
				defer func() { detach(time.Now()) }()
				welcome, err = relay.welcome(current, true, time.Now())
			}
		}
		if err == nil && !relay.acknowledge(current, welcome.Resume, time.Now()) {
			err = errors.New("unrelated controller's pong was refused")
		}
		otherDone <- err
	}()
	testwait.Eventually(t, testwait.DefaultTimeout, "unrelated admission, resume and pong during account cleanup", func() bool {
		select {
		case err := <-otherDone:
			if err != nil {
				t.Fatal(err)
			}
			return true
		default:
			return false
		}
	})
	select {
	case <-takeoverDone:
		t.Fatal("takeover bypassed an unfinished account command")
	default:
	}

	closed := make(chan struct{})
	go func() { relay.Close(); close(closed) }()
	testwait.Eventually(t, testwait.DefaultTimeout, "relay transport cancellation during account cleanup", func() bool { return relay.ctx.Err() != nil })
	unblock()
	<-closed
	<-expired
	<-detached
	<-takeoverDone
	if err := relay.applyAccount(controller, client, bridgecontract.RelayOpSignOut, time.Now()); !errors.Is(err, net.ErrClosed) {
		t.Fatal("cancelled controller changed the account", err)
	}
}
