package proxy

import (
	"context"
	"errors"
	"io"
	"log/slog"
	"net"
	"net/http"
	"os"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/sandertv/gophertunnel/minecraft"
	"golang.org/x/oauth2"
)

var errTokenProfileBoundary = errors.New("token profile reached the transport boundary")

type tokenProfileNetwork struct{ minecraft.RakNet }

func (tokenProfileNetwork) DialContext(context.Context, string) (net.Conn, error) {
	return nil, errTokenProfileBoundary
}

// tokenProfileTransport logs each auth request's host, path and duration; never headers or bodies.
type tokenProfileTransport struct {
	base http.RoundTripper
	t    *testing.T
	mu   sync.Mutex
}

func (transport *tokenProfileTransport) RoundTrip(request *http.Request) (*http.Response, error) {
	started := time.Now()
	response, err := transport.base.RoundTrip(request)
	status := 0
	if response != nil {
		status = response.StatusCode
	}
	transport.mu.Lock()
	transport.t.Logf("TOKEN_PROFILE request host=%s path=%s status=%d duration_ms=%.1f", request.URL.Host, request.URL.Path, status, time.Since(started).Seconds()*1000)
	transport.mu.Unlock()
	return response, err
}

// TestFirstJoinTokenLive times a fresh process's first and second key-bound join token from a reused auth cache.
// CINNABAR_JOIN_PROFILE_KEEP_WARM (a duration) starts the core's background warm-up and idles that long first.
func TestFirstJoinTokenLive(t *testing.T) {
	path := os.Getenv("CINNABAR_JOIN_PROFILE_AUTH_CACHE")
	if path == "" {
		t.Skip("missing fixture: CINNABAR_JOIN_PROFILE_AUTH_CACHE")
	}
	base := http.DefaultTransport
	http.DefaultTransport = &tokenProfileTransport{base: base, t: t}
	t.Cleanup(func() { http.DefaultTransport = base })
	ctx, cancel := context.WithTimeout(t.Context(), 60*time.Second)
	defer cancel()
	started := time.Now()
	source, err := authcache.Source(ctx, authcache.Config{Path: path, Writer: io.Discard,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			return nil, errors.New("token profile requires a reusable authentication fixture")
		}})
	if err != nil {
		t.Fatal("validate cached authentication fixture")
	}
	account := authcache.NewAccount(ctx, authcache.DerivedCachePath(path), source, os.Stderr)
	defer account.Close()
	t.Logf("TOKEN_PROFILE stage=account_open duration_ms=%.1f", time.Since(started).Seconds()*1000)
	if idle, err := time.ParseDuration(os.Getenv("CINNABAR_JOIN_PROFILE_KEEP_WARM")); err == nil {
		defer StartVerifierPreload(ctx, slog.New(slog.NewTextHandler(os.Stderr, nil)))()
		go account.KeepFresh(ctx)
		time.Sleep(idle)
		t.Logf("TOKEN_PROFILE stage=keep_warm idle_ms=%d", idle.Milliseconds())
	}
	for attempt := range 2 {
		started := time.Now()
		_, err := (minecraft.Dialer{TokenSource: account, ErrorLog: slog.New(slog.DiscardHandler)}).DialContextNetwork(ctx, tokenProfileNetwork{}, "")
		if !errors.Is(err, errTokenProfileBoundary) {
			t.Fatalf("join token attempt %d did not reach the transport boundary", attempt)
		}
		t.Logf("TOKEN_PROFILE stage=join_token attempt=%d duration_ms=%.1f", attempt, time.Since(started).Seconds()*1000)
	}
}
