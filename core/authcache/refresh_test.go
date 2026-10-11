package authcache

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"encoding/base64"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"net/url"
	"path/filepath"
	"regexp"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"

	"github.com/df-mc/go-xsapi/v2"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

func countingServiceDeps(exchanges *atomic.Int32, deviceIDs chan<- string) derivedDeps {
	return derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: func(env *service.AuthorizationEnvironment, tickets service.SessionTicketSource, token *service.Token, deviceID, sessionID string) service.TokenSource {
			if deviceIDs != nil {
				deviceIDs <- deviceID
			}
			return fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
				exchanges.Add(1)
				return testServiceToken(time.Now().Add(time.Hour)), nil
			})(env, tickets, token, deviceID, sessionID)
		},
	}
}

// rewriteXUID replaces the fixture account's XUID in its persisted bundle.
func rewriteXUID(t *testing.T, path, xuid string) {
	t.Helper()
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, token := range state.SISU.XSTSTokens {
		for index := range token.DisplayClaims.UserInfo {
			token.DisplayClaims.UserInfo[index].XUID = xuid
		}
	}
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
}

func serviceDeviceID(t *testing.T, xuid string) string {
	t.Helper()
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	rewriteXUID(t, path, xuid)
	var exchanges atomic.Int32
	deviceIDs := make(chan string, 4)
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, countingServiceDeps(&exchanges, deviceIDs))
	defer account.Close()
	if _, err := account.ServiceToken(context.Background()); err != nil {
		t.Fatal(err)
	}
	return <-deviceIDs
}

// Service sessions present one device per account across processes, never a fresh random one.
func TestServiceDeviceIDIsStablePerAccount(t *testing.T) {
	first, again, other := serviceDeviceID(t, "123"), serviceDeviceID(t, "123"), serviceDeviceID(t, "456")
	if !regexp.MustCompile(`^[0-9a-f]{32}$`).MatchString(first) || first != again || first == other {
		t.Fatalf("device IDs = %q, %q, %q; want one undashed ID per account", first, again, other)
	}
	if unknown := serviceDeviceID(t, ""); unknown != "" {
		t.Fatalf("device ID without an XUID = %q, want the random fallback", unknown)
	}
}

// A service token near expiry is replaced in the background and persisted for later processes.
func TestRefreshAheadReplacesServiceTokenBeforeExpiry(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(serviceRefreshLead/2))
	var exchanges atomic.Int32
	deps := countingServiceDeps(&exchanges, nil)
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	remaining, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead)
	if err != nil || exchanges.Load() != 1 || remaining <= serviceRefreshLead {
		t.Fatalf("refresh: err=%v exchanges=%d remaining=%v", err, exchanges.Load(), remaining)
	}
	settle(t, account)
	if _, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead); err != nil || exchanges.Load() != 1 {
		t.Fatalf("fresh token was refreshed again: err=%v exchanges=%d", err, exchanges.Load())
	}
	other := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer other.Close()
	if _, err := other.refreshServiceAhead(context.Background(), serviceRefreshLead); err != nil || exchanges.Load() != 1 {
		t.Fatalf("another process refreshed a persisted fresh token: err=%v exchanges=%d", err, exchanges.Load())
	}
}

// Signing out ends the background refresher instead of leaving it waiting for the next expiry.
func TestKeepFreshStopsWhenAccountCloses(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var exchanges atomic.Int32
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, countingServiceDeps(&exchanges, nil))
	done := make(chan struct{})
	go func() {
		defer close(done)
		account.KeepFresh(context.Background())
	}()
	if err := account.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case <-done:
	case <-time.After(5 * time.Second):
		t.Fatal("KeepFresh outlived the account")
	}
	if exchanges.Load() != 0 {
		t.Fatalf("a fresh token was exchanged %d times", exchanges.Load())
	}
}

// Sign-in caches the service token, so a later core's first join performs no service exchange.
func TestCompletedSignInLeavesAServiceTokenForTheFirstJoin(t *testing.T) {
	oauthPath := filepath.Join(derivedTestDir(t), "microsoft-token.json")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, DerivedCachePath(oauthPath), oauthToken, time.Now().Add(-time.Minute))
	var exchanges atomic.Int32
	deps := countingServiceDeps(&exchanges, nil)
	deps.mint = mintFromService
	if err := completeSignIn(context.Background(), oauthPath, oauth2.StaticTokenSource(oauthToken), nil, deps); err != nil || exchanges.Load() != 1 {
		t.Fatalf("sign-in: err=%v exchanges=%d", err, exchanges.Load())
	}
	core := newAccount(context.Background(), DerivedCachePath(oauthPath), oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer core.Close()
	key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := core.MultiplayerToken(context.Background(), &key.PublicKey); err != nil || exchanges.Load() != 1 {
		t.Fatalf("first join: err=%v exchanges=%d, want only the key-bound mint", err, exchanges.Load())
	}
}

// A second refresher for the same live account returns at once instead of doubling refreshes.
func TestKeepFreshRunsOncePerAccount(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var exchanges atomic.Int32
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, countingServiceDeps(&exchanges, nil))
	first := make(chan struct{})
	go func() {
		defer close(first)
		account.KeepFresh(context.Background())
	}()
	testwait.Eventually(t, testwait.DefaultTimeout, "the first refresh to start", account.refreshing.Load)
	second := make(chan struct{})
	go func() {
		defer close(second)
		account.KeepFresh(context.Background())
	}()
	select {
	case <-second:
	case <-time.After(5 * time.Second):
		t.Fatal("a second refresher started for the same account")
	}
	if err := account.Close(); err != nil {
		t.Fatal(err)
	}
	<-first
}

// A failed early exchange keeps the still-valid token in memory and on disk.
func TestFailedEarlyRefreshKeepsTheValidToken(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(serviceRefreshLead/2))
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			return nil, errors.New("exchange unavailable")
		}),
	}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	if _, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead); err == nil {
		t.Fatal("failed exchange reported success")
	}
	token, err := account.ServiceToken(context.Background())
	if err != nil || token.AuthorizationHeader != testServiceToken(time.Time{}).AuthorizationHeader {
		t.Fatalf("valid token was dropped after a failed refresh: err=%v", err)
	}
	if state, err := loadDerived(path); err != nil || state.ServiceToken == nil || !state.ServiceToken.Valid() {
		t.Fatalf("persisted token was cleared: err=%v", err)
	}
}

// A hung exchange ends at the attempt deadline instead of wedging the refresher.
func TestRefreshAttemptIsBoundedByItsDeadline(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(ctx context.Context, _ *service.AuthorizationEnvironment, _ xsapi.TokenAndSignaturer) (*service.Token, error) {
			<-ctx.Done()
			return nil, ctx.Err()
		}),
	}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	attempt, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	done := make(chan error, 1)
	go func() {
		_, err := account.refreshServiceAhead(attempt, serviceRefreshLead)
		done <- err
	}()
	select {
	case err := <-done:
		if err == nil {
			t.Fatal("hung exchange reported success")
		}
	case <-time.After(5 * time.Second):
		t.Fatal("refresh attempt outlived its deadline")
	}
}

// A local clock ahead of the service must not make a fresh token look due for refresh.
func TestRefreshScheduleUsesTheServiceClock(t *testing.T) {
	serviceNow := time.Now().UTC().Add(-90 * time.Minute).Truncate(time.Second)
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Date", serviceNow.Format(http.TimeFormat))
		_ = json.NewEncoder(w).Encode(map[string]any{"result": map[string]any{
			"authorizationHeader": skewedServiceJWT(t, serviceNow, serviceNow.Add(time.Hour)),
			"validUntil":          serviceNow.Add(time.Hour),
		}})
	}))
	defer server.Close()
	serviceURI, _ := url.Parse(server.URL)
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	var exchanges atomic.Int32
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(ctx context.Context, _ *service.AuthorizationEnvironment, _ xsapi.TokenAndSignaturer) (*service.Token, error) {
			exchanges.Add(1)
			env := &service.AuthorizationEnvironment{ServiceURI: serviceURI, HTTPClient: server.Client()}
			return env.Token(ctx, service.TokenConfig{User: service.UserConfig{Token: "playfab-token"}})
		}),
	}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	for attempt := range 3 {
		remaining, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead)
		if err != nil || remaining < 50*time.Minute {
			t.Fatalf("attempt %d: err=%v remaining=%v, want the service-clock lifetime", attempt, err, remaining)
		}
	}
	if exchanges.Load() != 1 {
		t.Fatalf("exchanges = %d, want one despite the skewed local clock", exchanges.Load())
	}
}

func skewedServiceJWT(t *testing.T, issuedAt, expiry time.Time) string {
	t.Helper()
	payload, err := json.Marshal(map[string]any{"pmid": "6a1c9a1e-0000-4000-8000-000000000000", "iat": issuedAt.Unix(), "exp": expiry.Unix()})
	if err != nil {
		t.Fatal(err)
	}
	return "MCToken header." + base64.RawURLEncoding.EncodeToString(payload) + ".signature"
}

// A join keeps using the still-valid token while its early replacement waits on the network.
func TestEarlyRefreshDoesNotBlockForegroundServiceToken(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(serviceRefreshLead/2))
	exchanging, release := make(chan struct{}), make(chan struct{})
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			close(exchanging)
			<-release
			return &service.Token{AuthorizationHeader: "MCToken replacement", ValidUntil: time.Now().Add(time.Hour)}, nil
		}),
	}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	var released sync.Once
	defer released.Do(func() { close(release) })
	refreshed := make(chan error, 1)
	go func() {
		_, err := account.refreshServiceAhead(context.Background(), serviceRefreshLead)
		refreshed <- err
	}()
	<-exchanging
	foreground, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	token, err := account.ServiceToken(foreground)
	if err != nil || token.AuthorizationHeader != testServiceToken(time.Time{}).AuthorizationHeader {
		t.Fatalf("foreground token during an early refresh: err=%v", err)
	}
	released.Do(func() { close(release) })
	if err := <-refreshed; err != nil {
		t.Fatal(err)
	}
	if token, err := account.ServiceToken(context.Background()); err != nil || token.AuthorizationHeader != "MCToken replacement" {
		t.Fatalf("replacement was not installed: err=%v", err)
	}
}

// A service token restored from disk carries its JWT claims, which messaging reads directly.
func TestRestoredServiceTokenCarriesItsClaims(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Now().UTC().Truncate(time.Second)
	state.ServiceToken = &service.Token{AuthorizationHeader: skewedServiceJWT(t, now, now.Add(time.Hour)), ValidUntil: now.Add(time.Hour)}
	b, err := json.Marshal(state)
	if err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, append(b, '\n')); err != nil {
		t.Fatal(err)
	}
	deps := defaultDerivedDeps()
	deps.discover = func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil }
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	token, err := account.ServiceToken(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if token.Claims.PlayerMessagingID.String() != "6a1c9a1e-0000-4000-8000-000000000000" {
		t.Fatalf("restored service token messaging ID = %v", token.Claims.PlayerMessagingID)
	}
}

// An exchange superseded by an account reset derives again instead of returning the stale token.
func TestSupersededServiceExchangeDerivesAgain(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	var account *Account
	var exchanges atomic.Int32
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			if exchanges.Add(1) == 1 {
				account.gate <- struct{}{}
				account.resetLocked(oauthBinding(testOAuthToken("account-a-rotated")))
				account.unlock()
				return &service.Token{AuthorizationHeader: "MCToken superseded", ValidUntil: time.Now().Add(time.Hour)}, nil
			}
			return &service.Token{AuthorizationHeader: "MCToken current", ValidUntil: time.Now().Add(time.Hour)}, nil
		}),
	}
	account = newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	account.gate <- struct{}{}
	account.oauth = oauthSourceFunc(func() (*oauth2.Token, error) { return testOAuthToken("account-a-rotated"), nil })
	account.binding = oauthBinding(testOAuthToken("account-a-rotated"))
	account.unlock()
	token, err := account.ServiceToken(context.Background())
	if err != nil || token.AuthorizationHeader != "MCToken current" {
		t.Fatalf("superseded exchange returned %v, err=%v", token, err)
	}
}

// resumingSource hands back its seed after onResume runs, as a native source resuming a restored token.
type resumingSource struct {
	seed     *service.Token
	onResume func()
	exchange func() *service.Token
}

func (r *resumingSource) ServiceToken(context.Context) (*service.Token, error) {
	if r.seed != nil && r.seed.Valid() {
		r.onResume()
		return r.seed, nil
	}
	return r.exchange(), nil
}

// A restored token refused while its source resumes it is never installed; a fresh one is exchanged.
func TestServiceTokenRefusedWhileResumingIsNotInstalled(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var account *Account
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: func(_ *service.AuthorizationEnvironment, _ service.SessionTicketSource, seed *service.Token, _, _ string) service.TokenSource {
			return &resumingSource{
				seed:     seed,
				onResume: func() { account.InvalidateServiceToken(seed) },
				exchange: func() *service.Token {
					return &service.Token{AuthorizationHeader: "MCToken replacement", ValidUntil: time.Now().Add(time.Hour)}
				},
			}
		},
	}
	account = newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	token, err := account.ServiceToken(context.Background())
	if err != nil || token.AuthorizationHeader != "MCToken replacement" {
		t.Fatalf("service token after a refusal during resume = %v, err=%v", token, err)
	}
}

// A restored token another process evicted while its source resumed it is never reinstalled.
func TestServiceTokenEvictedByAReloadMidExchangeIsNotInstalled(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var account *Account
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: func(_ *service.AuthorizationEnvironment, _ service.SessionTicketSource, seed *service.Token, _, _ string) service.TokenSource {
			return &resumingSource{
				seed: seed,
				onResume: func() {
					// Another process evicts the token, and a concurrent call reloads that eviction.
					state, err := loadDerived(path)
					if err != nil {
						t.Error(err)
						return
					}
					state.ServiceToken = nil
					b, _ := json.Marshal(state)
					if err := savePrivate(path, append(b, '\n')); err != nil {
						t.Error(err)
					}
					if _, err := account.Environment(context.Background()); err != nil {
						t.Error(err)
					}
				},
				exchange: func() *service.Token {
					return &service.Token{AuthorizationHeader: "MCToken replacement", ValidUntil: time.Now().Add(time.Hour)}
				},
			}
		},
	}
	account = newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	defer account.Close()
	token, err := account.ServiceToken(context.Background())
	if err != nil || token.AuthorizationHeader != "MCToken replacement" {
		t.Fatalf("service token after an eviction mid-exchange = %v, err=%v", token, err)
	}
}
