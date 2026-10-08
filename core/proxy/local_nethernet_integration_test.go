//go:build integration

package proxy

import (
	"bytes"
	"context"
	"io"
	"log/slog"
	"net"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/endpoint"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/pion/webrtc/v4"
)

// Mojang's HTTP exchange is a single full-ICE offer/answer. This in-process listener refuses
// anonymous offers as BDS does, verifying the signed-out transport without Xbox service access.
func TestLocalNetherNetOfflineHTTPDialCarriesCompleteSDPAndData(t *testing.T) {
	log := slog.New(slog.DiscardHandler)
	handler := endpoint.HandlerConfig{Logger: log}.New()
	var settings webrtc.SettingEngine
	settings.SetIncludeLoopbackCandidate(true)
	settings.SetIPFilter(func(ip net.IP) bool { return ip.IsLoopback() && ip.To4() != nil })
	listener, err := (nethernet.ListenConfig{
		API: webrtc.NewAPI(webrtc.WithSettingEngine(settings)), Log: log, DisableTrickleICE: true,
	}).Listen(handler)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	var offers atomic.Int32
	var complete, identified atomic.Bool
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPost {
			offer, err := io.ReadAll(r.Body)
			if err != nil {
				http.Error(w, err.Error(), http.StatusBadRequest)
				return
			}
			offers.Add(1)
			complete.Store(strings.Contains(string(offer), "a=candidate:"))
			identified.Store(strings.Contains(string(offer), "a=identity:"))
			r.Body = io.NopCloser(bytes.NewReader(offer))
		}
		handler.ServeHTTP(w, r)
	}))
	t.Cleanup(server.Close)
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: server.Listener.Addr().String(), Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("online"))
	target, err := resolve(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithTimeout(t.Context(), 10*time.Second)
	defer cancel()
	stop := context.AfterFunc(ctx, func() { _ = listener.Close() })
	defer stop()
	client, err := target.network.DialContext(ctx, target.address)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = client.Close() })
	peer, err := listener.Accept()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = peer.Close() })
	deadline, _ := ctx.Deadline()
	_ = client.SetDeadline(deadline)
	_ = peer.SetDeadline(deadline)
	const payload = "signed-out local BDS transport"
	if _, err := client.Write([]byte(payload)); err != nil {
		t.Fatal(err)
	}
	buf := make([]byte, len(payload))
	if _, err := io.ReadFull(peer, buf); err != nil || string(buf) != payload {
		t.Fatalf("data channel payload = %q, %v", buf, err)
	}
	if offers.Load() != 1 || !complete.Load() || !identified.Load() {
		t.Fatalf("full signed-out SDP: posts=%d, candidates=%v, identified=%v", offers.Load(), complete.Load(), identified.Load())
	}
}
