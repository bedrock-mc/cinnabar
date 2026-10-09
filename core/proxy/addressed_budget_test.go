package proxy

import (
	"context"
	"errors"
	"net"
	"net/http"
	"strings"
	"testing"
	"testing/synctest"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
)

// stalledUpstreamDialer waits for cancellation without opening a UDP socket.
type stalledUpstreamDialer struct{}

// DialContext models an addressed server that never accepts its connection.
func (stalledUpstreamDialer) DialContext(ctx context.Context, _, _ string) (net.Conn, error) {
	<-ctx.Done()
	return nil, ctx.Err()
}

// refusedProbeTransport models an address that does not serve NetherNet signaling.
type refusedProbeTransport struct{}

// RoundTrip refuses both signaling candidates without contacting a real server.
func (refusedProbeTransport) RoundTrip(*http.Request) (*http.Response, error) {
	return nil, errors.New("no signaling service")
}

// stalledAddressedNetwork chooses the library's real RakNet fallback around a fake UDP dialer.
func stalledAddressedNetwork() addressedServerNetwork {
	return addressedServerNetwork{minecraft.AddressNetwork{
		RakNet:     minecraft.RakNet{UpstreamDialer: stalledUpstreamDialer{}},
		HTTPClient: &http.Client{Transport: refusedProbeTransport{}},
	}}
}

// A probed RakNet fallback must fail within its own budget, before its caller expires.
func TestAddressedRakNetFallbackKeepsConnectBudget(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
		defer cancel()
		const address = "fallback.example.test:19132"
		selected, err := stalledAddressedNetwork().Select(ctx, address)
		if err != nil {
			t.Fatal(err)
		}
		_, err = dialTransportWithin(ctx, selected, address, time.Second)
		if ctx.Err() != nil || !errors.Is(err, context.DeadlineExceeded) || !strings.Contains(err.Error(), "did not accept") {
			t.Fatalf("dial err = %v, caller err = %v", err, ctx.Err())
		}
	})
}

// Preparing an addressed fallback during authentication keeps the same connect budget.
func TestPreparedAddressedRakNetKeepsConnectBudget(t *testing.T) {
	synctest.Test(t, func(t *testing.T) {
		ctx, cancel := context.WithTimeout(context.Background(), time.Minute)
		defer cancel()
		prepared := newPreparedTransport(ctx, stalledAddressedNetwork(), "prepared.example.test:19132")
		<-prepared.done
		prepared.finish(false)
		if ctx.Err() != nil || !errors.Is(prepared.err, context.DeadlineExceeded) || !strings.Contains(prepared.err.Error(), "did not accept") {
			t.Fatalf("prepared err = %v, caller err = %v", prepared.err, ctx.Err())
		}
	})
}
