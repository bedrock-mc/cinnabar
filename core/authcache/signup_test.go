package authcache

import (
	"context"
	"errors"
	"fmt"
	"net/url"
	"path/filepath"
	"testing"
	"time"

	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

func TestSignInPreservesXboxSignupError(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "oauth.json")
	token := testOAuthToken("account-a")
	writeDerivedState(t, DerivedCachePath(path), token, time.Now().Add(-time.Minute))
	signup := &sisu.AccountCreationRequiredError{SignupURL: &url.URL{Scheme: "https", Host: "sisu.xboxlive.com", Path: "/signup", RawQuery: "signature=private"}}
	deps := defaultDerivedDeps()
	deps.discover = func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil }
	deps.login = func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*playfab.Client, error) {
		return nil, fmt.Errorf("exchange: %w", signup)
	}
	err := completeSignIn(context.Background(), path, oauth2.StaticTokenSource(token), nil, deps)
	var got *sisu.AccountCreationRequiredError
	if !errors.As(err, &got) || got != signup {
		t.Fatalf("completion lost the Xbox signup error: %v", err)
	}
}
