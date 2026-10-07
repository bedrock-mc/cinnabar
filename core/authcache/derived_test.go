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
	"net/http/httptest"
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
	"github.com/google/uuid"
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
	settle(t, source.(*Account))
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
	settle(t, account)
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

// Concurrent joins needing an expired service token share one exchange.
func TestConcurrentExpiredRefreshUsesOneExchange(t *testing.T) {
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
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	var wg sync.WaitGroup
	errs := make(chan error, 4)
	for range cap(errs) {
		wg.Go(func() {
			key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
			_, err := account.MultiplayerToken(context.Background(), &key.PublicKey)
			errs <- err
		})
	}
	wg.Wait()
	close(errs)
	for err := range errs {
		if err != nil {
			t.Fatal(err)
		}
	}
	if calls := serviceCalls.Load(); calls != 1 {
		t.Fatalf("concurrent service exchanges = %d, want 1", calls)
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
	t.Cleanup(func() { _ = source.(*Account).Close() })
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
	settle(t, source)
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
func fakeServices(exchange func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error)) func(*service.AuthorizationEnvironment, service.SessionTicketSource, *service.Token, string, string) service.TokenSource {
	return func(env *service.AuthorizationEnvironment, _ service.SessionTicketSource, token *service.Token, _, _ string) service.TokenSource {
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
		services: func(_ *service.AuthorizationEnvironment, source service.SessionTicketSource, _ *service.Token, _, _ string) service.TokenSource {
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

// Every service token source an account rebuilds, and its multiplayer mint, name one Session-Id.
func TestAccountKeepsOneSessionIDAcrossRebuilds(t *testing.T) {
	var mu sync.Mutex
	sessions := map[string][]string{}
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		mu.Lock()
		sessions[r.URL.Path] = append(sessions[r.URL.Path], r.Header.Get("Session-Id"))
		mu.Unlock()
		now := time.Now().UTC().Truncate(time.Second)
		if r.URL.Path == "/api/v1.0/multiplayer/session/start" {
			_ = json.NewEncoder(w).Encode(map[string]any{"result": map[string]any{
				"issuedAt": now, "signedToken": "multiplayer-token", "validUntil": now.Add(time.Hour),
			}})
			return
		}
		payload, _ := json.Marshal(map[string]any{"pmid": uuid.NewString(), "iat": now.Unix(), "exp": now.Add(time.Hour).Unix()})
		_ = json.NewEncoder(w).Encode(map[string]any{"result": &service.Token{
			AuthorizationHeader: "MCToken header." + base64.RawURLEncoding.EncodeToString(payload) + ".signature",
			ValidUntil:          now.Add(time.Hour),
		}})
	}))
	defer server.Close()
	deps := defaultDerivedDeps()
	deps.discover = func(context.Context) (*service.AuthorizationEnvironment, error) {
		env := testEnvironment()
		env.ServiceURI, _ = url.Parse(server.URL)
		env.HTTPClient = server.Client()
		return env, nil
	}
	var logins atomic.Int32
	deps.login = func(ctx context.Context, env *service.AuthorizationEnvironment, _ xsapi.TokenAndSignaturer) (*playfab.Client, error) {
		return playfab.Login(ctx, env.PlayFabTitleID, fakeIdentityProvider{&logins}, playfab.ClientConfig{
			HTTPClient: &http.Client{Transport: refusingTransport{}}, Logger: slog.New(slog.DiscardHandler),
		})
	}
	account := newAccount(context.Background(), "", oauth2.StaticTokenSource(testOAuthToken("account-a")), nil, deps)
	defer account.Close()

	token, err := account.ServiceToken(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	account.InvalidateServiceToken(token)
	if _, err := account.ServiceToken(context.Background()); err != nil {
		t.Fatal(err)
	}
	if _, err := account.refreshServiceAhead(context.Background(), 2*time.Hour); err != nil {
		t.Fatal(err)
	}
	key, _ := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if _, err := account.MultiplayerToken(context.Background(), &key.PublicKey); err != nil {
		t.Fatal(err)
	}

	mu.Lock()
	defer mu.Unlock()
	starts, mints := sessions["/api/v1.0/session/start"], sessions["/api/v1.0/multiplayer/session/start"]
	if len(starts) != 3 || len(mints) != 1 {
		t.Fatalf("requests = %v, want three service exchanges and one mint", sessions)
	}
	if _, err := uuid.Parse(starts[0]); err != nil {
		t.Fatalf("Session-Id %q is not a UUID", starts[0])
	}
	if starts[1] != starts[0] || starts[2] != starts[0] || mints[0] != starts[0] {
		t.Fatalf("Session-Ids = (exchanges %q, mint %q), want one per account", starts, mints)
	}
}

// An XSTS request that read a token before its invalidation finished never returns or reinstalls it.
func TestXSTSOverlappingInvalidationNeverReturnsTheRejectedToken(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	// The SISU cache still holds the token, as when an invalidation has recorded it but not yet evicted it.
	account.session = newAccountSession(account, &sisu.SessionConfig{Snapshot: state.SISU, DeviceTokenSource: generationDeviceSource{}})
	rejected := state.SISU.XSTSTokens[cachedRelyingParty]
	account.rejected = map[string]*xsts.Token{cachedRelyingParty: rejected}
	delete(account.xstsTokens, cachedRelyingParty)
	token, err := account.deriveXSTS(context.Background(), cachedRelyingParty)
	if token != nil && token.Token == rejected.Token {
		t.Fatal("overlapping request returned the rejected token")
	}
	if err == nil {
		t.Fatal("offline re-request succeeded")
	}
	if account.xstsTokens[cachedRelyingParty] != nil {
		t.Fatal("overlapping request reinstalled the rejected token")
	}
}

// A snapshot taken before an invalidation finished never writes the rejected token back to disk.
func TestStaleSnapshotNeverPublishesTheRejectedToken(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	rejected := account.xstsTokens[cachedRelyingParty]
	account.rejected = map[string]*xsts.Token{cachedRelyingParty: rejected}
	account.publishNow(context.Background())
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	if state.SISU.XSTSTokens[cachedRelyingParty] != nil {
		t.Fatal("publish wrote the rejected token")
	}
}

// Diagnostics written by concurrent credential calls never race on the caller's writer.
func TestConcurrentCallsSerializeDiagnostics(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var diagnostics bytes.Buffer
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), &diagnostics, derivedDeps{})
	defer account.Close()
	var wg sync.WaitGroup
	for range 8 {
		wg.Go(func() {
			if _, err := account.XSTSToken(context.Background(), cachedRelyingParty); err != nil {
				t.Error(err)
			}
		})
	}
	wg.Wait()
}

// A service-token eviction survives another process publishing the refused token concurrently.
func TestServiceEvictionSurvivesAConcurrentPublication(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	rejected := account.service
	account.gate <- struct{}{}
	account.rejectedService, account.service = rejected, nil
	account.unlock()
	// Another process republishes a compatible bundle that still carries the refused token.
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	other := *state.SISU.XSTSTokens[cachedRelyingParty]
	state.SISU.XSTSTokens["https://other.example.test/"] = &other
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
	account.publishNow(context.Background())
	if state, err := loadDerived(path); err != nil || state.ServiceToken != nil {
		t.Fatalf("publish kept the refused service token on disk: err=%v", err)
	}
	account.gate <- struct{}{}
	account.reloadLocked()
	service := account.service
	account.unlock()
	if service != nil {
		t.Fatal("reload resurrected the refused service token")
	}
}

// Adopting another process's bundle keeps this account's fresher service token and XSTS tokens.
func TestPublishMergesFresherLocalCredentialsIntoAnotherBundle(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			return nil, errors.New("offline test")
		}),
	}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	if _, err := account.Environment(context.Background()); err != nil {
		t.Fatal(err)
	}
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	local := &service.Token{AuthorizationHeader: "MCToken local", ValidUntil: time.Now().Add(time.Hour)}
	localXSTS := *state.SISU.XSTSTokens[cachedRelyingParty]
	localXSTS.Token, localXSTS.NotAfter = "local-xsts", time.Now().Add(2*time.Hour)
	account.gate <- struct{}{}
	account.service = local
	account.session = newAccountSession(account, &sisu.SessionConfig{
		Snapshot: &sisu.Snapshot{XSTSTokens: map[string]*xsts.Token{cachedRelyingParty: &localXSTS}}, DeviceTokenSource: account.device,
	})
	account.unlock()
	// Another process publishes an unrelated XSTS token while this account's credentials are unpublished.
	other := *state.SISU.XSTSTokens[cachedRelyingParty]
	state.SISU.XSTSTokens["https://other.example.test/"] = &other
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
	account.publishNow(context.Background())
	published, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	if published.ServiceToken == nil || published.ServiceToken.AuthorizationHeader != local.AuthorizationHeader {
		t.Fatal("adopting another bundle dropped the fresher local service token")
	}
	if published.SISU.XSTSTokens[cachedRelyingParty].Token != "local-xsts" || published.SISU.XSTSTokens["https://other.example.test/"] == nil {
		t.Fatal("merged bundle lost the local or the published XSTS token")
	}
	if token, err := account.ServiceToken(context.Background()); err != nil || token.AuthorizationHeader != local.AuthorizationHeader {
		t.Fatalf("account dropped its fresher service token: err=%v", err)
	}
}

// A service exchange whose own SISU refresh rotated the OAuth token still installs and persists.
func TestServiceExchangeSurvivesItsOwnOAuthRotation(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oldToken, rotated := testOAuthToken("account-a"), testOAuthToken("account-a-rotated")
	writeDerivedState(t, path, oldToken, time.Now().Add(-time.Minute))
	var account *Account
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			if _, err := account.session.Token(); err != nil { // SISU's own refresh reads the rotated token
				return nil, err
			}
			return &service.Token{AuthorizationHeader: "MCToken exchanged", ValidUntil: time.Now().Add(time.Hour)}, nil
		}),
	}
	oauth := &sequenceOAuthSource{tokens: []*oauth2.Token{oldToken, oldToken, rotated}}
	account = newAccount(context.Background(), path, oauth, nil, deps)
	defer account.Close()
	if _, err := account.ServiceToken(context.Background()); err != nil {
		t.Fatal(err)
	}
	settle(t, account)
	if state, err := loadDerived(path); err != nil || state.ServiceToken == nil || state.ServiceToken.AuthorizationHeader != "MCToken exchanged" {
		t.Fatalf("exchanged token was not persisted after its own OAuth rotation: err=%v", err)
	}
}

// A recorded rejection stops the cached-token path even before the invalidation finishes.
func TestCachedXSTSPathHonoursRecordedRejection(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	rejected := account.xstsTokens[cachedRelyingParty]
	account.gate <- struct{}{}
	account.session = newAccountSession(account, &sisu.SessionConfig{Snapshot: state.SISU, DeviceTokenSource: generationDeviceSource{}})
	account.xstsTokens[cachedRelyingParty] = state.SISU.XSTSTokens[cachedRelyingParty]
	account.rejected = map[string]*xsts.Token{cachedRelyingParty: rejected}
	account.unlock()
	if token, _ := account.XSTSToken(context.Background(), cachedRelyingParty); token != nil && token.Token == rejected.Token {
		t.Fatal("cached path returned a token already recorded as rejected")
	}
}

// A merge never restores a synced XSTS token another process has since evicted.
func TestPublishMergeKeepsAnotherProcessEviction(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	// Another process evicts the shared token; this account still holds it, unchanged since its last sync.
	delete(state.SISU.XSTSTokens, cachedRelyingParty)
	state.ServiceToken = nil
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
	account.publishNow(context.Background())
	published, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	if published.SISU.XSTSTokens[cachedRelyingParty] != nil || published.ServiceToken != nil {
		t.Fatal("merge restored credentials another process evicted")
	}
}

// A delayed service exchange never replaces a fresher token another refresh already installed.
func TestDelayedServiceExchangeKeepsAFresherToken(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	fresher := &service.Token{AuthorizationHeader: "MCToken fresher", ValidUntil: time.Now().Add(2 * time.Hour)}
	var account *Account
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			account.gate <- struct{}{}
			account.service = fresher // adopted from another process mid-exchange
			account.unlock()
			return &service.Token{AuthorizationHeader: "MCToken older", ValidUntil: time.Now().Add(time.Hour)}, nil
		}),
	}
	account = newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	token, err := account.ServiceToken(context.Background())
	if err != nil || token != fresher {
		t.Fatalf("delayed exchange returned %v, err=%v; want the fresher token", token, err)
	}
	account.gate <- struct{}{}
	current := account.service
	account.unlock()
	if current != fresher {
		t.Fatal("delayed exchange replaced the fresher token")
	}
}

// Reloading another process's bundle keeps credentials this account derived but has not yet published.
func TestReloadKeepsUnpublishedLocalCredentials(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	deps := derivedDeps{discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil }}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	if _, err := account.Environment(context.Background()); err != nil {
		t.Fatal(err)
	}
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	local := &service.Token{AuthorizationHeader: "MCToken local", ValidUntil: time.Now().Add(time.Hour)}
	localXSTS := *state.SISU.XSTSTokens[cachedRelyingParty]
	localXSTS.Token, localXSTS.NotAfter = "local-xsts", time.Now().Add(2*time.Hour)
	account.gate <- struct{}{}
	account.service = local
	account.xstsTokens[cachedRelyingParty] = &localXSTS
	account.unlock()
	other := *state.SISU.XSTSTokens[cachedRelyingParty]
	state.SISU.XSTSTokens["https://other.example.test/"] = &other
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
	if _, err := account.Environment(context.Background()); err != nil {
		t.Fatal(err)
	}
	account.gate <- struct{}{}
	service, xstsToken := account.service, account.xstsTokens[cachedRelyingParty]
	account.unlock()
	if service != local || xstsToken == nil || xstsToken.Token != "local-xsts" {
		t.Fatal("reload discarded credentials this account had not yet published")
	}
	// A further unrelated publication must not make the still-unpublished credentials look synced.
	third := *state.SISU.XSTSTokens[cachedRelyingParty]
	state.SISU.XSTSTokens["https://third.example.test/"] = &third
	if b, err = json.Marshal(state); err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
	account.publishNow(context.Background())
	published, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	if published.ServiceToken == nil || published.ServiceToken.AuthorizationHeader != local.AuthorizationHeader ||
		published.SISU.XSTSTokens[cachedRelyingParty].Token != "local-xsts" {
		t.Fatal("unpublished credentials were treated as synced and dropped")
	}
}

// Adopting another bundle during publication keeps an XSTS token derived after the snapshot was taken.
func TestPublishKeepsXSTSDerivedAfterItsSnapshot(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	fresh := *state.SISU.XSTSTokens[cachedRelyingParty]
	fresh.Token, fresh.NotAfter = "fresh-xsts", time.Now().Add(2*time.Hour)
	account.gate <- struct{}{}
	account.xstsTokens["https://fresh.example.test/"] = &fresh // in the mirror, not yet in any SISU snapshot
	account.unlock()
	other := *state.SISU.XSTSTokens[cachedRelyingParty]
	state.SISU.XSTSTokens["https://other.example.test/"] = &other
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
	account.publishNow(context.Background())
	published, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	if token := published.SISU.XSTSTokens["https://fresh.example.test/"]; token == nil || token.Token != "fresh-xsts" {
		t.Fatal("adoption during publication lost an XSTS token derived after the snapshot")
	}
}

// A bundle this account cannot restore, such as another cold start's proof key, is replaced on publish.
func TestPublishReplacesAnUnrestorableBundle(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	want := account.ProofKey()
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour)) // same account, another proof key
	account.publishNow(context.Background())
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	if got := decodeProofKey(t, state.ProofKey); got.D.Cmp(want.D) != 0 {
		t.Fatal("publication left an unrestorable bundle in place")
	}
}
