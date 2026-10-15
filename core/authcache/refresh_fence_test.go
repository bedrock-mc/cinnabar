package authcache

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

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
