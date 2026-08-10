package main

import (
	"bytes"
	"context"
	"errors"
	"flag"
	"io"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/packcache"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/sandertv/gophertunnel/minecraft"
	"golang.org/x/oauth2"
)

func TestRunReportsOrderedStartupLifecycle(t *testing.T) {
	var stderr bytes.Buffer
	source := oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "secret-token-sentinel"})
	err := run(
		context.Background(),
		[]string{"-socket-dir", "run", "-upstream", "zeqa.net:19132", "-auth-cache", "token.json"},
		io.Discard,
		&stderr,
		func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			return source, nil
		},
		func(_ context.Context, cfg proxy.Config) error {
			if cfg.Logger == nil {
				t.Fatal("proxy logger is nil")
			}
			cfg.Logger.Info("listener ready; waiting for local Rust client", "socket_dir", cfg.SocketDir, "network", "tcp", "endpoint", "127.0.0.1:43123")
			cfg.Logger.Info("local client accepted", "socket_dir", cfg.SocketDir)
			cfg.Logger.Info("upstream connection starting", "target", cfg.Upstream, "authentication", "microsoft")
			cfg.Logger.Info("upstream connected", "target", cfg.Upstream, "authentication", "microsoft")
			return nil
		},
	)
	if err != nil {
		t.Fatalf("run() error = %v", err)
	}

	output := stderr.String()
	assertTextInOrder(t, output,
		"msg=\"core starting\" endpoint=run upstream=zeqa.net:19132",
		"msg=\"authentication starting\" mode=microsoft",
		"msg=\"authentication ready\" mode=microsoft",
		"msg=\"listener ready; waiting for local Rust client\" socket_dir=run network=tcp endpoint=127.0.0.1:43123",
		"msg=\"local client accepted\" socket_dir=run",
		"msg=\"upstream connection starting\" target=zeqa.net:19132 authentication=microsoft",
		"msg=\"upstream connected\" target=zeqa.net:19132 authentication=microsoft",
	)
	if strings.Contains(output, "secret-token-sentinel") || strings.Contains(output, "token.json") {
		t.Fatalf("startup output exposed credential material or its cache path:\n%s", output)
	}
}

func TestExecuteReportsFatalStartupError(t *testing.T) {
	var stderr bytes.Buffer
	wantErr := errors.New("listener bind failed")
	exitCode := execute(
		context.Background(),
		[]string{"-socket-dir", "run", "-upstream", "localhost:19132"},
		io.Discard,
		&stderr,
		func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			t.Fatal("offline mode called auth source")
			return nil, nil
		},
		func(context.Context, proxy.Config) error { return wantErr },
	)
	if exitCode != 1 {
		t.Fatalf("execute() exit code = %d, want 1", exitCode)
	}
	assertTextInOrder(t, stderr.String(),
		"msg=\"core starting\" endpoint=run upstream=localhost:19132",
		"msg=\"authentication ready\" mode=offline",
		"level=ERROR msg=\"core failed\" error=\"listener bind failed\"",
	)
}

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

func TestResourcePackCacheFlagsAreOptInAndBounded(t *testing.T) {
	disabled, err := parseFlags(nil, io.Discard)
	if err != nil {
		t.Fatalf("parse disabled flags: %v", err)
	}
	if disabled.resourcePackCacheDir != "" || disabled.resourcePackCacheQuota != packcache.DefaultQuota {
		t.Fatalf("disabled cache options = %#v", disabled)
	}
	enabled, err := parseFlags([]string{"-resource-pack-cache-dir", "packs"}, io.Discard)
	if err != nil {
		t.Fatalf("parse enabled flags: %v", err)
	}
	if enabled.resourcePackCacheDir != "packs" || enabled.resourcePackCacheQuota != packcache.DefaultQuota {
		t.Fatalf("enabled cache options = %#v", enabled)
	}
	for _, args := range [][]string{
		{"-resource-pack-cache-quota-bytes", "1"},
		{"-resource-pack-cache-dir", "packs", "-resource-pack-cache-quota-bytes", "0"},
	} {
		if _, err := parseFlags(args, io.Discard); err == nil {
			t.Fatalf("parseFlags(%v) succeeded", args)
		}
	}
}

func TestControlStatusIsOptInAndOwnedByCoreRun(t *testing.T) {
	disabled, err := parseFlags(nil, io.Discard)
	if err != nil || disabled.controlStatus {
		t.Fatalf("disabled control options = %#v, %v", disabled, err)
	}
	dir := t.TempDir()
	err = run(
		context.Background(),
		[]string{"-socket-dir", dir, "-upstream", "localhost:19132", "-control-status"},
		io.Discard,
		io.Discard,
		func(context.Context, authcache.Config) (oauth2.TokenSource, error) { return nil, nil },
		func(_ context.Context, cfg proxy.Config) error {
			if cfg.ResourcePackAdmissionUpdate == nil {
				t.Fatal("proxy did not receive latest-status callback")
			}
			if _, _, resolveErr := streamnet.ResolveControl(dir); resolveErr != nil {
				t.Fatalf("control endpoint was not available during Serve: %v", resolveErr)
			}
			return nil
		},
	)
	if err != nil {
		t.Fatalf("run() = %v", err)
	}
	if _, _, err := streamnet.ResolveControl(dir); err == nil {
		t.Fatal("control endpoint remained after core shutdown")
	}
}

func TestRunOwnsAndClosesResourcePackCacheOnServeFailure(t *testing.T) {
	root := "private-cache-root-sentinel"
	wantErr := errors.New("serve failed")
	cache := new(closingResourcePackCacheStub)
	err := runWithResourcePackCacheFactory(context.Background(), []string{
		"-socket-dir", "run", "-upstream", "localhost:19132", "-resource-pack-cache-dir", root,
	}, io.Discard, io.Discard,
		func(context.Context, authcache.Config) (oauth2.TokenSource, error) { return nil, nil },
		func(_ context.Context, cfg proxy.Config) error {
			if cfg.ResourcePackCache == nil || cfg.ResourcePackAdmission == nil {
				t.Fatal("proxy did not receive cache interface and admission callback")
			}
			return wantErr
		},
		func(gotRoot string, _ ...packcache.Option) (ownedResourcePackCache, error) {
			if gotRoot != root {
				t.Fatalf("cache root = %q", gotRoot)
			}
			return cache, nil
		})
	if !errors.Is(err, wantErr) {
		t.Fatalf("run() error = %v, want serve failure", err)
	}
	if !cache.closed {
		t.Fatal("cache was not closed after Serve failure")
	}
}

type closingResourcePackCacheStub struct {
	minecraft.ResourcePackCache
	closed bool
}

func (cache *closingResourcePackCacheStub) Close() error {
	cache.closed = true
	return nil
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

func TestAuthEventsRequiresCacheAndIsMutuallyExclusive(t *testing.T) {
	for _, args := range [][]string{
		{"-auth-events"},
		{"-auth-events", "-auth-cache", "token.json", "-upstream", "example.test:19132"},
		{"-auth-events", "-auth-cache", "token.json", "-catalog-file", "catalog.json"},
	} {
		err := run(context.Background(), args, io.Discard, io.Discard,
			func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
				t.Fatal("normal auth source called in auth-events mode")
				return nil, nil
			},
			func(context.Context, proxy.Config) error {
				t.Fatal("proxy called in auth-events mode")
				return nil
			})
		if err == nil {
			t.Fatalf("run(%v) succeeded", args)
		}
	}
}

func TestOneShotModesDoNotBindTheirLifetimeToStdin(t *testing.T) {
	for _, args := range [][]string{
		{"-auth-events", "-auth-cache", "token.json"},
		{"-catalog-file", "catalog.json", "-auth-cache", "token.json"},
	} {
		if !catalogMode(args) {
			t.Fatalf("catalogMode(%v) = false", args)
		}
	}
	if catalogMode([]string{"-auth-cache", "token.json"}) {
		t.Fatal("long-running proxy was classified as one-shot")
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

func assertTextInOrder(t *testing.T, text string, parts ...string) {
	t.Helper()
	position := 0
	for _, part := range parts {
		next := strings.Index(text[position:], part)
		if next < 0 {
			t.Fatalf("output missing %q after byte %d:\n%s", part, position, text)
		}
		position += next + len(part)
	}
}
