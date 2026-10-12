package proxy

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// A handler's dialer carries the target and every pre-login transfer hop, so a server cannot
// transfer a session past the caller's policy.
func TestSessionHandlerJoinsAndHopsThroughItsDialer(t *testing.T) {
	dialer := new(net.Dialer)
	handler, err := NewSessionHandler(SessionHandlerConfig{Dialer: dialer, Logger: slog.New(slog.DiscardHandler)})
	if err != nil {
		t.Fatal(err)
	}
	defer handler.Close()
	target, err := handler.server.dialTarget(t.Context(), "entry.example:19132")
	if err != nil {
		t.Fatal(err)
	}
	for _, address := range []string{"entry.example:19132", "hop.example:19132"} {
		network, ok := handler.server.prepared.hopNetwork(target, address).(minecraft.RakNet)
		if !ok || network.UpstreamDialer != dialer {
			t.Fatalf("%s joins through %#v, want RakNet over the handler's dialer", address, network)
		}
	}
	if _, err := NewSessionHandler(SessionHandlerConfig{}); err == nil {
		t.Fatal("a handler without a dialer was created")
	}
}

var errPolicyRefused = errors.New("refused by policy")

// policyDialer opens connections to allowed only.
type policyDialer struct{ allowed string }

func (d policyDialer) DialContext(ctx context.Context, network, address string) (net.Conn, error) {
	if address != d.allowed {
		return nil, errPolicyRefused
	}
	return new(net.Dialer).DialContext(ctx, network, address)
}

// A server's pack URL and every redirect it answers with dial through the handler's dialer, so a
// download cannot reach a host the dialer refuses.
func TestSessionHandlerDownloadsPacksThroughItsDialer(t *testing.T) {
	var reached atomic.Bool
	internal := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) { reached.Store(true) }))
	defer internal.Close()
	public := httptest.NewServer(http.RedirectHandler(internal.URL+"/pack.zip", http.StatusFound))
	defer public.Close()
	handler, err := NewSessionHandler(SessionHandlerConfig{
		Dialer: policyDialer{allowed: public.Listener.Addr().String()},
		Logger: slog.New(slog.DiscardHandler),
	})
	if err != nil {
		t.Fatal(err)
	}
	defer handler.Close()
	prepared := handler.server.prepared
	prepared.resolveTarget = func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{address: "entry.example:19132", network: minecraft.RakNet{}}, nil
	}
	var client *http.Client
	prepared.dialTarget = func(_ context.Context, _ *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
		client = dialer.HTTPClient
		return nil, errors.New("stop")
	}
	if _, err := prepared.connect(t.Context(), dialerTestDownstream{protocol: minecraft.DefaultProtocol}); err == nil {
		t.Fatal("connect succeeded without an upstream")
	}
	if client == nil {
		t.Fatal("the upstream Dialer downloads packs with http.DefaultClient")
	}
	if _, err := resource.ReadURLWithProgress(t.Context(), client, public.URL+"/pack.zip", 1<<20, nil); !errors.Is(err, errPolicyRefused) {
		t.Fatalf("download redirected to a refused host: %v, want the dialer's refusal", err)
	}
	if reached.Load() {
		t.Fatal("a redirect reached a host the dialer refuses")
	}
}
