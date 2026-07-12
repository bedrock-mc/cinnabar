package main

import (
	"context"
	"errors"
	"flag"
	"io"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"golang.org/x/oauth2"
)

func TestParseFlagsAuthCacheIsOptional(t *testing.T) {
	withoutAuth, err := parseFlags([]string{"-socket-dir", "run", "-upstream", "localhost:19132"}, io.Discard)
	if err != nil {
		t.Fatalf("parseFlags() without auth cache: %v", err)
	}
	if withoutAuth.authCache != "" {
		t.Fatalf("authCache = %q, want empty", withoutAuth.authCache)
	}

	withAuth, err := parseFlags([]string{"-socket-dir", "run", "-upstream", "zeqa.net:19132", "-auth-cache", ".local/auth/microsoft-token.json"}, io.Discard)
	if err != nil {
		t.Fatalf("parseFlags() with auth cache: %v", err)
	}
	if withAuth.authCache != ".local/auth/microsoft-token.json" {
		t.Fatalf("authCache = %q, want configured path", withAuth.authCache)
	}
}

func TestRunAuthFailureDoesNotStartProxy(t *testing.T) {
	wantErr := errors.New("auth failed")
	serveCalls := 0
	sourceCalls := 0
	stdout := &strings.Builder{}
	err := run(
		context.Background(),
		[]string{"-socket-dir", "run", "-upstream", "zeqa.net:19132", "-auth-cache", "token.json"},
		stdout,
		io.Discard,
		func(_ context.Context, cfg authcache.Config) (oauth2.TokenSource, error) {
			sourceCalls++
			if cfg.Path != "token.json" {
				t.Fatalf("auth cache path = %q, want token.json", cfg.Path)
			}
			if cfg.Writer != stdout {
				t.Fatal("auth cache writer is not command stdout")
			}
			return nil, wantErr
		},
		func(context.Context, proxy.Config) error {
			serveCalls++
			return nil
		},
	)
	if !errors.Is(err, wantErr) {
		t.Fatalf("run() error = %v, want auth failure", err)
	}
	if sourceCalls != 1 {
		t.Fatalf("source calls = %d, want 1", sourceCalls)
	}
	if serveCalls != 0 {
		t.Fatalf("serve calls = %d, want 0", serveCalls)
	}
}

func TestRunAuthenticatedPassesTokenSourceToProxy(t *testing.T) {
	source := oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "sentinel"})
	serveCalls := 0
	err := run(
		context.Background(),
		[]string{"-socket-dir", "run", "-upstream", "zeqa.net:19132", "-auth-cache", "token.json"},
		io.Discard,
		io.Discard,
		func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			return source, nil
		},
		func(_ context.Context, cfg proxy.Config) error {
			serveCalls++
			if cfg.TokenSource != source {
				t.Fatal("proxy token source was not preserved")
			}
			return nil
		},
	)
	if err != nil {
		t.Fatalf("run() error = %v", err)
	}
	if serveCalls != 1 {
		t.Fatalf("serve calls = %d, want 1", serveCalls)
	}
}

func TestRunOfflineSkipsAuthSource(t *testing.T) {
	serveCalls := 0
	err := run(
		context.Background(),
		[]string{"-socket-dir", "run", "-upstream", "localhost:19132"},
		io.Discard,
		io.Discard,
		func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			t.Fatal("offline mode called auth source")
			return nil, nil
		},
		func(_ context.Context, cfg proxy.Config) error {
			serveCalls++
			if cfg.TokenSource != nil {
				t.Fatal("offline proxy token source is non-nil")
			}
			return nil
		},
	)
	if err != nil {
		t.Fatalf("run() error = %v", err)
	}
	if serveCalls != 1 {
		t.Fatalf("serve calls = %d, want 1", serveCalls)
	}
}

func TestRunHelpReturnsSuccessWithoutStartingProxy(t *testing.T) {
	serveCalls := 0
	err := run(
		context.Background(),
		[]string{"-h"},
		io.Discard,
		io.Discard,
		func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			t.Fatal("help called auth source")
			return nil, nil
		},
		func(context.Context, proxy.Config) error {
			serveCalls++
			return nil
		},
	)
	if err != nil {
		t.Fatalf("run(-h) error = %v, want nil", err)
	}
	if serveCalls != 0 {
		t.Fatalf("serve calls = %d, want 0", serveCalls)
	}

	err = run(
		context.Background(),
		[]string{"-definitely-not-a-flag"},
		io.Discard,
		io.Discard,
		func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			t.Fatal("parse error called auth source")
			return nil, nil
		},
		func(context.Context, proxy.Config) error {
			t.Fatal("parse error started proxy")
			return nil
		},
	)
	if err == nil {
		t.Fatal("run(invalid flag) error = nil, want parse failure")
	}
}

func TestHelpDocumentsAuthCache(t *testing.T) {
	help := &strings.Builder{}
	_, err := parseFlags([]string{"-h"}, help)
	if !errors.Is(err, flag.ErrHelp) {
		t.Fatalf("parseFlags(-h) error = %v, want flag.ErrHelp", err)
	}
	if !strings.Contains(help.String(), "-auth-cache") {
		t.Fatalf("help text does not document -auth-cache:\n%s", help.String())
	}
}

func TestStdinEOFCancelsCoreContext(t *testing.T) {
	reader, writer := io.Pipe()
	ctx, stop := contextWithStdinEOF(context.Background(), reader)
	defer stop()

	if err := writer.Close(); err != nil {
		t.Fatalf("close core stdin: %v", err)
	}
	select {
	case <-ctx.Done():
	case <-time.After(time.Second):
		t.Fatal("stdin EOF did not cancel the core context")
	}
}

func TestParentCancellationStillStopsCoreContext(t *testing.T) {
	reader, writer := io.Pipe()
	parent, cancelParent := context.WithCancel(context.Background())
	ctx, stop := contextWithStdinEOF(parent, reader)
	defer stop()
	defer writer.Close()

	cancelParent()
	select {
	case <-ctx.Done():
	case <-time.After(time.Second):
		t.Fatal("parent cancellation did not cancel the core context")
	}
}
