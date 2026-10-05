package proxy

import (
	"context"
	"errors"
	"net"
	"sync/atomic"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
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
