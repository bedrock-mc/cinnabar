package authcache

import (
	"context"
	"encoding/json"
	"path/filepath"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2"
	"github.com/google/uuid"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

// A foreground resume preserves an early replacement; a genuinely new credential supersedes it.
func TestEarlyRefreshFencesForegroundCredentialChanges(t *testing.T) {
	for _, replace := range []bool{false, true} {
		name := "restored claims"
		if replace {
			name = "replacement credential"
		}
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(derivedTestDir(t), "derived")
			oauthToken := testOAuthToken("account-a")
			expiry := time.Now().Add(serviceRefreshLead / 2)
			writeDerivedState(t, path, oauthToken, expiry)
			state, err := loadDerived(path)
			if err != nil {
				t.Fatal(err)
			}
			state.ServiceToken.AuthorizationHeader = skewedServiceJWT(t, time.Now().Add(-time.Minute), expiry)
			before := state.ServiceToken.AuthorizationHeader
			b, err := json.Marshal(state)
			if err != nil {
				t.Fatal(err)
			}
			if err := savePrivate(path, append(b, '\n')); err != nil {
				t.Fatal(err)
			}
			late := &service.Token{AuthorizationHeader: "MCToken background", ValidUntil: time.Now().Add(time.Hour)}
			fresh := &service.Token{AuthorizationHeader: "MCToken foreground", ValidUntil: time.Now().Add(30 * time.Minute)}
			var account *Account
			var foreground *service.Token
			deps := derivedDeps{
				discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
				services: func(env *service.AuthorizationEnvironment, tickets service.SessionTicketSource, seed *service.Token, _, _ string) service.TokenSource {
					if seed != nil {
						if replace {
							return fakeServiceSourceFunc(func(context.Context) (*service.Token, error) { return fresh, nil })
						}
						return env.ResumeTokenSource(tickets, service.TokenConfig{}, seed)
					}
					return fakeServiceSourceFunc(func(ctx context.Context) (*service.Token, error) {
						// Force the foreground call between the background snapshot and its response.
						var err error
						foreground, err = account.ServiceToken(ctx)
						return late, err
					})
				},
			}
			account = newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
			defer account.Close()
			if _, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead); err != nil {
				t.Fatal(err)
			}
			want := late.AuthorizationHeader
			if replace {
				want = fresh.AuthorizationHeader
				if foreground != fresh {
					t.Fatal("the foreground exchange did not supply its new credential")
				}
			} else if foreground == nil || foreground.AuthorizationHeader != before || foreground.Claims.PlayerMessagingID == uuid.Nil {
				t.Fatal("the foreground call did not restore the cached credential's claims")
			}
			if token, err := account.ServiceToken(context.Background()); err != nil || token.AuthorizationHeader != want {
				t.Fatalf("the usable credential after foreground access was not preserved: err=%v", err)
			}
			settle(t, account)
			state, err = loadDerived(path)
			if err != nil || state.ServiceToken == nil || state.ServiceToken.AuthorizationHeader != want {
				t.Fatalf("the usable credential was not persisted: err=%v", err)
			}
		})
	}
}

// An eviction or refusal cannot be undone by a delayed early exchange, including discovery's gap.
func TestEarlyRefreshDoesNotRestoreRejectedOrSupersededTokens(t *testing.T) {
	for _, scenario := range []struct {
		name        string
		discovery   bool
		rejectedOld bool
		returnOld   bool
	}{
		{name: "rejected result", rejectedOld: true, returnOld: true},
		{name: "superseded exchange", rejectedOld: true},
		{name: "superseded during discovery", discovery: true, rejectedOld: true},
		{name: "refused different header"},
	} {
		t.Run(scenario.name, func(t *testing.T) {
			path := filepath.Join(derivedTestDir(t), "derived")
			oauthToken := testOAuthToken("account-a")
			writeDerivedState(t, path, oauthToken, time.Now().Add(serviceRefreshLead/2))
			old := testServiceToken(time.Now().Add(serviceRefreshLead / 2))
			late := &service.Token{AuthorizationHeader: "MCToken delayed", ValidUntil: time.Now().Add(time.Hour)}
			if scenario.returnOld {
				late.AuthorizationHeader = old.AuthorizationHeader
			}
			var account *Account
			invalidate := func() {
				rejected := late
				if scenario.rejectedOld {
					rejected = old
				}
				account.InvalidateServiceToken(rejected)
			}
			exchanges := 0
			deps := derivedDeps{
				discover: func(context.Context) (*service.AuthorizationEnvironment, error) {
					if scenario.discovery {
						invalidate()
					}
					return testEnvironment(), nil
				},
				services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
					exchanges++
					if exchanges == 1 {
						if !scenario.discovery {
							invalidate()
						}
						return late, nil
					}
					return &service.Token{AuthorizationHeader: "MCToken current", ValidUntil: time.Now().Add(time.Hour)}, nil
				}),
			}
			account = newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
			defer account.Close()
			_, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead)
			if scenario.rejectedOld && err == nil {
				t.Fatal("an evicted service generation reported a successful refresh")
			}
			if !scenario.rejectedOld && err != nil {
				t.Fatal(err)
			}
			settle(t, account)
			state, err := loadDerived(path)
			if err != nil {
				t.Fatal(err)
			}
			if state.ServiceToken != nil && state.ServiceToken.AuthorizationHeader == late.AuthorizationHeader {
				t.Fatal("the discarded refresh was persisted")
			}
			if scenario.rejectedOld && state.ServiceToken != nil {
				t.Fatal("the evicted service token was restored")
			}
			want := "MCToken current"
			if !scenario.rejectedOld {
				want = old.AuthorizationHeader
			}
			token, err := account.ServiceToken(context.Background())
			if err != nil || token == nil || token.AuthorizationHeader != want {
				t.Fatalf("subsequent cache read did not preserve the usable token: %v", err)
			}
			settle(t, account)
			state, err = loadDerived(path)
			if err != nil || state.ServiceToken == nil || state.ServiceToken.AuthorizationHeader != want {
				t.Fatalf("current token was not persisted: %v", err)
			}
		})
	}
}
