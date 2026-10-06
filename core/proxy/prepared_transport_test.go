package proxy

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"errors"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sandertv/go-raknet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

type transportFixture struct {
	minecraft.Network
	dial func(context.Context, string) (net.Conn, error)
}

func (fixture transportFixture) DialContext(ctx context.Context, address string) (net.Conn, error) {
	return fixture.dial(ctx, address)
}

type countedTransport struct {
	net.Conn
	closes atomic.Int32
}

func (transport *countedTransport) Close() error {
	transport.closes.Add(1)
	return nil
}

func TestPreparedTransportStartsBeforeAuthenticationCompletes(t *testing.T) {
	started := make(chan struct{})
	release := make(chan struct{})
	transport := new(countedTransport)
	network := transportFixture{dial: func(ctx context.Context, address string) (net.Conn, error) {
		close(started)
		select {
		case <-release:
			return transport, nil
		case <-ctx.Done():
			return nil, ctx.Err()
		}
	}}
	prepared := newPreparedTransport(t.Context(), network, "server")
	defer prepared.finish(true)
	<-started
	close(release)
	conn, err := prepared.DialContext(t.Context(), "server")
	if err != nil || conn != transport {
		t.Fatalf("transport handoff = (%T, %v), want the original transport", conn, err)
	}
	prepared.finish(true)
	if transport.closes.Load() != 0 {
		t.Fatal("successful handoff closed the retained transport")
	}
	if _, err := prepared.DialContext(t.Context(), "server"); err == nil {
		t.Fatal("prepared transport was handed off twice")
	}
}

func TestPreparedTransportFailureCancelsUnfinishedSetup(t *testing.T) {
	started, stopped := make(chan struct{}), make(chan struct{})
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(ctx context.Context, _ string) (net.Conn, error) {
		close(started)
		<-ctx.Done()
		close(stopped)
		return nil, ctx.Err()
	}}, "server")
	<-started
	prepared.finish(false)
	select {
	case <-stopped:
	default:
		t.Fatal("authentication failure left transport setup running")
	}
}

func TestPreparedTransportClosesUnusedAndFailedHandoffs(t *testing.T) {
	for _, claim := range []bool{false, true} {
		t.Run(map[bool]string{false: "unused", true: "failed_login"}[claim], func(t *testing.T) {
			transport := new(countedTransport)
			prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
				return transport, nil
			}}, "server")
			if claim {
				if _, err := prepared.DialContext(t.Context(), "server"); err != nil {
					t.Fatal(err)
				}
			}
			prepared.finish(false)
			if transport.closes.Load() != 1 {
				t.Fatalf("abandoned transport closed %d times", transport.closes.Load())
			}
		})
	}
}

func TestPreparedTransportPreservesFailureAndCancellation(t *testing.T) {
	failure := errors.New("transport failed")
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return nil, failure
	}}, "server")
	defer prepared.finish(false)
	if _, err := prepared.DialContext(t.Context(), "server"); !errors.Is(err, failure) {
		t.Fatalf("transport failure = %v", err)
	}
	transport := new(countedTransport)
	cancelled := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return transport, nil
	}}, "server")
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := cancelled.DialContext(ctx, "server"); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled handoff = %v", err)
	}
	cancelled.finish(true)
	if transport.closes.Load() != 1 {
		t.Fatal("cancelled handoff retained an unused transport")
	}
}

func TestPreparedTransportTypedNilFailurePreservesErrorAndCleansUp(t *testing.T) {
	defer func() {
		if recovered := recover(); recovered != nil {
			t.Fatalf("typed-nil failed transport panicked instead of returning its error (type %T)", recovered)
		}
	}()
	want := errors.New("fixture transport unavailable")
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		var conn *raknet.Conn
		return conn, want
	}}, "server")
	conn, err := prepared.DialContext(t.Context(), "server")
	prepared.finish(false)
	prepared.finish(false)
	if conn != nil || !errors.Is(err, want) {
		t.Fatalf("failed transport: nonnil=%v err=%v", conn != nil, err)
	}
}

func TestPreparedTransportTypedNilAbandonedSetupClosesNoConnection(t *testing.T) {
	defer func() {
		if recovered := recover(); recovered != nil {
			t.Fatalf("typed-nil abandoned setup panicked during cleanup (type %T)", recovered)
		}
	}()
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		var conn *raknet.Conn
		return conn, errors.New("fixture transport unavailable")
	}}, "server")
	prepared.finish(false)
	if prepared.conn != nil {
		t.Fatal("typed-nil failed setup kept a connection")
	}
}

func TestPreparedTransportNilSuccessIsUnavailable(t *testing.T) {
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return nil, nil
	}}, "server")
	defer prepared.finish(false)
	if conn, err := prepared.DialContext(t.Context(), "server"); conn != nil || !errors.Is(err, net.ErrClosed) {
		t.Fatalf("nil success: nonnil=%v err=%v", conn != nil, err)
	}
}

func TestPreparedTransportFailedNonNilResultClosesOnce(t *testing.T) {
	transport := new(countedTransport)
	failure := errors.New("fixture transport unavailable")
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return transport, failure
	}}, "server")
	if conn, err := prepared.DialContext(t.Context(), "server"); conn != nil || !errors.Is(err, failure) {
		t.Fatalf("failed result: nonnil=%v err=%v", conn != nil, err)
	}
	prepared.finish(false)
	prepared.finish(false)
	if transport.closes.Load() != 1 {
		t.Fatalf("failed connection closed %d times", transport.closes.Load())
	}
}

func TestPreparedTransportRejectsChangedTarget(t *testing.T) {
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		return nil, nil
	}}, "server")
	defer prepared.finish(false)
	if _, err := prepared.DialContext(t.Context(), "other"); err == nil {
		t.Fatal("prepared transport accepted a different target")
	}
}

func TestPreparedTransportContainsTransportPanic(t *testing.T) {
	prepared := newPreparedTransport(t.Context(), transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		panic("private transport error")
	}}, "server")
	defer prepared.finish(false)
	_, err := prepared.DialContext(t.Context(), "server")
	if err == nil || err.Error() != "proxy: panic while preparing upstream transport (type string)" {
		t.Fatalf("transport panic = %v", err)
	}
}

func TestPreparedTransportLeavesIdentityNetworksUntouched(t *testing.T) {
	network := transportFixture{dial: func(context.Context, string) (net.Conn, error) {
		t.Fatal("identity transport started before authentication")
		return nil, nil
	}}
	_, err := dialWithPreparedTransport(t.Context(), network, "server", func(_ context.Context, actual minecraft.Network, _ string) (*minecraft.Conn, error) {
		if _, ok := actual.(transportFixture); !ok {
			t.Fatal("non-RakNet transport was wrapped")
		}
		return nil, nil
	})
	if err != nil {
		t.Fatal(err)
	}
}

func BenchmarkJoinTransportOverlap(b *testing.B) {
	for _, overlap := range []bool{false, true} {
		b.Run(map[bool]string{false: "serial", true: "overlap"}[overlap], func(b *testing.B) {
			network := transportFixture{dial: func(ctx context.Context, _ string) (net.Conn, error) {
				select {
				case <-time.After(20 * time.Millisecond):
					return new(countedTransport), nil
				case <-ctx.Done():
					return nil, ctx.Err()
				}
			}}
			for b.Loop() {
				var prepared *preparedTransport
				if overlap {
					prepared = newPreparedTransport(b.Context(), network, "server")
				}
				time.Sleep(30 * time.Millisecond)
				if prepared == nil {
					conn, err := network.DialContext(b.Context(), "server")
					if err != nil {
						b.Fatal(err)
					}
					_ = conn.Close()
				} else {
					if _, err := prepared.DialContext(b.Context(), "server"); err != nil {
						b.Fatal(err)
					}
					prepared.finish(false)
				}
			}
		})
	}
}

// Authentication slower than the upstream's login deadline must not fail the join on the expired prefix.
func TestPreparedTransportRedialsAfterUpstreamLoginTimeout(t *testing.T) {
	for _, authDelay := range []time.Duration{160 * time.Millisecond, time.Second} {
		t.Run(authDelay.String(), func(t *testing.T) { redialAfterLoginTimeout(t, authDelay) })
	}
}

func redialAfterLoginTimeout(t *testing.T, authDelay time.Duration) {
	listener, err := minecraft.ListenConfig{
		AuthenticationDisabled: true,
		LoginTimeout:           150 * time.Millisecond,
		ErrorLog:               slog.New(slog.DiscardHandler),
	}.Listen("raknet", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	serving, stopServing := context.WithCancel(t.Context())
	defer func() {
		stopServing()
		_ = listener.Close()
		// Later tests assert that no listener connection outlives its owner.
		waitForGoroutineStack(t, "minecraft.(*Listener).handleConn", false, 5*time.Second)
	}()
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			go func() {
				_ = conn.(*minecraft.Conn).StartGameContext(serving, minecraft.GameData{})
				<-serving.Done()
				_ = conn.Close()
			}()
		}
	}()
	ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
	defer cancel()
	address := listener.Addr().String()
	connection, err := dialWithPreparedTransport(ctx, minecraft.RakNet{}, address, func(ctx context.Context, network minecraft.Network, address string) (*minecraft.Conn, error) {
		// Stands in for credential acquisition that outlasts the upstream login deadline.
		time.Sleep(authDelay)
		return minecraft.Dialer{IdentityData: login.IdentityData{DisplayName: "fixture"}, ErrorLog: slog.New(slog.DiscardHandler)}.DialContextNetwork(ctx, network, address)
	})
	if err != nil {
		t.Fatalf("join after slow authentication = %v, want a fresh transport", err)
	}
	_ = connection.Close()
}

// The NetherNet probe of an addressed server runs while authentication is still pending, and the
// chosen NetherNet dial then presents the login identity.
func TestAddressedServerProbesDuringAuthenticationAndDialsNetherNetWithIdentity(t *testing.T) {
	probed := make(chan struct{}, 1)
	offers := make(chan string, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodGet && r.URL.Path == "/v1/join" {
			probed <- struct{}{}
			return
		}
		body, _ := io.ReadAll(r.Body)
		offers <- string(body)
		http.Error(w, "fixture ends the negotiation", http.StatusInternalServerError)
	}))
	defer server.Close()
	address := server.Listener.Addr().String()
	prepared := newPreparedTransport(t.Context(), remoteServerNetwork(slog.New(slog.DiscardHandler), nil), address)
	defer prepared.finish(false)
	select {
	case <-probed:
	case <-time.After(2 * time.Second):
		t.Fatal("probe did not start before authentication finished")
	}

	key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	if conn, err := prepared.DialContextIdentityProvider(ctx, address, syntheticMultiplayerToken(t, &key.PublicKey), key, "https://authorization.example/"); err == nil {
		_ = conn.Close()
		t.Fatal("dial succeeded against a fixture that rejects the offer")
	}
	select {
	case offer := <-offers:
		if !strings.Contains(offer, "a=identity") {
			t.Fatal("NetherNet offer omitted the login identity")
		}
	default:
		t.Fatal("dial never signaled a NetherNet offer")
	}
}

// Without HTTP signaling the addressed server falls back to RakNet, still dialed during authentication.
func TestAddressedServerWithoutSignalingPreDialsRakNet(t *testing.T) {
	server, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	prepared := newPreparedTransport(t.Context(), remoteServerNetwork(slog.New(slog.DiscardHandler), nil), server.LocalAddr().String())
	defer prepared.finish(false)
	buffer := make([]byte, 2048)
	_ = server.SetReadDeadline(time.Now().Add(2 * time.Second))
	if _, _, err := server.ReadFrom(buffer); err != nil {
		t.Fatalf("no RakNet dial before authentication finished: %v", err)
	}
	const openConnectionRequest1 = 0x05
	if buffer[0] != openConnectionRequest1 {
		t.Fatalf("first datagram id %#x, want an Open Connection Request 1", buffer[0])
	}
}
