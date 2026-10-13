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
	"testing/synctest"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
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
		network, ok := handler.server.prepared.hopNetwork(target, address).(sessionRakNet)
		if !ok || network.UpstreamDialer != dialer {
			t.Fatalf("%s joins through %#v, want RakNet over the handler's dialer", address, network)
		}
	}
	if _, err := NewSessionHandler(SessionHandlerConfig{}); err == nil {
		t.Fatal("a handler without a dialer was created")
	}
}

// silentUpstreamDialer opens an in-memory socket that never receives a server reply.
type silentUpstreamDialer struct{}

// DialContext lets the real RakNet handshake run without contacting a server.
func (silentUpstreamDialer) DialContext(context.Context, string, string) (net.Conn, error) {
	conn, peer := net.Pipe()
	return silentUpstreamConn{Conn: conn, peer: peer}, nil
}

// silentUpstreamConn preserves socket deadlines while discarding outgoing datagrams.
type silentUpstreamConn struct {
	net.Conn
	peer net.Conn
}

// Write accepts each datagram without sending a reply to the socket's reader.
func (silentUpstreamConn) Write(data []byte) (int, error) { return len(data), nil }

// Close releases both ends of the in-memory socket.
func (conn silentUpstreamConn) Close() error {
	return errors.Join(conn.Conn.Close(), conn.peer.Close())
}

// Signed-out joins and transfer hops must time out on the RakNet budget before their caller expires.
func TestSessionHandlerSignedOutRakNetKeepsConnectBudget(t *testing.T) {
	for _, address := range []string{"entry.example:19132", "hop.example:19132"} {
		t.Run(address, func(t *testing.T) {
			synctest.Test(t, func(t *testing.T) {
				ctx, cancel := context.WithTimeout(context.Background(), 2*rakNetConnectBudget)
				defer cancel()
				handler, err := NewSessionHandler(SessionHandlerConfig{
					Dialer: silentUpstreamDialer{},
					Logger: slog.New(slog.DiscardHandler),
				})
				if err != nil {
					t.Fatal(err)
				}
				defer handler.Close()
				target, err := handler.server.dialTarget(ctx, "entry.example:19132")
				if err != nil {
					t.Fatal(err)
				}
				network := handler.server.prepared.hopNetwork(target, address)
				dialer := minecraft.Dialer{
					IdentityData: login.IdentityData{DisplayName: "fixture"},
					ErrorLog:     slog.New(slog.DiscardHandler),
				}
				conn, err := dialer.DialContextNetwork(ctx, network, address)
				if conn != nil {
					_ = conn.Close()
					t.Fatal("joined a server that sends no replies")
				}
				if ctx.Err() != nil || !errors.Is(err, context.DeadlineExceeded) {
					t.Fatalf("dial err = %v, caller err = %v", err, ctx.Err())
				}
			})
		})
	}
}

var errPolicyRefused = errors.New("refused by policy")

// policyDialer opens connections to allowed only.
type policyDialer struct{ allowed string }

// DialContext enforces the test destination policy after each redirect.
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
