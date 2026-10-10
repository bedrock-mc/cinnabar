package main

import (
	"context"
	"io"
	"path/filepath"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"golang.org/x/oauth2"
)

func TestPresenceCleanupRetainsCredentialLifetime(t *testing.T) {
	ctx, stop := context.WithCancel(context.Background())
	defer stop()
	var credentials context.Context
	err := run(ctx, []string{"-socket-dir", filepath.Join(t.TempDir(), "socket"),
		"-upstream", "localhost:19132", "-auth-cache", filepath.Join(t.TempDir(), "unused")},
		io.Discard, io.Discard,
		func(ctx context.Context, _ authcache.Config) (oauth2.TokenSource, error) {
			credentials = ctx
			return oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "fixture"}), nil
		}, func(_ context.Context, cfg proxy.Config) error {
			stop()
			if credentials.Err() != nil {
				t.Error("shutdown canceled credentials before title cleanup")
			}
			if _, err := cfg.Account.Token(); err != nil {
				t.Errorf("cleanup cannot authenticate: %v", err)
			}
			return nil
		})
	if err != nil {
		t.Fatal(err)
	}
	if credentials.Err() == nil {
		t.Fatal("credentials outlived core cleanup")
	}
}

func TestPresenceOwnershipIsExplicit(t *testing.T) {
	session, err := parseFlags([]string{"-control-status"}, io.Discard)
	if err != nil || session.xboxPresence {
		t.Fatalf("session took presence ownership: %v", err)
	}
	owner, err := parseFlags([]string{"-control-status", "-xbox-presence"}, io.Discard)
	if err != nil || !owner.xboxPresence {
		t.Fatalf("account did not take presence ownership: %v", err)
	}
	if _, err := parseFlags([]string{"-xbox-presence"}, io.Discard); err == nil {
		t.Fatal("presence accepted without account control")
	}
}

func TestPresenceLifetimeStillCancelsInitialAuthentication(t *testing.T) {
	ctx, stop := context.WithCancel(context.Background())
	defer stop()
	called := false
	err := run(ctx, []string{"-socket-dir", "unused", "-upstream", "localhost:19132", "-auth-cache", "unused"},
		io.Discard, io.Discard,
		func(ctx context.Context, _ authcache.Config) (oauth2.TokenSource, error) {
			called = true
			stop()
			<-ctx.Done()
			return nil, ctx.Err()
		}, func(context.Context, proxy.Config) error { t.Fatal("served after canceled sign-in"); return nil })
	if !called || err == nil {
		t.Fatal("initial authentication did not follow core cancellation")
	}
}
