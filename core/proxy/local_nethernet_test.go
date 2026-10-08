package proxy

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"encoding/json"
	"errors"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/endpoint"
	"github.com/go-jose/go-jose/v4"
	"github.com/go-jose/go-jose/v4/jwt"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

func TestLocalBDSUsesHTTPStatusWithoutOnlineResolution(t *testing.T) {
	t.Parallel()
	want := endpoint.Status{ServerName: "Local fixture", Protocol: protocol.CurrentProtocol, Version: protocol.CurrentVersion, MaxPlayerCount: 8}
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method != http.MethodGet || r.URL.Path != "/v1/join" {
			http.Error(w, "unexpected endpoint", http.StatusNotFound)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		_ = json.NewEncoder(w).Encode(want)
	}))
	t.Cleanup(server.Close)
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: server.Listener.Addr().String(), Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, func(context.Context) (*resolvedUpstreamTarget, error) {
		t.Fatal("local BDS must not resolve Xbox or online catalog targets")
		return nil, errors.New("unexpected online resolution")
	})
	target, err := resolve(t.Context())
	if err != nil || target.address != server.URL {
		t.Fatalf("target = %+v, %v", target, err)
	}
	pong, err := target.network.PingContext(t.Context(), target.address)
	if err != nil {
		t.Fatal(err)
	}
	got, err := endpoint.RakNetPongData(pong)
	if err != nil || got != want {
		t.Fatalf("HTTP status = %+v, %v, want %+v", got, err, want)
	}
	ctx, cancel := context.WithCancel(t.Context())
	cancel()
	if _, err := target.network.PingContext(ctx, target.address); !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled status request = %v", err)
	}
}

func TestLocalNetherNetRejectsUnspecifiedTransportAndNonLoopbackURLs(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name      string
		address   string
		transport localworld.Transport
	}{
		{name: "missing transport", address: "127.0.0.1:5000"},
		{name: "remote host", address: "192.0.2.10:5000", transport: localworld.TransportNetherNetHTTP},
		{name: "zero port", address: "127.0.0.1:0", transport: localworld.TransportNetherNetHTTP},
		{name: "overflow port", address: "127.0.0.1:65536", transport: localworld.TransportNetherNetHTTP},
		{name: "url injection", address: "127.0.0.1:5000/path", transport: localworld.TransportNetherNetHTTP},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Parallel()
			resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
				return localworld.ConnectionTarget{Address: test.address, Transport: test.transport}, true, nil
			}, onlineStub("online"))
			if target, err := resolve(t.Context()); err == nil || target != nil {
				t.Fatalf("invalid local target accepted: %+v, %v", target, err)
			}
		})
	}
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: "[::1]:5000", Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("online"))
	if target, err := resolve(t.Context()); err != nil || !strings.HasPrefix(target.address, "http://[::1]:") {
		t.Fatalf("IPv6 loopback target = %+v, %v", target, err)
	}
}

func TestLocalBDSDoesNotFollowStatusRedirects(t *testing.T) {
	t.Parallel()
	destination := httptest.NewServer(http.HandlerFunc(func(http.ResponseWriter, *http.Request) {
		t.Error("local BDS status must not cross an HTTP redirect")
	}))
	t.Cleanup(destination.Close)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, destination.URL, http.StatusTemporaryRedirect)
	}))
	t.Cleanup(server.Close)
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: server.Listener.Addr().String(), Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("online"))
	target, err := resolve(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	if _, err := target.network.PingContext(t.Context(), target.address); err == nil {
		t.Fatal("local BDS status redirect was accepted")
	}
}

// A signed-out dial must still be admitted by a listener that, like BDS, refuses anonymous offers.
func TestLocalNetherNetSignedOutDialIsAdmittedByIdentityRequiringListener(t *testing.T) {
	t.Parallel()
	log := slog.New(slog.DiscardHandler)
	handler := endpoint.HandlerConfig{Logger: log}.New()
	listener, err := nethernet.ListenConfig{Log: log, DisableTrickleICE: true}.Listen(handler)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	answers := make(chan string, 1)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		var body bytes.Buffer
		handler.ServeHTTP(teeResponse{ResponseWriter: w, body: &body}, r)
		if r.Method == http.MethodPost {
			answers <- body.String()
		}
	}))
	t.Cleanup(server.Close)
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: server.Listener.Addr().String(), Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("online"))
	target, err := resolve(t.Context())
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(t.Context())
	dialed := make(chan error, 1)
	go func() {
		conn, err := target.network.DialContext(ctx, target.address)
		if err == nil {
			_ = conn.Close()
		}
		dialed <- err
	}()
	var answer string
	select {
	case answer = <-answers:
	case err := <-dialed:
		t.Fatalf("dial ended before its offer was answered: %v", err)
	}
	cancel()
	<-dialed
	if _, err := strconv.ParseUint(answer, 10, 32); err == nil || !strings.HasPrefix(answer, "v=0") {
		t.Fatalf("signed-out offer refused: answer %q, want an SDP answer", answer)
	}
}

// BDS refuses an identity whose cpk is a JWK, so it must stay base64 DER.
func TestLocalNetherNetIdentityCarriesDERPublicKey(t *testing.T) {
	t.Parallel()
	identity, err := selfSignedIdentity(time.Now())
	if err != nil {
		t.Fatal(err)
	}
	token, err := jwt.ParseSigned(identity.Token, []jose.SignatureAlgorithm{jose.ES384})
	if err != nil {
		t.Fatal(err)
	}
	var claims struct {
		PublicKey string `json:"cpk"`
	}
	if err := token.Claims(&identity.PrivateKey.PublicKey, &claims); err != nil {
		t.Fatal(err)
	}
	var key ecdsa.PublicKey
	if err := login.ParsePublicKey(claims.PublicKey, &key); err != nil || !key.Equal(&identity.PrivateKey.PublicKey) {
		t.Fatalf("cpk %q does not carry the identity key: %v", claims.PublicKey, err)
	}
}

type teeResponse struct {
	http.ResponseWriter
	body *bytes.Buffer
}

func (w teeResponse) Write(b []byte) (int, error) {
	w.body.Write(b)
	return w.ResponseWriter.Write(b)
}
