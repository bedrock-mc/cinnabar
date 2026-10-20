package cinnabarapi

import (
	"context"
	"crypto/ecdsa"
	"crypto/rand"
	"crypto/sha512"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"math/big"
	"net/http"
	"net/http/httptest"
	"strings"
	"sync"
	"sync/atomic"
	"testing"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

// fakeAPI authenticates clients exactly as the contract requires and hands each session to the test.
type fakeAPI struct {
	t              *testing.T
	server         *httptest.Server
	keys           sync.Map // token → *ecdsa.PublicKey
	minted         atomic.Int64
	welcome        parameters
	ownedCosmetics []string
	sessions       chan *fakeSession
}

type fakeSession struct {
	conn  *websocket.Conn
	hello helloMessage
	key   *ecdsa.PublicKey
	done  chan struct{}
}

func newFakeAPI(t *testing.T) *fakeAPI {
	api := &fakeAPI{t: t, sessions: make(chan *fakeSession, 8)}
	api.server = httptest.NewServer(http.HandlerFunc(api.serve))
	t.Cleanup(api.server.Close)
	return api
}

// token mints a distinct token per key so the server can find the proof key.
func (api *fakeAPI) token(_ context.Context, key *ecdsa.PublicKey) (string, error) {
	payload, _ := json.Marshal(map[string]any{"exp": time.Now().Add(time.Hour).Unix(), "serial": api.minted.Add(1)})
	token := "header." + base64.RawURLEncoding.EncodeToString(payload) + ".signature"
	api.keys.Store(token, key)
	return token, nil
}

func (api *fakeAPI) serve(w http.ResponseWriter, r *http.Request) {
	if r.URL.Path != connectPath {
		http.NotFound(w, r)
		return
	}
	if r.UserAgent() != userAgent("0.1.18") {
		http.Error(w, "unexpected user agent", http.StatusForbidden)
		return
	}
	conn, err := websocket.Accept(w, r, nil)
	if err != nil {
		return
	}
	defer conn.CloseNow()
	ctx := r.Context()
	var hello helloMessage
	if readJSON(ctx, conn, &hello) != nil || hello.Type != typeHello {
		return
	}
	key, ok := api.keys.Load(hello.Token)
	if !ok {
		_ = writeJSON(ctx, conn, map[string]string{"type": typeError, "code": "unauthorized"})
		return
	}
	nonceBytes := make([]byte, 32)
	_, _ = rand.Read(nonceBytes)
	nonce := base64.RawURLEncoding.EncodeToString(nonceBytes)
	// An unknown type during the handshake must be ignored.
	_ = writeJSON(ctx, conn, map[string]string{"type": "future"})
	_ = writeJSON(ctx, conn, map[string]string{"type": typeChallenge, "nonce": nonce})
	var proof proofMessage
	if readJSON(ctx, conn, &proof) != nil || !verify(key.(*ecdsa.PublicKey), nonce, proof.Signature) {
		_ = writeJSON(ctx, conn, map[string]string{"type": typeError, "code": "unauthorized"})
		conn.Close(4401, "")
		return
	}
	_ = writeJSON(ctx, conn, struct {
		Type           string   `json:"type"`
		XUID           string   `json:"xuid"`
		OwnedCosmetics []string `json:"owned_cosmetics,omitempty"`
		parameters
	}{Type: typeWelcome, XUID: "2535400000000001", OwnedCosmetics: api.ownedCosmetics, parameters: api.welcome})
	session := &fakeSession{conn: conn, hello: hello, key: key.(*ecdsa.PublicKey), done: make(chan struct{})}
	api.sessions <- session
	select {
	case <-session.done:
	case <-ctx.Done():
	}
}

func (s *fakeSession) send(t *testing.T, value any) {
	t.Helper()
	if err := writeJSON(context.Background(), s.conn, value); err != nil {
		t.Fatal(err)
	}
}

func (s *fakeSession) receive(t *testing.T, value any) {
	t.Helper()
	ctx, cancel := context.WithTimeout(context.Background(), 5*time.Second)
	defer cancel()
	if err := readJSON(ctx, s.conn, value); err != nil {
		t.Fatal(err)
	}
}

func readJSON(ctx context.Context, conn *websocket.Conn, value any) error {
	_, data, err := conn.Read(ctx)
	if err != nil {
		return err
	}
	return json.Unmarshal(data, value)
}

// verify checks a raw r||s ES384 signature over the contract's proof bytes.
func verify(key *ecdsa.PublicKey, nonce, signature string) bool {
	raw, err := base64.RawURLEncoding.DecodeString(signature)
	if err != nil || len(raw) != 96 {
		return false
	}
	digest := sha512.Sum384([]byte(proofPrefix + nonce))
	return ecdsa.Verify(key, digest[:], new(big.Int).SetBytes(raw[:48]), new(big.Int).SetBytes(raw[48:]))
}

func receive[T any](t *testing.T, values <-chan T) T {
	t.Helper()
	select {
	case value := <-values:
		return value
	case <-time.After(5 * time.Second):
		t.Fatal("Cinnabar API client did not finish its operation")
		var zero T
		return zero
	}
}

func newTestClient(t *testing.T, api *fakeAPI, onRelease func(Release)) *Client {
	client := New(context.Background(), Options{
		Base: api.server.URL, Token: api.token, OnRelease: onRelease,
		delay:  func(int) time.Duration { return time.Millisecond },
		random: func(int64) int64 { return 0 },
	})
	t.Cleanup(client.Close)
	return client
}

var testConfig = Config{ClientVersion: "0.1.18", Channel: "beta", Visible: true}

func TestNothingConnectsBeforeConfiguration(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	if _, err := client.Lookup(context.Background(), []string{"1"}, nil); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("lookup before configuration: %v", err)
	}
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	if session.hello != (helloMessage{Type: typeHello, Protocol: 1, ClientVersion: "0.1.18", Channel: "beta", Visible: true, Token: session.hello.Token}) {
		t.Fatalf("hello = %+v", session.hello)
	}
}

func TestOfflineClientNeverConnects(t *testing.T) {
	client := New(context.Background(), Options{Base: "http://127.0.0.1:1"})
	client.Configure(testConfig)
	if _, err := client.Lookup(context.Background(), []string{"1"}, nil); !errors.Is(err, ErrUnavailable) {
		t.Fatalf("offline lookup: %v", err)
	}
	client.Close()
}

func TestLookupReturnsTheServersSubset(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	testwait.Eventually(t, testwait.DefaultTimeout, "an authenticated session", func() bool {
		client.mu.Lock()
		defer client.mu.Unlock()
		return client.live != nil
	})
	results := make(chan []string, 1)
	errs := make(chan error, 2)
	go func() {
		found, err := client.Lookup(context.Background(), []string{"11", "22", "33"}, nil)
		errs <- err
		results <- found.Cinnabar
	}()
	var lookup lookupMessage
	session.receive(t, &lookup)
	if lookup.Type != typeLookup || len(lookup.XUIDs) != 3 {
		t.Fatalf("lookup = %+v", lookup)
	}
	session.send(t, map[string]any{"type": typeLookupResult, "id": lookup.ID, "cinnabar": []string{"22"}})
	if err := receive(t, errs); err != nil {
		t.Fatal(err)
	}
	if found := receive(t, results); len(found) != 1 || found[0] != "22" {
		t.Fatalf("found = %v", found)
	}
	go func() {
		_, err := client.Lookup(context.Background(), []string{"44"}, nil)
		errs <- err
	}()
	session.receive(t, &lookup)
	session.send(t, map[string]any{"type": typeError, "code": codeRateLimited, "id": lookup.ID})
	if err := receive(t, errs); !errors.Is(err, ErrRateLimited) {
		t.Fatalf("rate-limited lookup: %v", err)
	}
}

// UUIDs travel in their own array and their matches come back separately from XUID matches.
func TestLookupRoundTripsUUIDs(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	testwait.Eventually(t, testwait.DefaultTimeout, "an authenticated session", func() bool {
		client.mu.Lock()
		defer client.mu.Unlock()
		return client.live != nil
	})
	uuid := "00112233-4455-6677-8899-aabbccddeeff"
	results := make(chan LookupResult, 1)
	errs := make(chan error, 1)
	go func() {
		found, err := client.Lookup(context.Background(), []string{"11"}, []string{uuid})
		errs <- err
		results <- found
	}()
	var lookup lookupMessage
	session.receive(t, &lookup)
	if len(lookup.XUIDs) != 1 || len(lookup.UUIDs) != 1 || lookup.UUIDs[0] != uuid {
		t.Fatalf("lookup = %+v", lookup)
	}
	session.send(t, map[string]any{"type": typeLookupResult, "id": lookup.ID, "cinnabar": []string{}, "cinnabar_uuids": []string{uuid}})
	if err := receive(t, errs); err != nil {
		t.Fatal(err)
	}
	if found := receive(t, results); len(found.Cinnabar) != 0 || len(found.CinnabarUUIDs) != 1 || found.CinnabarUUIDs[0] != uuid {
		t.Fatalf("found = %+v", found)
	}
}

func TestLookupRejectsBatchesTheAPIWouldRefuse(t *testing.T) {
	client := New(context.Background(), Options{})
	defer client.Close()
	tooMany := make([]string, bridgecontract.CinnabarAPIMaxLookupXUIDs+1)
	for i := range tooMany {
		tooMany[i] = "1"
	}
	for _, xuids := range [][]string{tooMany, {""}, {"12a"}, {"123456789012345678901"}} {
		if _, err := client.Lookup(context.Background(), xuids, nil); !errors.Is(err, ErrInvalidLookup) {
			t.Fatalf("%d XUIDs: %v", len(xuids), err)
		}
	}
	// The bound is combined, UUIDs must be canonical lowercase, and an empty lookup is refused.
	for _, uuids := range [][]string{{"00112233-4455-6677-8899-aabbccddeeff"}, {"00112233-4455-6677-8899-AABBCCDDEEFF"}, {"00112233445566778899aabbccddeeff"}} {
		if _, err := client.Lookup(context.Background(), tooMany[1:], uuids); !errors.Is(err, ErrInvalidLookup) {
			t.Fatalf("UUIDs %v: %v", uuids, err)
		}
	}
	if _, err := client.Lookup(context.Background(), nil, nil); !errors.Is(err, ErrInvalidLookup) {
		t.Fatalf("empty lookup: %v", err)
	}
}

func TestReleasesOnlyForTheConfiguredChannel(t *testing.T) {
	api := newFakeAPI(t)
	releases := make(chan Release, 4)
	client := newTestClient(t, api, func(release Release) { releases <- release })
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	session.send(t, map[string]string{"type": typeRelease, "channel": "nightly", "version": "0.1.19", "published_at": "2026-10-13T00:00:00.000Z"})
	session.send(t, map[string]string{"type": typeRelease, "channel": "beta", "version": "0.1.19", "published_at": "2026-10-13T00:00:00.000Z"})
	if got := receive(t, releases); got != (Release{Channel: "beta", Version: "0.1.19", PublishedAt: "2026-10-13T00:00:00.000Z"}) {
		t.Fatalf("release = %+v", got)
	}
	// A rolling channel republishes one version; each build is still announced.
	session.send(t, map[string]any{"type": typeRelease, "channel": "beta", "version": "0.1.19", "build": 42, "published_at": "2026-10-14T00:00:00.000Z"})
	if got := receive(t, releases); got.PublishedAt != "2026-10-14T00:00:00.000Z" || got.Build != 42 {
		t.Fatalf("second build of one version = %+v", got)
	}
}

func TestVisibilityChangesAreSentLive(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	hidden := testConfig
	hidden.Visible = false
	client.Configure(hidden)
	var visibility visibilityMessage
	session.receive(t, &visibility)
	if visibility != (visibilityMessage{Type: typeVisibility, Visible: false}) {
		t.Fatalf("visibility = %+v", visibility)
	}
}

func TestChannelChangeReusesUnexpiredCredential(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	first := receive(t, api.sessions)
	nightly := testConfig
	nightly.Channel = "nightly"
	client.Configure(nightly)
	second := receive(t, api.sessions)
	if second.hello.Channel != "nightly" {
		t.Fatalf("reconnected hello = %+v", second.hello)
	}
	if !first.key.Equal(second.key) || first.hello.Token != second.hello.Token {
		t.Fatal("reconnect replaced an unexpired key or token")
	}
}

func TestReconnectsAfterTheServerDrops(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	first := receive(t, api.sessions)
	close(first.done)
	second := receive(t, api.sessions)
	if second.hello.Token != first.hello.Token {
		t.Fatal("reconnect replaced an unexpired token")
	}
}

func TestBackoffIsCappedAndJittered(t *testing.T) {
	lowest := func(int64) int64 { return 0 }
	highest := func(n int64) int64 { return n - 1 }
	for failures, want := range map[int][2]time.Duration{
		1:  {minBackoff / 2, minBackoff},
		3:  {2 * minBackoff, 4 * minBackoff},
		40: {maxBackoff / 2, maxBackoff},
	} {
		if got := backoff(failures, maxBackoff, lowest); got != want[0] {
			t.Fatalf("failures %d lowest = %v, want %v", failures, got, want[0])
		}
		if got := backoff(failures, maxBackoff, highest); got != want[1] {
			t.Fatalf("failures %d highest = %v, want %v", failures, got, want[1])
		}
	}
}

func TestSocketURLUsesTheConnectRoute(t *testing.T) {
	if got := socketURL("https://api.example/"); got != "wss://api.example/v1/connect" {
		t.Fatal(got)
	}
	if got := socketURL(bridgecontract.CinnabarAPIBaseURL); got[:6] != "wss://" {
		t.Fatal(got)
	}
}

// Frame rejections back off long; unauthorized and issuer outages keep the normal backoff.
func TestOnlyFrameRejectionsBackOffLong(t *testing.T) {
	for code, want := range map[websocket.StatusCode]bool{
		websocket.StatusInvalidFramePayloadData: true,
		websocket.StatusPolicyViolation:         true,
		websocket.StatusMessageTooBig:           true,
		4401:                                    false,
		websocket.StatusInternalError:           false,
	} {
		if got := rejectedFrames(fmt.Errorf("read: %w", websocket.CloseError{Code: code})); got != want {
			t.Fatalf("close %d: %v", code, got)
		}
	}
	if rejectedFrames(errors.New("dial failed")) {
		t.Fatal("a transport failure counted as a rejected frame")
	}
}

func TestConfigValidityMatchesTheHelloSchema(t *testing.T) {
	for config, want := range map[Config]bool{
		{ClientVersion: "0.1.18", Channel: "beta"}:                       true,
		{ClientVersion: "0.1.18", Channel: "beta", ClientBuild: 1 << 53}: false,
		{ClientVersion: "1.2.3-rc.1", Channel: "nightly"}:                true,
		{ClientVersion: "0.1.18+7", Channel: "beta"}:                     false,
		{ClientVersion: "0.1", Channel: "beta"}:                          false,
		{ClientVersion: "0.1.18", Channel: "canary"}:                     false,
		{ClientVersion: "1.2.3-" + strings.Repeat("a", 60)}:              false,
	} {
		if got := config.Valid(); got != want {
			t.Fatalf("%+v valid = %v", config, got)
		}
	}
}

// A refused upgrade must not make a request to the token issuer.
func TestDialFailureDoesNotMint(t *testing.T) {
	var minted atomic.Int64
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Retry-After", "90")
		w.WriteHeader(http.StatusTooManyRequests)
	}))
	defer server.Close()
	client := New(context.Background(), Options{Base: server.URL, Token: func(context.Context, *ecdsa.PublicKey) (string, error) {
		minted.Add(1)
		return "unused", nil
	}})
	defer client.Close()
	_, _, err := client.connect(context.Background(), testConfig)
	if err == nil || minted.Load() != 0 {
		t.Fatalf("dial: %v, minted %d", err, minted.Load())
	}
}

// Every correlated error must complete the lookup, including future error codes.
func TestUnknownLookupErrorCompletesPendingRequest(t *testing.T) {
	api := newFakeAPI(t)
	client := newTestClient(t, api, nil)
	client.Configure(testConfig)
	session := receive(t, api.sessions)
	testwait.Eventually(t, testwait.DefaultTimeout, "an authenticated session", func() bool {
		client.mu.Lock()
		defer client.mu.Unlock()
		return client.live != nil
	})
	done := make(chan error, 1)
	go func() { _, err := client.Lookup(context.Background(), []string{"11"}, nil); done <- err }()
	var lookup lookupMessage
	session.receive(t, &lookup)
	session.send(t, map[string]any{"type": typeError, "code": "future_error", "id": lookup.ID})
	if err := receive(t, done); err == nil {
		t.Fatal("lookup error was lost")
	}
}
