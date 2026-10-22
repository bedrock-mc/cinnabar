package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"net/http/httptest"
	"sync/atomic"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"golang.org/x/oauth2"
)

// Only a launcher-owned BDS without verification keys may retry with local identity.
func TestLocalBDSKeylessLoginRetry(t *testing.T) {
	t.Parallel()
	for _, test := range []struct {
		name                                                               string
		local, signedIn, cancelled, repeated, transferred, authUnavailable bool
		reason                                                             int32
		wantDials                                                          int
	}{
		{name: "local keyless", local: true, signedIn: true, reason: packet.DisconnectReasonHostWithoutKeys, wantDials: 2},
		{name: "local invalid login", local: true, signedIn: true, reason: packet.DisconnectReasonNotAuthenticated, wantDials: 1},
		{name: "local issuer mismatch", local: true, signedIn: true, reason: packet.DisconnectReasonDiscoveryEnvironmentMismatch, wantDials: 1},
		{name: "remote keyless", signedIn: true, reason: packet.DisconnectReasonHostWithoutKeys, wantDials: 1},
		{name: "signed out", local: true, reason: packet.DisconnectReasonHostWithoutKeys, wantDials: 1},
		{name: "cancelled", local: true, signedIn: true, cancelled: true, reason: packet.DisconnectReasonHostWithoutKeys, wantDials: 1},
		{name: "account unavailable", local: true, signedIn: true, authUnavailable: true, wantDials: 2},
		{name: "transfer keyless", local: true, signedIn: true, transferred: true, reason: packet.DisconnectReasonHostWithoutKeys, wantDials: 1},
		{name: "second rejection", local: true, signedIn: true, repeated: true, reason: packet.DisconnectReasonHostWithoutKeys, wantDials: 2},
	} {
		t.Run(test.name, func(t *testing.T) {
			ctx, cancel := context.WithCancel(t.Context())
			defer cancel()
			var account *authcache.Account
			if test.signedIn {
				account = authcache.NewAccount(ctx, "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "unused"}), nil)
				defer account.Close()
			}
			connections := newPreparedConnections("unused.invalid:19132", account, slog.New(slog.DiscardHandler))
			defer connections.shutdown()
			connections.resolveTarget = withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
				return localworld.ConnectionTarget{Address: "127.0.0.1:5000", Transport: localworld.TransportNetherNetHTTP}, test.local, nil
			}, onlineStub("remote.invalid:19132"))
			rejected := &minecraft.DisconnectPacketError{Reason: test.reason}
			stopped := errors.New("retry inspected")
			calls := 0
			connections.dialTarget = func(_ context.Context, _ *resolvedUpstreamTarget, dialer minecraft.Dialer) (upstreamSession, error) {
				calls++
				if calls == 1 {
					if (dialer.TokenSource != nil) != test.signedIn {
						t.Error("initial login changed account authentication")
					}
					if test.cancelled {
						cancel()
					}
					var failure error = fmt.Errorf("server rejected login: %w", rejected)
					if test.authUnavailable {
						failure = errors.New("account service offline")
					}
					if test.local && !test.transferred {
						failure = &localBDSLoginError{err: failure, beforeTransport: test.authUnavailable}
					}
					return nil, failure
				}
				if calls > test.wantDials {
					t.Fatal("unexpected login retry")
				}
				if dialer.TokenSource != nil || dialer.XBLClient != nil || dialer.PlayFabClient != nil {
					t.Error("local retry requires online credentials")
				}
				if dialer.IdentityData.Identity != "local-player" || dialer.IdentityData.DisplayName != "Local Player" {
					t.Errorf("local retry identity = %+v", dialer.IdentityData)
				}
				if dialer.IdentityData.XUID != "" || dialer.IdentityData.TitleID != "" {
					t.Error("offline retry retained authenticated claims")
				}
				if test.repeated {
					return nil, rejected
				}
				return nil, stopped
			}
			_, err := connections.connect(ctx, dialerTestDownstream{
				protocol: minecraft.DefaultProtocol,
				identity: login.IdentityData{Identity: "local-player", DisplayName: "Local Player", XUID: "not-an-offline-claim"},
			})
			wantErr := error(rejected)
			if test.wantDials == 2 && !test.repeated {
				wantErr = stopped
			}
			if calls != test.wantDials || !errors.Is(err, wantErr) {
				t.Fatalf("connect = %v; dials = %d, want %d and %v", err, calls, test.wantDials, wantErr)
			}
		})
	}
}

type unavailableLocalAccountService struct{}

// RoundTrip models a disconnected account service without contacting it.
func (unavailableLocalAccountService) RoundTrip(*http.Request) (*http.Response, error) {
	return nil, errors.New("account service offline")
}

func TestLocalBDSCanAttemptOfflineLoginWhenAccountServicesAreUnavailable(t *testing.T) {
	var offers atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.Method == http.MethodPost {
			offers.Add(1)
		}
		http.Error(w, "stop after the local offer", http.StatusBadRequest)
	}))
	defer server.Close()
	account := authcache.NewAccount(t.Context(), "", oauth2.StaticTokenSource(&oauth2.Token{AccessToken: "unused"}), nil)
	defer account.Close()
	connections := newPreparedConnections("unused.invalid:19132", account, slog.New(slog.DiscardHandler))
	defer connections.shutdown()
	connections.httpClient = &http.Client{Transport: unavailableLocalAccountService{}}
	connections.resolveTarget = withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: server.Listener.Addr().String(), Transport: localworld.TransportNetherNetHTTP}, true, nil
	}, onlineStub("unused.invalid:19132"))
	_, err := connections.connect(t.Context(), dialerTestDownstream{
		protocol: minecraft.DefaultProtocol,
		identity: login.IdentityData{DisplayName: "Local Player", Identity: "local-player"},
	})
	if err == nil || offers.Load() != 1 {
		t.Fatalf("local offers = %d, error = %v; want one offline offer after the account service fails", offers.Load(), err)
	}
}
