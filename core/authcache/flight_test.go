package authcache

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"errors"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/xal"
	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

// hangingTransport never connects, like a host whose TCP handshake never completes.
type hangingTransport struct{ started chan struct{} }

func (h hangingTransport) RoundTrip(req *http.Request) (*http.Response, error) {
	select {
	case h.started <- struct{}{}:
	default:
	}
	<-req.Context().Done()
	return nil, req.Context().Err()
}

// hungLoginAccount restores a cached account whose expired service token needs a PlayFab login that hangs.
func hungLoginAccount(t *testing.T) (*Account, <-chan struct{}) {
	t.Helper()
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	started := make(chan struct{}, 1)
	deps := defaultDerivedDeps()
	deps.discover = func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil }
	deps.login = func(ctx context.Context, _ *service.AuthorizationEnvironment, _ xsapi.TokenAndSignaturer) (*playfab.Client, error) {
		if _, ok := ctx.Deadline(); !ok || xal.ContextClient(ctx).Timeout == 0 {
			t.Error("PlayFab login ran without a deadline and a timed HTTP client")
		}
		req, err := http.NewRequestWithContext(ctx, http.MethodGet, "https://title.example.test/titles/current/endpoints", nil)
		if err != nil {
			return nil, err
		}
		_, err = (&http.Client{Transport: hangingTransport{started}}).Do(req)
		return nil, err
	}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	t.Cleanup(func() { _ = account.Close() })
	return account, started
}

// A service-token refresh stuck on an unreachable host must not block a join's XSTS token.
func TestHungServiceRefreshDoesNotBlockOtherCredentials(t *testing.T) {
	account, started := hungLoginAccount(t)
	go func() { _, _ = account.ServiceToken(context.Background()) }()
	select {
	case <-started:
	case <-time.After(5 * time.Second):
		t.Fatal("service refresh never reached PlayFab login")
	}
	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	if token, err := account.XSTSToken(ctx, cachedRelyingParty); err != nil || token.Token != "xsts-token" {
		t.Fatalf("XSTS during a hung refresh: token=%v err=%v", token, err)
	}
	if _, err := account.DeviceToken(ctx); err != nil {
		t.Fatalf("device token during a hung refresh: %v", err)
	}
	if _, err := account.Environment(ctx); err != nil {
		t.Fatalf("environment during a hung refresh: %v", err)
	}
	if account.ProofKey() == nil {
		t.Fatal("proof key unavailable during a hung refresh")
	}
}

// A caller leaving a hung refresh returns at its own deadline without cancelling other waiters.
func TestHungRefreshRespectsEachCallerContext(t *testing.T) {
	account, started := hungLoginAccount(t)
	patient, cancelPatient := context.WithCancel(context.Background())
	defer cancelPatient()
	waiting := make(chan error, 1)
	go func() { _, err := account.ServiceToken(patient); waiting <- err }()
	<-started

	short, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	began := time.Now()
	if _, err := account.ServiceToken(short); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("short caller = %v, want its own deadline", err)
	}
	if elapsed := time.Since(began); elapsed > time.Second {
		t.Fatalf("short caller returned after %v", elapsed)
	}
	select {
	case err := <-waiting:
		t.Fatalf("another caller's deadline ended this wait: %v", err)
	default:
	}
	cancelPatient()
	select {
	case err := <-waiting:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("patient caller = %v, want its own cancellation", err)
		}
	case <-time.After(time.Second):
		t.Fatal("patient caller ignored cancellation")
	}
	closed := make(chan error, 1)
	go func() { closed <- account.Close() }()
	select {
	case <-closed:
	case <-time.After(5 * time.Second):
		t.Fatal("Close waited on the hung refresh")
	}
}

// Concurrent first uses of the PlayFab session share one login.
func TestConcurrentPlayFabCallersShareOneLogin(t *testing.T) {
	var logins atomic.Int32
	release := make(chan struct{})
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		login: func(ctx context.Context, env *service.AuthorizationEnvironment, _ xsapi.TokenAndSignaturer) (*playfab.Client, error) {
			<-release
			return playfab.Login(ctx, env.PlayFabTitleID, fakeIdentityProvider{&logins}, playfab.ClientConfig{
				HTTPClient: &http.Client{Transport: refusingTransport{}}, Logger: slog.New(slog.DiscardHandler),
			})
		},
	}
	account := newAccount(context.Background(), "", oauth2.StaticTokenSource(testOAuthToken("account-a")), nil, deps)
	defer account.Close()
	clients := make(chan *playfab.Client, 4)
	var wg sync.WaitGroup
	for range cap(clients) {
		wg.Go(func() {
			client, err := account.PlayFab(context.Background())
			if err != nil {
				t.Error(err)
			}
			clients <- client
		})
	}
	time.Sleep(50 * time.Millisecond)
	close(release)
	wg.Wait()
	close(clients)
	first := <-clients
	for client := range clients {
		if client != first {
			t.Fatal("concurrent callers received different PlayFab sessions")
		}
	}
	if logins.Load() != 1 {
		t.Fatalf("logins = %d, want 1", logins.Load())
	}
}

// hookDevice runs hook on its first device-token read.
type hookDevice struct {
	token *xasd.Token
	once  sync.Once
	hook  func()
}

func (d *hookDevice) DeviceToken(context.Context) (*xasd.Token, error) {
	d.once.Do(d.hook)
	return d.token, nil
}

func (d *hookDevice) ProofKey() *ecdsa.PrivateKey { return nil }

// An XSTS result from a session replaced mid-request is retried against the current session.
func TestXSTSFromAReplacedSessionIsRetried(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	replacement := *state.SISU.XSTSTokens[cachedRelyingParty]
	replacement.Token = "replacement-xsts"
	device := &hookDevice{token: state.DeviceToken}
	device.hook = func() {
		account.gate <- struct{}{}
		account.session = newAccountSession(account, &sisu.SessionConfig{
			Snapshot:          &sisu.Snapshot{XSTSTokens: map[string]*xsts.Token{cachedRelyingParty: &replacement}},
			DeviceTokenSource: device,
		})
		account.unlock()
	}
	account.gate <- struct{}{}
	account.device = device
	account.session = newAccountSession(account, &sisu.SessionConfig{Snapshot: state.SISU, DeviceTokenSource: device})
	delete(account.xstsTokens, cachedRelyingParty)
	account.unlock()
	token, err := account.XSTSToken(context.Background(), cachedRelyingParty)
	if err != nil || token.Token != "replacement-xsts" {
		t.Fatalf("XSTS after a mid-request session replacement: token=%v err=%v", token, err)
	}
}

// A publish queued behind another snapshots the state current when its turn comes.
func TestQueuedPublishSnapshotsWhenItRuns(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, derivedDeps{})
	defer account.Close()
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	account.publishGate <- struct{}{} // another publication is in progress
	done := make(chan struct{})
	go func() {
		defer close(done)
		account.publishNow(context.Background())
	}()
	time.Sleep(50 * time.Millisecond)
	extra := *state.SISU.XSTSTokens[cachedRelyingParty]
	tokens := map[string]*xsts.Token{cachedRelyingParty: state.SISU.XSTSTokens[cachedRelyingParty], "https://other.example.test/": &extra}
	account.gate <- struct{}{}
	account.session = newAccountSession(account, &sisu.SessionConfig{Snapshot: &sisu.Snapshot{XSTSTokens: tokens}, DeviceTokenSource: account.device})
	account.unlock()
	<-account.publishGate
	<-done
	published, err := loadDerived(path)
	if err != nil || published.SISU.XSTSTokens["https://other.example.test/"] == nil {
		t.Fatalf("queued publish wrote a snapshot older than its turn: err=%v", err)
	}
}

// A login flight that starts after another installed the PlayFab session reuses it.
func TestPlayFabFlightReusesAnInstalledSession(t *testing.T) {
	var logins atomic.Int32
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		login: func(ctx context.Context, env *service.AuthorizationEnvironment, _ xsapi.TokenAndSignaturer) (*playfab.Client, error) {
			return playfab.Login(ctx, env.PlayFabTitleID, fakeIdentityProvider{&logins}, playfab.ClientConfig{
				HTTPClient: &http.Client{Transport: refusingTransport{}}, Logger: slog.New(slog.DiscardHandler),
			})
		},
	}
	account := newAccount(context.Background(), "", oauth2.StaticTokenSource(testOAuthToken("account-a")), nil, deps)
	defer account.Close()
	installed, err := account.PlayFab(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	client, err := account.loginPlayFab(context.Background(), testEnvironment())
	if err != nil || client != installed || logins.Load() != 1 {
		t.Fatalf("late login flight: same=%v logins=%d err=%v", client == installed, logins.Load(), err)
	}
}

// keyedDevice issues a valid device token bound to a real proof key.
type keyedDevice struct{ key *ecdsa.PrivateKey }

func (d keyedDevice) DeviceToken(context.Context) (*xasd.Token, error) {
	return &xasd.Token{Token: "device", NotAfter: time.Now().Add(time.Hour)}, nil
}

func (d keyedDevice) ProofKey() *ecdsa.PrivateKey { return d.key }

// A service token exchanged without any Xbox request still reaches the shared cache.
func TestServiceTokenPublishesWithoutAPriorDeviceRequest(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
	}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(testOAuthToken("account-a")), nil, deps)
	defer account.Close()
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	account.gate <- struct{}{}
	account.device = keyedDevice{key}
	account.session = newAccountSession(account, &sisu.SessionConfig{DeviceTokenSource: account.device})
	account.unlock()
	if _, err := account.ServiceToken(context.Background()); err != nil {
		t.Fatal(err)
	}
	settle(t, account)
	if state, err := loadDerived(path); err != nil || state.ServiceToken == nil {
		t.Fatalf("exchanged service token was not persisted: err=%v", err)
	}
}

// TestMain keeps every test offline: the account's default HTTP client refuses all requests.
func TestMain(m *testing.M) {
	authHTTPClient.Transport = refusingTransport{}
	os.Exit(m.Run())
}

// A derivation that ignores its context still releases its callers at the derivation deadline.
func TestFlightCompletesAtItsDeadline(t *testing.T) {
	previous := derivationTimeout
	derivationTimeout = 50 * time.Millisecond
	defer func() { derivationTimeout = previous }()
	account := newAccount(context.Background(), "", oauth2.StaticTokenSource(testOAuthToken("account-a")), nil, derivedDeps{})
	release := make(chan struct{})
	defer func() { close(release); _ = account.Close() }()
	done := make(chan error, 1)
	go func() {
		_, err := awaitFlight(account, context.Background(), "stuck", func(context.Context) (struct{}, error) {
			<-release
			return struct{}{}, nil
		})
		done <- err
	}()
	select {
	case err := <-done:
		if !errors.Is(err, errDerivationTimeout) {
			t.Fatalf("stuck flight = %v, want the derivation timeout", err)
		}
	case <-time.After(5 * time.Second):
		t.Fatal("stuck flight held its caller past the derivation deadline")
	}
}

// A run that finishes at once is never reported as cancelled.
func TestFinishedFlightIsNeverReportedCancelled(t *testing.T) {
	account := newAccount(context.Background(), "", oauth2.StaticTokenSource(testOAuthToken("account-a")), nil, derivedDeps{})
	defer account.Close()
	for range 2000 {
		if _, err := awaitFlight(account, context.Background(), "instant", func(context.Context) (int, error) { return 1, nil }); err != nil {
			t.Fatalf("finished flight = %v", err)
		}
	}
}

// settle waits for publications still finishing in the background after the calls under test returned.
func settle(t *testing.T, account *Account) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		account.activeMu.Lock()
		active := account.active
		account.activeMu.Unlock()
		if active == 0 {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("background publication never finished")
}

// blockingDevice holds SISU's token lock by never answering until released.
type blockingDevice struct {
	entered chan struct{}
	release chan struct{}
}

func (d blockingDevice) DeviceToken(ctx context.Context) (*xasd.Token, error) {
	close(d.entered)
	select {
	case <-d.release:
	case <-ctx.Done():
	}
	return nil, errors.New("offline test")
}

func (d blockingDevice) ProofKey() *ecdsa.PrivateKey { return nil }

// A credential reaches its caller even while publication waits on an unrelated SISU request.
func TestCredentialIsNotHeldByABusyPublication(t *testing.T) {
	path := filepath.Join(derivedTestDir(t), "derived")
	oauthToken := testOAuthToken("account-a")
	writeDerivedState(t, path, oauthToken, time.Now().Add(-time.Minute))
	deps := derivedDeps{
		discover: func(context.Context) (*service.AuthorizationEnvironment, error) { return testEnvironment(), nil },
		services: fakeServices(func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error) {
			return testServiceToken(time.Now().Add(time.Hour)), nil
		}),
	}
	account := newAccount(context.Background(), path, oauth2.StaticTokenSource(oauthToken), nil, deps)
	device := blockingDevice{entered: make(chan struct{}), release: make(chan struct{})}
	defer func() { close(device.release); _ = account.Close() }()
	account.gate <- struct{}{}
	account.session = newAccountSession(account, &sisu.SessionConfig{DeviceTokenSource: device})
	session := account.session
	account.unlock()
	go func() { _, _ = session.session.TitleToken(context.Background()) }() // an unrelated SISU refresh
	<-device.entered
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	if _, err := account.ServiceToken(ctx); err != nil {
		t.Fatalf("service token while SISU is busy: %v", err)
	}
}
