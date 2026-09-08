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
	"io/fs"
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2"
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

func TestPersistentSourceRetainsCanonicalCachePath(t *testing.T) {
	raw := filepath.Join(t.TempDir(), "alias", "derived")
	canonical := filepath.Join(t.TempDir(), "canonical", "derived")
	var input string
	deps := derivedDeps{canonicalize: func(path string) (string, error) {
		input = path
		return canonical, nil
	}}
	source := persistentSource(context.Background(), raw, oauth2.StaticTokenSource(testOAuthToken("account-a")), nil, deps)
	persistent, ok := source.(*persistentAuthSource)
	if !ok {
		t.Fatal("persistent source was not constructed")
	}
	wantInput, err := filepath.Abs(raw)
	if err != nil {
		t.Fatal(err)
	}
	if input != filepath.Clean(wantInput) {
		t.Fatalf("canonicalization input = %q, want absolute clean path %q", input, filepath.Clean(wantInput))
	}
	if persistent.path != canonical {
		t.Fatalf("persistent path = %q, want canonical path %q", persistent.path, canonical)
	}
}

func TestPersistentSourceRejectsUntrustedCanonicalization(t *testing.T) {
	oauth := oauth2.StaticTokenSource(testOAuthToken("account-a"))
	deps := derivedDeps{canonicalize: func(string) (string, error) {
		return "", errors.New("untrusted alias")
	}}
	if got := persistentSource(context.Background(), filepath.Join(t.TempDir(), "derived"), oauth, nil, deps); got != oauth {
		t.Fatal("persistent source retained an untrusted cache path")
	}
}

func TestPersistentSourceFreshInstanceReusesDerivedStateAndMintsPerKey(t *testing.T) {
	path := filepath.Join(t.TempDir(), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var discoveryCalls, serviceCalls, mintCalls int
	var minted []string
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) {
			discoveryCalls++
			return testEnvironment(), nil
		},
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		},
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
	path := filepath.Join(t.TempDir(), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	var discoveryCalls, serviceCalls int
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) {
			discoveryCalls++
			return testEnvironment(), nil
		},
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		},
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
	path := filepath.Join(t.TempDir(), "derived")
	oldToken := testOAuthToken("account-a")
	newToken := testOAuthToken("account-b")
	writeDerivedState(t, path, oldToken, time.Now().Add(time.Hour))
	oauth := &sequenceOAuthSource{tokens: []*oauth2.Token{oldToken, newToken}}
	source := persistentSource(context.Background(), path, oauth, nil, derivedDeps{})
	if _, err := source.Token(); err != nil {
		t.Fatal(err)
	}
	persistent := source.(*persistentAuthSource)
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
	path := filepath.Join(t.TempDir(), "derived")
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
	path := filepath.Join(t.TempDir(), "derived")
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
	path := filepath.Join(t.TempDir(), "derived")
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
			path := filepath.Join(t.TempDir(), "derived")
			token := testOAuthToken("account-a")
			arrange(t, path, token)
			var diagnostics bytes.Buffer
			source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(token), &diagnostics, derivedDeps{})
			persistent := source.(*persistentAuthSource)
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
	path := filepath.Join(t.TempDir(), "derived")
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
	restored := source.(*persistentAuthSource).ProofKey()
	if restored.D.Cmp(cachedKey.D) == 0 {
		t.Fatal("valid SISU/proof key was partially restored from a bundle with an invalid environment")
	}
}

func TestPersistentSourceMultiplayerCallChecksOAuthBindingBeforeReuse(t *testing.T) {
	path := filepath.Join(t.TempDir(), "derived")
	oldToken := testOAuthToken("account-a")
	newToken := testOAuthToken("account-b")
	writeDerivedState(t, path, oldToken, time.Now().Add(time.Hour))
	var serviceCalls int
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		},
		mint: func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error) {
			return "fresh-jwt", nil
		},
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

func TestPersistentSourceUnauthorizedServiceTokenRefreshesOnce(t *testing.T) {
	path := filepath.Join(t.TempDir(), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var serviceCalls, mintCalls int
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		},
		mint: func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error) {
			mintCalls++
			if mintCalls == 1 {
				return "", &service.ResponseError{StatusCode: http.StatusUnauthorized}
			}
			return "fresh-jwt", nil
		},
	}
	source := persistentSource(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	key, _ := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if _, err := source.(minecraft.MultiplayerTokenSource).MultiplayerToken(context.Background(), &key.PublicKey); err != nil {
		t.Fatal(err)
	}
	if serviceCalls != 1 || mintCalls != 2 {
		t.Fatalf("calls = (service=%d mint=%d), want bounded (1,2)", serviceCalls, mintCalls)
	}
}

func TestPersistentSourceServiceEnvironmentMismatchDoesNotReuse(t *testing.T) {
	path := filepath.Join(t.TempDir(), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var serviceCalls int
	different := testEnvironment()
	different.ServiceURI, _ = url.Parse("https://replacement.example.test")
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return different, nil },
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls++
			return testServiceToken(time.Now().Add(time.Hour)), nil
		},
		mint: func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error) {
			return "fresh-jwt", nil
		},
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
	path := filepath.Join(t.TempDir(), "derived")
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
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			calls++
			return nil, context.Canceled
		},
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
	path := filepath.Join(t.TempDir(), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			return testServiceToken(time.Now().Add(time.Hour)), nil
		},
		mint: func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error) {
			return "fresh-jwt", nil
		},
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
	path := filepath.Join(t.TempDir(), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	var serviceCalls atomic.Int32
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			serviceCalls.Add(1)
			time.Sleep(100 * time.Millisecond)
			return testServiceToken(time.Now().Add(time.Hour)), nil
		},
		mint: func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error) {
			return "fresh-jwt", nil
		},
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
	path := filepath.Join(t.TempDir(), "derived")
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
	path := filepath.Join(t.TempDir(), "derived")
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
	path := filepath.Join(t.TempDir(), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	if err := prepareLeasePath(path + ".lock"); err != nil {
		t.Fatal(err)
	}
	lease, err := lockfile.Acquire(path+".lock", 0)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	refreshing := make(chan struct{})
	finishRefresh := make(chan struct{})
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		serviceToken: func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			close(refreshing)
			<-finishRefresh
			return testServiceToken(time.Now().Add(time.Hour)), nil
		},
		mint: func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error) {
			return "fresh-jwt", nil
		},
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

func TestPrepareLeasePathConcurrentFirstCreationKeepsStableIdentity(t *testing.T) {
	path := filepath.Join(t.TempDir(), "derived.lock")
	start := make(chan struct{})
	errs := make(chan error, 8)
	for range cap(errs) {
		go func() {
			<-start
			errs <- prepareLeasePath(path)
		}()
	}
	close(start)
	for range cap(errs) {
		if err := <-errs; err != nil {
			t.Fatal(err)
		}
	}
	before, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	lease, err := lockfile.Acquire(path, 0)
	if err != nil {
		t.Fatal(err)
	}
	created, err := createPrivateOnce(path, []byte("replacement\n"))
	if err != nil {
		t.Fatal(err)
	}
	if created {
		t.Fatal("existing active lease file was replaced")
	}
	after, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if !os.SameFile(before, after) {
		t.Fatal("lease file identity changed while held")
	}
	if second, err := lockfile.Acquire(path, 0); !errors.Is(err, lockfile.ErrBusy) {
		if second != nil {
			_ = second.Close()
		}
		t.Fatalf("second acquisition error = %v, want busy", err)
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(contents) != "join-auth-lease\n" {
		t.Fatalf("lease contents = %q, want original marker", contents)
	}
}

func TestValidateLeasePathRejectsLinkedTarget(t *testing.T) {
	dir := t.TempDir()
	target := filepath.Join(dir, "target")
	if err := os.WriteFile(target, []byte("outside"), 0o600); err != nil {
		t.Fatal(err)
	}
	linked := filepath.Join(dir, "derived.lock")
	if err := os.Symlink(target, linked); err != nil {
		t.Skipf("symlink unavailable: %v", err)
	}
	if err := validateLeasePath(linked); err == nil {
		t.Fatal("linked lease target accepted")
	}
}

func TestValidateLeasePathRejectsMissingTarget(t *testing.T) {
	path := filepath.Join(t.TempDir(), "missing.lock")
	if err := validateLeasePath(path); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("error = %v, want missing", err)
	}
	if lease, err := lockfile.AcquireExisting(path, 0); !errors.Is(err, fs.ErrNotExist) {
		if lease != nil {
			_ = lease.Close()
		}
		t.Fatalf("AcquireExisting error = %v, want missing", err)
	}
	if _, err := os.Lstat(path); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("missing lease was unexpectedly created: %v", err)
	}
}

func TestSavePrivateFailurePreservesOldCache(t *testing.T) {
	path := filepath.Join(t.TempDir(), "derived")
	if err := savePrivate(path, []byte("old\n")); err != nil {
		t.Fatal(err)
	}
	err := savePrivateWithHooks(path, []byte("new\n"), saveHooks{afterTokenSync: func(string) error {
		return errors.New("injected failure")
	}})
	if err == nil {
		t.Fatal("save succeeded, want injected failure")
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
