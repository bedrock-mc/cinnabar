package main

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"slices"
	"testing"
	"testing/synctest"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"golang.org/x/oauth2"
)

// fakeFriendHost records lifecycle actions without a service connection or listener.
type fakeFriendHost struct {
	session localworld.Session
	events  *[]string
	done    chan struct{}
	hold    <-chan struct{}
	cancel  <-chan struct{}
	closed  bool
	invites int
}

// Invite records delivery to the installed session.
func (f *fakeFriendHost) Invite(context.Context, string) error {
	f.invites++
	return nil
}

// Done can hold the reconciler before its wait so replacement reports deterministically coalesce.
func (f *fakeFriendHost) Done() <-chan struct{} {
	if f.hold != nil {
		select {
		case <-f.hold:
		case <-f.cancel:
		}
	}
	return f.done
}

// Close records withdrawal before the owning Xbox client may close.
func (f *fakeFriendHost) Close() error {
	f.closed = true
	*f.events = append(*f.events, "host:"+f.session.World.ID)
	return nil
}

// hostingCloser records the Xbox close after the host's close.
type hostingCloser func() error

// Close executes the fixture's teardown assertion.
func (close hostingCloser) Close() error { return close() }

// reportHostedWorld publishes a running original fixture, without any network access.
func reportHostedWorld(t *testing.T, local *localworld.Local, world localworld.World, port string) {
	t.Helper()
	if _, err := local.SetLocal(localworld.Status{
		State: localworld.StateRunning, World: &world,
		Target: &localworld.ConnectionTarget{Address: "127.0.0.1:" + port, Transport: localworld.TransportRakNet},
	}); err != nil {
		t.Fatal(err)
	}
}

// Coalesced reports and a late startup both replace advertising and prevent old-session routing.
func TestFriendHostingFollowsTheAdvertisedOpening(t *testing.T) {
	for _, scenario := range []string{"coalesced worlds", "same world reopened", "late startup"} {
		t.Run(scenario, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				ctx, cancel := context.WithCancel(context.Background())
				account := authcache.NewAccount(ctx, "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "fixture-account"}), nil)
				defer account.Close()
				local := localworld.NewLocal()
				first := localworld.World{ID: "world-a", Name: "World A", GameMode: localworld.GameModeSurvival, Backend: localworld.BackendBDS}
				reportHostedWorld(t, local, first, "5000")
				var events []string
				var hosts []*fakeFriendHost
				release := make(chan struct{})
				h := &friendHosting{
					account: account, worlds: local, log: slog.New(slog.DiscardHandler),
					open: func(startCtx context.Context, session localworld.Session) (hostedFriends, io.Closer, error) {
						host := &fakeFriendHost{session: session, events: &events, done: make(chan struct{})}
						hosts = append(hosts, host)
						events = append(events, "open:"+session.World.ID)
						if scenario != "late startup" && len(hosts) == 1 {
							host.hold, host.cancel = release, ctx.Done()
						}
						if scenario == "late startup" && len(hosts) == 1 {
							select {
							case <-release:
							case <-startCtx.Done():
							}
						}
						return host, hostingCloser(func() error {
							if !host.closed {
								t.Error("Xbox client closed before its host")
							}
							events = append(events, "xbox:"+session.World.ID)
							return nil
						}), nil
					},
				}
				done := make(chan struct{})
				go func() { defer close(done); h.run(ctx) }()
				defer func() { cancel(); synctest.Wait(); <-done }()
				synctest.Wait()
				if len(hosts) != 1 {
					t.Fatalf("initial host attempts = %d", len(hosts))
				}
				if scenario != "late startup" {
					if err := h.Invite(ctx, "fixture-friend"); err != nil || hosts[0].invites != 1 {
						t.Fatalf("current host invite = %v", err)
					}
				}
				second := first
				if scenario != "same world reopened" {
					second.ID, second.Name = "world-b", "World B"
				}
				// Both reports arrive before the hosting goroutine observes either one.
				if _, err := local.SetLocal(localworld.Status{State: localworld.StateIdle}); err != nil {
					t.Fatal(err)
				}
				reportHostedWorld(t, local, second, "6000")
				if _, ok, err := hosts[0].session.ConnectionTarget(ctx); ok || !errors.Is(err, localworld.ErrNotOpen) {
					t.Fatalf("old session reached a replacement: ok=%v err=%v", ok, err)
				}
				if err := h.Invite(ctx, "fixture-friend"); !errors.Is(err, localworld.ErrNotOpen) {
					t.Fatalf("obsolete host invite = %v", err)
				}
				close(release)
				synctest.Wait()
				if len(hosts) != 2 || !hosts[0].closed || hosts[1].closed {
					t.Fatalf("replacement host lifecycle: attempts=%d", len(hosts))
				}
				if host, session := h.current(); host != hosts[1] || session != hosts[1].session || session.World != second {
					t.Fatal("the installed host advertises an obsolete world")
				}
				if target, ok, err := hosts[1].session.ConnectionTarget(ctx); err != nil || !ok || target.Address != "127.0.0.1:6000" {
					t.Fatalf("replacement target = %+v, %v, %v", target, ok, err)
				}
				want := []string{"open:" + first.ID, "host:" + first.ID, "xbox:" + first.ID, "open:" + second.ID}
				if !slices.Equal(events, want) {
					t.Fatalf("host cleanup order = %v, want %v", events, want)
				}
				// A repeated report of the same opening keeps the installed host.
				reportHostedWorld(t, local, second, "6000")
				synctest.Wait()
				if len(hosts) != 2 {
					t.Fatal("an unchanged running report restarted hosting")
				}
				cancel()
				synctest.Wait()
				if !hosts[1].closed {
					t.Fatal("shutdown retained the current host")
				}
			})
		})
	}
}

// Turning multiplayer off withdraws hosting and invalidates admission through the old session.
func TestFriendHostingWithdrawsSinglePlayerWorlds(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctx, cancel := context.WithCancel(context.Background())
		account := authcache.NewAccount(ctx, "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "fixture-account"}), nil)
		defer account.Close()
		local := localworld.NewLocal()
		world := localworld.World{ID: "private-world", Name: "Private", GameMode: localworld.GameModeSurvival, Backend: localworld.BackendBDS, MultiplayerDisabled: true}
		reportHostedWorld(t, local, world, "5000")
		var events []string
		var hosts []*fakeFriendHost
		h := &friendHosting{account: account, worlds: local, log: slog.New(slog.DiscardHandler),
			open: func(_ context.Context, session localworld.Session) (hostedFriends, io.Closer, error) {
				host := &fakeFriendHost{session: session, events: &events, done: make(chan struct{})}
				hosts = append(hosts, host)
				return host, nil, nil
			},
		}
		done := make(chan struct{})
		go func() { defer close(done); h.run(ctx) }()
		defer func() { cancel(); synctest.Wait(); <-done }()
		synctest.Wait()
		if len(hosts) != 0 {
			t.Fatal("single-player world was advertised")
		}
		world.MultiplayerDisabled = false
		reportHostedWorld(t, local, world, "5000")
		synctest.Wait()
		if len(hosts) != 1 {
			t.Fatalf("host count = %d", len(hosts))
		}
		previous := hosts[0].session
		world.MultiplayerDisabled = true
		reportHostedWorld(t, local, world, "5000")
		if _, ok, err := previous.ConnectionTarget(ctx); ok || !errors.Is(err, localworld.ErrNotOpen) {
			t.Fatalf("single-player admission = %v, %v", ok, err)
		}
		synctest.Wait()
		if !hosts[0].closed {
			t.Fatal("friend session was not withdrawn")
		}
		if err := h.Invite(ctx, "fixture-friend"); !errors.Is(err, localworld.ErrNotOpen) {
			t.Fatalf("invite = %v", err)
		}
	})
}
