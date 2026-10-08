package imagecache

import (
	"context"
	"errors"
	"net"
	"slices"
	"testing"
	"time"
)

func TestPublicDialerFallsBackWithTimeReservedForRemainingAddresses(t *testing.T) {
	for _, firstErr := range []error{errors.New("unreachable address"), context.DeadlineExceeded} {
		t.Run(firstErr.Error(), func(t *testing.T) {
			ctx, cancel := context.WithTimeout(context.Background(), time.Second)
			defer cancel()
			requestDeadline, _ := ctx.Deadline()
			client, server := net.Pipe()
			defer client.Close()
			defer server.Close()
			var addresses []string
			lookup := func(context.Context, string) ([]net.IPAddr, error) {
				return []net.IPAddr{{IP: net.ParseIP("2001:4860:4860::8888")}, {IP: net.ParseIP("8.8.8.8")}}, nil
			}
			dial := publicDialer(lookup, func(attempt context.Context, network, addr string) (net.Conn, error) {
				if network != "tcp" {
					t.Fatalf("network = %q", network)
				}
				addresses = append(addresses, addr)
				if len(addresses) == 1 {
					deadline, ok := attempt.Deadline()
					if !ok || !deadline.Before(requestDeadline) {
						t.Fatalf("first attempt left no fallback time: %v", deadline)
					}
					return nil, firstErr
				}
				if err := attempt.Err(); err != nil {
					t.Fatalf("fallback received cancelled context: %v", err)
				}
				return client, nil
			})
			conn, err := dial(ctx, "tcp", "images.example:443")
			want := []string{"[2001:4860:4860::8888]:443", "8.8.8.8:443"}
			if err != nil || conn != client || !slices.Equal(addresses, want) {
				t.Fatalf("fallback = %v, %v; addresses = %v", conn, err, addresses)
			}
		})
	}
}

func TestPublicDialerCancellationStopsResolutionOrFallback(t *testing.T) {
	for _, duringLookup := range []bool{true, false} {
		name := "dial"
		if duringLookup {
			name = "lookup"
		}
		t.Run(name, func(t *testing.T) {
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			attempts := 0
			lookup := func(context.Context, string) ([]net.IPAddr, error) {
				if duringLookup {
					cancel()
					return nil, context.Canceled
				}
				return []net.IPAddr{{IP: net.ParseIP("8.8.8.8")}, {IP: net.ParseIP("8.8.4.4")}}, nil
			}
			dial := publicDialer(lookup, func(context.Context, string, string) (net.Conn, error) {
				attempts++
				cancel()
				return nil, errors.New("interrupted dial")
			})
			conn, err := dial(ctx, "tcp", "images.example:443")
			wantAttempts := 1
			if duringLookup {
				wantAttempts = 0
			}
			if conn != nil || !errors.Is(err, context.Canceled) || attempts != wantAttempts {
				t.Fatalf("cancelled dial = %v, %v; attempts = %d", conn, err, attempts)
			}
		})
	}
}

func TestPublicDialerRejectsMixedResolutionBeforeConnecting(t *testing.T) {
	for _, unsafeIP := range []string{"127.0.0.1", "10.0.0.1", "169.254.169.254", "::1", "fd00::1"} {
		t.Run(unsafeIP, func(t *testing.T) {
			lookup := func(context.Context, string) ([]net.IPAddr, error) {
				return []net.IPAddr{{IP: net.ParseIP("8.8.8.8")}, {IP: net.ParseIP(unsafeIP)}}, nil
			}
			dial := publicDialer(lookup, func(context.Context, string, string) (net.Conn, error) {
				t.Fatal("dial started before all DNS answers were validated")
				return nil, nil
			})
			if conn, err := dial(context.Background(), "tcp", "images.example:443"); conn != nil || !errors.Is(err, ErrRejected) {
				t.Fatalf("unsafe DNS answer accepted: %v, %v", conn, err)
			}
		})
	}
}
