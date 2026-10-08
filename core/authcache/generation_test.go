package authcache

import (
	"context"
	"crypto/ecdsa"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

// signInForGenerationTest creates an OAuth source without contacting Microsoft.
func signInForGenerationTest(t *testing.T, path, identity string) oauth2.TokenSource {
	t.Helper()
	source, err := Source(context.Background(), Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
		return testOAuthToken(identity), nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	return source
}

// generationAccount opens a real PlayFab client using only local identity responses.
func generationAccount(t *testing.T, source oauth2.TokenSource) (*Account, *playfab.Client) {
	t.Helper()
	var logins atomic.Int32
	account := newAccount(context.Background(), "", source, nil, derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		login: func(ctx context.Context, env *service.AuthorizationEnvironment, _ xsapi.TokenAndSignaturer) (*playfab.Client, error) {
			return playfab.Login(ctx, env.PlayFabTitleID, fakeIdentityProvider{&logins}, playfab.ClientConfig{
				HTTPClient: &http.Client{Transport: refusingTransport{}}, Logger: slog.New(slog.DiscardHandler),
			})
		},
	})
	t.Cleanup(func() { _ = account.Close() })
	account.session = newAccountSession(account, &sisu.SessionConfig{DeviceTokenSource: generationDeviceSource{}})
	client, err := account.PlayFab(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	return account, client
}

// generationDeviceSource lets SISU reach OAuth without network requests.
type generationDeviceSource struct{}

// DeviceToken supplies the valid device credential required before SISU reads OAuth.
func (generationDeviceSource) DeviceToken(context.Context) (*xasd.Token, error) {
	return &xasd.Token{Token: "device", NotAfter: time.Now().Add(time.Hour)}, nil
}

// ProofKey prevents the test from making HTTP requests if OAuth is incorrectly accepted.
func (generationDeviceSource) ProofKey() *ecdsa.PrivateKey { return nil }

func TestLegacyCacheMigrationPreservesGenerationAcrossExternalRefresh(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oauth.json")
	writeToken(t, path, testOAuthToken("legacy"))
	first := signInForGenerationTest(t, path, "unexpected")
	initial, err := load(path)
	if err != nil || initial.Generation == "" {
		t.Fatalf("legacy generation was not migrated: %v", err)
	}
	account, playfabClient := generationAccount(t, first)
	rotated := testOAuthToken("rotated")
	_, err = Source(context.Background(), Config{Path: path, Refresh: func(*oauth2.Token, io.Writer) oauth2.TokenSource {
		return oauth2.StaticTokenSource(rotated)
	}})
	if err != nil {
		t.Fatal(err)
	}
	current, err := load(path)
	if err != nil || current.Generation != initial.Generation {
		t.Fatalf("external refresh changed the sign-in generation: %v", err)
	}
	got, err := account.Token()
	if err != nil || !sameToken(got, rotated) {
		t.Fatalf("account did not adopt its own generation's rotation: %v", err)
	}
	client, err := account.PlayFab(context.Background())
	if err != nil || client != playfabClient || account.Closed() {
		t.Fatalf("normal rotation discarded the shared account: %v", err)
	}
	select {
	case <-playfabClient.TitlePlayerAccount().Context().Done():
		t.Fatal("normal rotation closed PlayFab")
	default:
	}
}

func TestNewSignInNeverReachesPreviousAccount(t *testing.T) {
	for _, retained := range []bool{false, true} {
		name := "account"
		if retained {
			name = "retained SISU source"
		}
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "oauth.json")
			previous := signInForGenerationTest(t, path, "previous")
			account, client := generationAccount(t, previous)
			initial, err := load(path)
			if err != nil {
				t.Fatal(err)
			}
			if err := Remove(context.Background(), path, os.Remove); err != nil {
				t.Fatal(err)
			}
			next := signInForGenerationTest(t, path, "next")
			replacement, err := load(path)
			if err != nil || replacement.Generation == initial.Generation {
				t.Fatalf("new sign-in reused the previous generation: %v", err)
			}
			if retained {
				if got, err := account.session.XSTSToken(context.Background(), cachedRelyingParty); err == nil || got != nil {
					t.Fatal("retained SISU source accepted the replacement sign-in")
				}
			} else if got, err := account.Token(); !errors.Is(err, errAccountChanged) || got != nil {
				t.Fatalf("old account token = %v, want replacement rejected", err)
			}
			if !account.Closed() {
				t.Fatal("replacement did not cancel the previous account")
			}
			select {
			case <-client.TitlePlayerAccount().Context().Done():
			case <-time.After(time.Second):
				t.Fatal("replacement left the previous PlayFab background session running")
			}
			if got, err := previous.Token(); !errors.Is(err, errAccountChanged) || got != nil {
				t.Fatalf("old OAuth source accepted the new account: %v", err)
			}
			if _, err := account.PlayFab(context.Background()); !errors.Is(err, ErrAccountClosed) {
				t.Fatalf("closed account reused PlayFab: %v", err)
			}
			got, err := next.Token()
			if err != nil || !sameToken(got, &replacement.Token) {
				t.Fatalf("old runtime damaged the replacement source: %v", err)
			}
		})
	}
}

func TestLiveSourceRejectsMissingGeneration(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oauth.json")
	source := signInForGenerationTest(t, path, "previous")
	// An older process cannot prove a replacement token belongs to this sign-in.
	writeToken(t, path, testOAuthToken("legacy replacement"))
	if got, err := source.Token(); !errors.Is(err, errAccountChanged) || got != nil {
		t.Fatalf("unbound replacement was accepted: %v", err)
	}
}
