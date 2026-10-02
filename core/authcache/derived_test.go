package authcache

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/x509"
	"encoding/base64"
	"encoding/json"
	"errors"
	"io"
	"log/slog"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-playfab/v2/entity"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/xal/nsal"
	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/df-mc/go-xsapi/v2/xal/xasu"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

const cachedRelyingParty = "http://xboxlive.com"

// derivedTestDir returns the account cache directory for one test.
func derivedTestDir(t *testing.T) string {
	t.Helper()
	return t.TempDir()
}

func TestPersistentSourceFreshInstanceReusesDerivedStateAndMintsPerKey(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var discoveryCalls, serviceCalls, mintCalls int
	var minted []string
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) {
			discoveryCalls++
			return testEnvironment(), nil
		},
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
		mint: func(_ context.Context, _ *service.AuthorizationEnvironment, src service.TokenSource, key *ecdsa.PublicKey) (string, error) {
			mintCalls++
			if _, err := src.ServiceToken(context.Background()); err != nil {
				return "", err
			}
			minted = append(minted, key.X.Text(16))
			return minted[len(minted)-1], nil
		},
	}

	for range 2 {
		var diagnostics bytes.Buffer
		source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), &diagnostics, deps)
		xbox := source.(xsapi.TokenSource)
		if _, err := xbox.XSTSToken(context.Background(), cachedRelyingParty); err != nil {
			t.Fatalf("XSTSToken: %v", err)
		}
		key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := source.(minecraft.MultiplayerTokenSource).MultiplayerToken(context.Background(), &key.PublicKey); err != nil {
			t.Fatalf("MultiplayerToken: %v", err)
		}
		if !strings.Contains(diagnostics.String(), "event=hit") {
			t.Fatalf("diagnostics = %q, want secret-safe hit", diagnostics.String())
		}
	}
	if discoveryCalls != 2 || serviceCalls != 0 {
		t.Fatalf("calls = (discovery=%d service=%d), want one environment check per process and zero cached auth exchanges", discoveryCalls, serviceCalls)
	}
	if mintCalls != 2 {
		t.Fatalf("multiplayer mint calls = %d, want one per ephemeral key", mintCalls)
	}
	if minted[0] == minted[1] {
		t.Fatal("multiplayer credentials reused across distinct ephemeral keys")
	}
}

func TestPersistentSourceExpiredServiceRefreshesOnlyServiceLayer(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	var discoveryCalls, serviceCalls int
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) {
			discoveryCalls++
			return testEnvironment(), nil
		},
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
		mint: func(_ context.Context, _ *service.AuthorizationEnvironment, src service.TokenSource, _ *ecdsa.PublicKey) (string, error) {
			_, err := src.ServiceToken(context.Background())
			return "fresh-jwt", err
		},
	}
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if _, err := source.(minecraft.MultiplayerTokenSource).MultiplayerToken(context.Background(), &key.PublicKey); err != nil {
		t.Fatal(err)
	}
	if discoveryCalls != 1 || serviceCalls != 1 {
		t.Fatalf("calls = (discovery=%d service=%d), want (1,1)", discoveryCalls, serviceCalls)
	}
	second := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	secondKey, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if _, err := second.(minecraft.MultiplayerTokenSource).MultiplayerToken(context.Background(), &secondKey.PublicKey); err != nil {
		t.Fatal(err)
	}
	if serviceCalls != 1 {
		t.Fatalf("service refresh calls after fresh source = %d, want persisted token reuse", serviceCalls)
	}
}

func TestPersistentSourceOAuthRotationInvalidatesInMemoryDerivedState(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oldToken := testOAuthToken("account-a")
	newToken := testOAuthToken("account-b")
	writeDerivedState(t, path, oldToken, time.Now().Add(time.Hour))
	oauth := &sequenceOAuthSource{tokens: []*oauth2.Token{oldToken, newToken}}
	source := persistentSource(context.Background(), path, oauth, nil, derivedDeps{})
	if _, err := source.Token(); err != nil {
		t.Fatal(err)
	}
	persistent := source.(*Account)
	if persistent.binding != oauthBinding(newToken) {
		t.Fatal("rotated OAuth material did not replace the in-memory binding")
	}
	if persistent.deviceToken != nil || persistent.service != nil || persistent.cachedEnv != nil {
		t.Fatal("old-account derived credentials survived OAuth rotation in memory")
	}
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	if state.OAuthBinding != oauthBinding(oldToken) {
		t.Fatal("unleased OAuth reset modified shared derived state")
	}
}

func TestPersistentSourceOAuthRotationDoesNotPublishWithoutLease(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oldToken := testOAuthToken("account-a")
	newToken := testOAuthToken("account-b")
	writeDerivedState(t, path, oldToken, time.Now().Add(time.Hour))
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	source := persistentSource(context.Background(), path, &sequenceOAuthSource{tokens: []*oauth2.Token{oldToken, newToken}}, nil, derivedDeps{})
	if _, err := source.Token(); err != nil {
		t.Fatal(err)
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(before, after) {
		t.Fatal("OAuth reset published derived state without a lease")
	}
}

func TestPersistentSourceProofKeyStableAcrossOAuthReset(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oldToken := testOAuthToken("account-a")
	newToken := testOAuthToken("account-b")
	writeDerivedState(t, path, oldToken, time.Now().Add(time.Hour))
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	want := decodeProofKey(t, state.ProofKey)
	source := persistentSource(context.Background(), path, &sequenceOAuthSource{tokens: []*oauth2.Token{oldToken, oldToken, newToken}}, nil, derivedDeps{})
	if _, err := source.(xsapi.TokenSource).DeviceToken(context.Background()); err != nil {
		t.Fatal(err)
	}
	if _, err := source.Token(); err != nil {
		t.Fatal(err)
	}
	if got := source.(xsapi.TokenSource).ProofKey(); got.D.Cmp(want.D) != 0 {
		t.Fatal("OAuth reset changed the proof key after a device token was exposed")
	}
}

func TestPersistentSourceProofKeyStableAcrossConflictingReload(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	want := decodeProofKey(t, state.ProofKey)
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	if _, err := source.(xsapi.TokenSource).DeviceToken(context.Background()); err != nil {
		t.Fatal(err)
	}
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	if _, err := source.(xsapi.TokenSource).XSTSToken(context.Background(), cachedRelyingParty); err != nil {
		t.Fatal(err)
	}
	if got := source.(xsapi.TokenSource).ProofKey(); got.D.Cmp(want.D) != 0 {
		t.Fatal("cache reload changed the proof key after a device token was exposed")
	}
}

func TestPersistentSourceBindingsAndUnsafeInputsAreConservativeMisses(t *testing.T) {
	tests := map[string]func(*testing.T, string, *oauth2.Token){
		"account": func(t *testing.T, path string, token *oauth2.Token) {
			writeDerivedState(t, path, testOAuthToken("other-account"), time.Now().Add(time.Hour))
		},
		"corrupt": func(t *testing.T, path string, _ *oauth2.Token) {
			if err := os.WriteFile(path, []byte("{not-json"), 0o600); err != nil {
				t.Fatal(err)
			}
		},
		"client_config": func(t *testing.T, path string, token *oauth2.Token) {
			writeDerivedState(t, path, token, time.Now().Add(time.Hour))
			state, err := loadDerived(path)
			if err != nil {
				t.Fatal(err)
			}
			state.ClientBinding = "different-client-config"
			b, err := json.Marshal(state)
			if err != nil {
				t.Fatal(err)
			}
			if err := savePrivate(path, append(b, '\n')); err != nil {
				t.Fatal(err)
			}
		},
		"oversized": func(t *testing.T, path string, _ *oauth2.Token) {
			if err := os.WriteFile(path, bytes.Repeat([]byte("x"), maxCacheSize+1), 0o600); err != nil {
				t.Fatal(err)
			}
		},
		"symlink": func(t *testing.T, path string, token *oauth2.Token) {
			target := path + ".target"
			writeDerivedState(t, target, token, time.Now().Add(time.Hour))
			if err := os.Symlink(target, path); err != nil {
				t.Skipf("symlink unavailable: %v", err)
			}
		},
	}
	for name, arrange := range tests {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(derivedTestDir(t), "derived")
			token := testOAuthToken("account-a")
			arrange(t, path, token)
			var diagnostics bytes.Buffer
			source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(token), &diagnostics, derivedDeps{})
			persistent := source.(*Account)
			if persistent.environment != nil || persistent.service != nil {
				t.Fatal("unsafe or mismatched state was reused")
			}
			if !strings.Contains(diagnostics.String(), "event=miss") {
				t.Fatalf("diagnostics = %q, want miss", diagnostics.String())
			}
			for _, secret := range []string{token.AccessToken, token.RefreshToken, path} {
				if strings.Contains(diagnostics.String(), secret) {
					t.Fatalf("diagnostics leaked secret/path: %q", diagnostics.String())
				}
			}
		})
	}
}

func TestPersistentSourceMalformedEnvironmentCannotPartiallyRestoreSISU(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	cachedDER, err := base64.RawStdEncoding.DecodeString(state.ProofKey)
	if err != nil {
		t.Fatal(err)
	}
	cachedKey, err := x509.ParseECPrivateKey(cachedDER)
	if err != nil {
		t.Fatal(err)
	}
	state.Environment.ServiceURI = "http://unsafe.example.test"
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	restored := source.(*Account).ProofKey()
	if restored.D.Cmp(cachedKey.D) == 0 {
		t.Fatal("valid SISU/proof key was partially restored from a bundle with an invalid environment")
	}
}

func TestPersistentSourceMultiplayerCallChecksOAuthBindingBeforeReuse(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oldToken := testOAuthToken("account-a")
	newToken := testOAuthToken("account-b")
	writeDerivedState(t, path, oldToken, time.Now().Add(time.Hour))
	var serviceCalls int
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
		mint: mintFromService,
	}
	source := persistentSource(context.Background(), path, &sequenceOAuthSource{tokens: []*oauth2.Token{oldToken, newToken}}, nil, deps)
	key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if _, err := source.(minecraft.MultiplayerTokenSource).MultiplayerToken(context.Background(), &key.PublicKey); err != nil {
		t.Fatal(err)
	}
	if serviceCalls != 1 {
		t.Fatalf("service refresh calls = %d, want 1 after OAuth binding changed without Token call", serviceCalls)
	}
}

// A service token a service rejected is evicted on disk too, so the next call issues a new one.
func TestPersistentSourceInvalidatedServiceTokenIsReplaced(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var serviceCalls int
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return &service.Token{AuthorizationHeader: "MCToken replacement", ValidUntil: time.Now().Add(time.Hour)}, nil
		}),
	}
	account := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps).(*Account)
	rejected, err := account.ServiceToken(context.Background())
	if err != nil || serviceCalls != 0 {
		t.Fatalf("restored token not reused: calls=%d err=%v", serviceCalls, err)
	}
	var invalidator service.TokenInvalidator = account
	invalidator.InvalidateServiceToken(rejected)
	if state, err := loadDerived(path); err != nil || state.ServiceToken != nil {
		t.Fatalf("persisted bundle kept the rejected token: err=%v", err)
	}
	token, err := account.ServiceToken(context.Background())
	if err != nil || serviceCalls != 1 || token.AuthorizationHeader != "MCToken replacement" {
		t.Fatalf("token=%v calls=%d err=%v", token, serviceCalls, err)
	}
}

func TestPersistentSourceServiceEnvironmentMismatchDoesNotReuse(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var serviceCalls int
	different := testEnvironment()
	different.ServiceURI, _ = url.Parse("https://replacement.example.test")
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return different, nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
		mint: mintFromService,
	}
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if _, err := source.(minecraft.MultiplayerTokenSource).MultiplayerToken(context.Background(), &key.PublicKey); err != nil {
		t.Fatal(err)
	}
	if serviceCalls != 1 {
		t.Fatalf("service refresh calls = %d, want 1 after environment mismatch", serviceCalls)
	}
}

func TestPersistentSourceCancellationDoesNotRetryOrLeak(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	var calls int
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) {
			calls++
			return testEnvironment(), nil
		},
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			calls++
			return nil, context.Canceled
		}),
		mint: func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error) {
			calls++
			return "", context.Canceled
		},
	}
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	_, err := source.(minecraft.MultiplayerTokenSource).MultiplayerToken(ctx, &key.PublicKey)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want cancellation", err)
	}
	if calls != 0 {
		t.Fatalf("network calls after cancellation = %d, want 0", calls)
	}
}

func TestPersistentSourceConcurrentFreshInstancesRemainUsable(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
		mint: mintFromService,
	}
	var wg sync.WaitGroup
	errs := make(chan error, 4)
	sources := make([]oauth2.TokenSource, 4)
	for index := range sources {
		sources[index] = persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	}
	for _, source := range sources {
		wg.Add(1)
		go func(source oauth2.TokenSource) {
			defer wg.Done()
			_, err := source.(xsapi.TokenSource).XSTSToken(context.Background(), cachedRelyingParty)
			errs <- err
		}(source)
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatalf("concurrent source failed: %v", err)
		}
	}
	if _, err := loadDerived(path); err != nil {
		t.Fatalf("cache corrupted after concurrent publication: %v", err)
	}
}

func TestPersistentSourceConcurrentExpiredRefreshUsesOneExchange(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	var serviceCalls atomic.Int32
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls.Add(1)
			time.Sleep(100 * time.Millisecond)
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
		mint: mintFromService,
	}
	var diagnostics [2]bytes.Buffer
	sources := []oauth2.TokenSource{
		persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), &diagnostics[0], deps),
		persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), &diagnostics[1], deps),
	}
	var wg sync.WaitGroup
	errs := make(chan error, len(sources))
	for _, source := range sources {
		wg.Add(1)
		go func(source oauth2.TokenSource) {
			defer wg.Done()
			key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
			_, err := source.(minecraft.MultiplayerTokenSource).MultiplayerToken(context.Background(), &key.PublicKey)
			errs <- err
		}(source)
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatal(err)
		}
	}
	if calls := serviceCalls.Load(); calls != 1 {
		t.Fatalf("concurrent service exchanges = %d, want 1; diagnostics = %q / %q", calls, diagnostics[0].String(), diagnostics[1].String())
	}
}

func TestPersistentSourceWarmReuseDoesNotRewriteBundle(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	before, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	if _, err := source.(xsapi.TokenSource).XSTSToken(context.Background(), cachedRelyingParty); err != nil {
		t.Fatal(err)
	}
	after, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if !os.SameFile(before, after) {
		t.Fatal("warm reuse atomically replaced an unchanged derived bundle")
	}
}

func TestPersistentSourceCancellationInterruptsLeaseWait(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	if err := savePrivate(path+".lock", []byte("join-auth-lease\n")); err != nil {
		t.Fatal(err)
	}
	lease, err := lockfile.Acquire(path+".lock", 0)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()

	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	ctx, cancel := context.WithCancel(context.Background())
	time.AfterFunc(50*time.Millisecond, cancel)
	started := time.Now()
	_, err = source.(xsapi.TokenSource).XSTSToken(ctx, cachedRelyingParty)
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("error = %v, want cancellation", err)
	}
	if elapsed := time.Since(started); elapsed > time.Second {
		t.Fatalf("lease cancellation took %v, want under 1s", elapsed)
	}
}

func TestPersistentSourceLeaseTimeoutCannotOverwriteOwnerState(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	lease, err := lockfile.Acquire(path+".lock", 0)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	refreshing := make(chan struct{})
	finishRefresh := make(chan struct{})
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			close(refreshing)
			<-finishRefresh
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
		mint: mintFromService,
	}
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	done := make(chan error, 1)
	go func() {
		key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
		_, err := source.(minecraft.MultiplayerTokenSource).MultiplayerToken(context.Background(), &key.PublicKey)
		done <- err
	}()
	<-refreshing
	writeDerivedState(t, path, oauthToken, time.Now().Add(2*time.Hour))
	ownerState, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	close(finishRefresh)
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(ownerState, after) {
		t.Fatal("memory-only lease fallback overwrote the lease owner's state")
	}
}

func TestSavePrivateFailurePreservesOldCache(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	if err := savePrivate(path, []byte("old\n")); err != nil {
		t.Fatal(err)
	}
	err := savePrivate(path, bytes.Repeat([]byte("x"), maxCacheSize+1))
	if err == nil {
		t.Fatal("save succeeded, want oversized cache rejection")
	}
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != "old\n" {
		t.Fatalf("cache = %q, want old cache preserved", got)
	}
}

func writeDerivedState(t *testing.T, path string, oauthToken *oauth2.Token, serviceExpiry time.Time) {
	t.Helper()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	der, err := x509.MarshalECPrivateKey(key)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now()
	state := derivedState{
		Version:       derivedCacheVersion,
		OAuthBinding:  oauthBinding(oauthToken),
		ClientBinding: clientBinding(),
		Environment:   snapshotEnvironment(testEnvironment()),
		DeviceToken: &xasd.Token{
			IssueInstant: now.Add(-time.Minute), NotAfter: now.Add(time.Hour), Token: "device-token",
			DisplayClaims: xasd.DisplayClaims{DeviceInfo: xasd.DeviceInfo{DeviceID: "device-id"}},
		},
		ProofKey: base64.RawStdEncoding.EncodeToString(der),
		SISU: &sisu.Snapshot{XSTSTokens: map[string]*xsts.Token{
			cachedRelyingParty: {
				IssueInstant: now.Add(-time.Minute), NotAfter: now.Add(time.Hour), Token: "xsts-token",
				DisplayClaims: xsts.DisplayClaims{UserInfo: []xsts.UserInfo{{UserInfo: xasu.UserInfo{UserHash: "user-hash"}, XUID: "123"}}},
			},
		}},
		ServiceToken: testServiceToken(serviceExpiry),
	}
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
}

func decodeProofKey(t *testing.T, encoded string) *ecdsa.PrivateKey {
	t.Helper()
	der, err := base64.RawStdEncoding.DecodeString(encoded)
	if err != nil {
		t.Fatal(err)
	}
	key, err := x509.ParseECPrivateKey(der)
	if err != nil {
		t.Fatal(err)
	}
	return key
}

func testOAuthToken(account string) *oauth2.Token {
	return &oauth2.Token{AccessToken: "access-" + account, RefreshToken: "refresh-" + account, TokenType: "Bearer", Expiry: time.Now().Add(time.Hour)}
}

func testEnvironment() *service.AuthorizationEnvironment {
	serviceURI, _ := url.Parse("https://service.example.test")
	issuer, _ := url.Parse("https://issuer.example.test")
	return &service.AuthorizationEnvironment{ServiceURI: serviceURI, Issuer: issuer, PlayFabTitleID: "20CA2"}
}

func testServiceToken(expiry time.Time) *service.Token {
	return &service.Token{AuthorizationHeader: "MCToken service-secret", ValidUntil: expiry}
}

type sequenceOAuthSource struct {
	mu     sync.Mutex
	tokens []*oauth2.Token
}

func (s *sequenceOAuthSource) Token() (*oauth2.Token, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.tokens) == 0 {
		return nil, errors.New("no token")
	}
	token := s.tokens[0]
	if len(s.tokens) > 1 {
		s.tokens = s.tokens[1:]
	}
	return token, nil
}

// A rejected XSTS token must stay evicted across a reload of the persisted bundle.
func TestPersistentSourceInvalidatedXSTSTokenIsNotResurrected(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{}).(*Account)
	var invalidator nsal.TokenInvalidator = source
	rejected := source.session.Snapshot().XSTSTokens[cachedRelyingParty]
	if rejected == nil {
		t.Fatal("fixture did not restore the XSTS token")
	}
	invalidator.InvalidateXSTSToken(cachedRelyingParty, rejected)
	if source.session.Snapshot().XSTSTokens[cachedRelyingParty] != nil {
		t.Fatal("in-memory session kept the rejected token")
	}
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	if state.SISU.XSTSTokens[cachedRelyingParty] != nil {
		t.Fatal("persisted bundle kept the rejected token")
	}
	fresh := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{}).(*Account)
	if fresh.session.Snapshot().XSTSTokens[cachedRelyingParty] != nil {
		t.Fatal("a fresh process restored the rejected token")
	}
	// A stale bundle written without the eviction must not resurrect it on reload.
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	source.gate <- struct{}{}
	source.reloadLocked()
	source.unlock()
	if source.session.Snapshot().XSTSTokens[cachedRelyingParty] != nil {
		t.Fatal("reload resurrected the rejected token")
	}
}

// persistentSource returns the account through its OAuth face so tests reach it via interfaces.
func persistentSource(ctx context.Context, path string, oauth oauth2.TokenSource, diagnostics io.Writer, deps derivedDeps) oauth2.TokenSource {
	return newAccount(ctx, path, oauth, diagnostics, deps)
}

// fakeServices stands in for the native service-token source: a valid token is reused, otherwise
// exchange issues the next one.
func fakeServices(exchange func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error)) func(*service.AuthorizationEnvironment, service.SessionTicketSource, *service.Token) service.TokenSource {
	return func(env *service.AuthorizationEnvironment, _ service.SessionTicketSource, token *service.Token) service.TokenSource {
		return &fakeServiceSource{env: env, token: token, exchange: exchange}
	}
}

type fakeServiceSource struct {
	env      *service.AuthorizationEnvironment
	token    *service.Token
	exchange func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error)
}

func (f *fakeServiceSource) ServiceToken(ctx context.Context) (*service.Token, error) {
	if f.token != nil && f.token.Valid() {
		return f.token, nil
	}
	token, err := f.exchange(ctx, f.env, nil)
	if err != nil {
		return nil, err
	}
	f.token = token
	return token, nil
}

func (f *fakeServiceSource) InvalidateServiceToken(rejected *service.Token) {
	if f.token != nil && f.token.AuthorizationHeader == rejected.AuthorizationHeader {
		f.token = nil
	}
}

// mintFromService fetches the service token the way the native multiplayer source does.
func mintFromService(ctx context.Context, _ *service.AuthorizationEnvironment, src service.TokenSource, _ *ecdsa.PublicKey) (string, error) {
	if _, err := src.ServiceToken(ctx); err != nil {
		return "", err
	}
	return "fresh-jwt", nil
}

type fakeIdentityProvider struct{ logins *atomic.Int32 }

func (p fakeIdentityProvider) Login(context.Context, *http.Client, playfab.LoginRequest) (*playfab.LoginResult, error) {
	p.logins.Add(1)
	return &playfab.LoginResult{
		EntityToken:   &entity.Token{Entity: entity.Key{Type: entity.TypeTitlePlayerAccount, ID: "title"}, Token: "entity", Expiration: time.Now().Add(time.Hour)},
		PlayFabID:     "master",
		SessionTicket: "ticket",
	}, nil
}

type refusingTransport struct{}

func (refusingTransport) RoundTrip(*http.Request) (*http.Response, error) {
	return nil, errors.New("offline test")
}

// Every consumer shares one PlayFab session, and Close ends it and refuses further service calls.
func TestAccountSharesOnePlayFabSessionUntilClosed(t *testing.T) {
	var logins atomic.Int32
	var tickets []string
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		login: func(ctx context.Context, env *service.AuthorizationEnvironment, _ xsapi.TokenAndSignaturer) (*playfab.Client, error) {
			return playfab.Login(ctx, env.PlayFabTitleID, fakeIdentityProvider{&logins}, playfab.ClientConfig{
				HTTPClient: &http.Client{Transport: refusingTransport{}}, Logger: slog.New(slog.DiscardHandler),
			})
		},
		services: func(_ *service.AuthorizationEnvironment, source service.SessionTicketSource, _ *service.Token) service.TokenSource {
			return fakeServiceSourceFunc(func(ctx context.Context) (*service.Token, error) {
				ticket, err := source.SessionTicket(ctx)
				tickets = append(tickets, ticket)
				return testServiceToken(time.Now().Add(time.Hour)), err
			})
		},
	}
	account := newAccount(context.Background(), "", oauth2.StaticTokenSource(testOAuthToken("account-a")), nil, deps)
	first, err := account.PlayFab(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if _, err := account.ServiceToken(context.Background()); err != nil {
		t.Fatal(err)
	}
	second, err := account.PlayFab(context.Background())
	if err != nil || first != second || logins.Load() != 1 || len(tickets) != 1 || tickets[0] != "ticket" {
		t.Fatalf("shared=%v logins=%d tickets=%v err=%v", first == second, logins.Load(), tickets, err)
	}
	if err := account.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-first.TitlePlayerAccount().Context().Done():
	case <-time.After(time.Second):
		t.Fatal("Close left the PlayFab session running")
	}
	if _, err := account.PlayFab(context.Background()); !errors.Is(err, ErrAccountClosed) {
		t.Fatalf("PlayFab after Close err = %v", err)
	}
	if _, err := account.ServiceToken(context.Background()); !errors.Is(err, ErrAccountClosed) {
		t.Fatalf("ServiceToken after Close err = %v", err)
	}
}

type fakeServiceSourceFunc func(context.Context) (*service.Token, error)

func (f fakeServiceSourceFunc) ServiceToken(ctx context.Context) (*service.Token, error) {
	return f(ctx)
}
