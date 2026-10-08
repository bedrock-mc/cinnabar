package proxy

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http/httptest"
	"os"
	"path/filepath"
	"slices"
	"sync"
	"testing"
	"time"

	"github.com/df-mc/go-nethernet"
	"github.com/df-mc/go-nethernet/endpoint"
	"github.com/go-jose/go-jose/v4"
	"github.com/go-jose/go-jose/v4/jwt"
	"github.com/sandertv/gophertunnel/minecraft"
)

func TestServerTrustFileRoundTripsKeys(t *testing.T) {
	path := ServerTrustFile(filepath.Join(t.TempDir(), "nested", "trusted_server_public_keys.json"))
	if keys, err := path.LoadTrustedKeys(); err != nil || len(keys) != 0 {
		t.Fatalf("missing file = %q, %v; want no keys", keys, err)
	}
	if err := path.SaveTrustedKeys([]string{"a", "b"}); err != nil {
		t.Fatal(err)
	}
	keys, err := path.LoadTrustedKeys()
	if err != nil || !slices.Equal(keys, []string{"a", "b"}) {
		t.Fatalf("keys = %q, %v; want [a b]", keys, err)
	}
	data, _ := os.ReadFile(string(path))
	if string(data) != `{"keys":["a","b"]}` {
		t.Fatalf("file = %s, want vanilla's {\"keys\":[...]} shape", data)
	}
}

type promptLog struct {
	mu      sync.Mutex
	pending map[uint64]ServerTrustPrompt
	shown   chan ServerTrustPrompt
}

func newPromptLog() *promptLog {
	return &promptLog{pending: map[uint64]ServerTrustPrompt{}, shown: make(chan ServerTrustPrompt, 8)}
}

func (log *promptLog) publish(prompt ServerTrustPrompt, pending bool) {
	log.mu.Lock()
	defer log.mu.Unlock()
	if pending {
		log.pending[prompt.ID] = prompt
		log.shown <- prompt
	} else {
		delete(log.pending, prompt.ID)
	}
}

func (log *promptLog) count() int {
	log.mu.Lock()
	defer log.mu.Unlock()
	return len(log.pending)
}

func TestServerTrustPromptsDeliverTheAnswerAndWithdraw(t *testing.T) {
	log := newPromptLog()
	prompts := NewServerTrustPrompts(log.publish)
	result := make(chan bool, 1)
	go func() {
		trusted, _ := prompts.Confirm(t.Context(), "http://127.0.0.1:19132")
		result <- trusted
	}()
	prompt := <-log.shown
	if prompt.URL != "http://127.0.0.1:19132" {
		t.Fatalf("prompt URL = %q", prompt.URL)
	}
	if !prompts.Answer(prompt.ID, true) || !<-result {
		t.Fatal("the answer did not reach Confirm")
	}
	if prompts.Answer(prompt.ID, false) {
		t.Fatal("an answered prompt accepted a second answer")
	}
	if log.count() != 0 {
		t.Fatal("an answered prompt stayed published")
	}
}

func TestServerTrustPromptsEndWithTheirJoin(t *testing.T) {
	log := newPromptLog()
	prompts := NewServerTrustPrompts(log.publish)
	ctx, cancel := context.WithCancel(t.Context())
	done := make(chan error, 1)
	go func() {
		_, err := prompts.Confirm(ctx, "http://example.com:19132")
		done <- err
	}()
	<-log.shown
	cancel()
	if err := <-done; !errors.Is(err, context.Canceled) {
		t.Fatalf("Confirm error = %v, want the join's cancellation", err)
	}
	if log.count() != 0 {
		t.Fatal("a cancelled prompt stayed published")
	}
}

// trustedListenerAddress serves a NetherNet listener over plain HTTP signaling, as BDS does.
func trustedListenerAddress(t *testing.T) string {
	t.Helper()
	signaling := endpoint.HandlerConfig{Logger: slog.New(slog.DiscardHandler)}.New()
	t.Cleanup(func() { _ = signaling.Close() })
	listener, err := nethernet.ListenConfig{Log: slog.New(slog.DiscardHandler), DisableTrickleICE: true, AllowAnonymous: true}.Listen(signaling)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = listener.Close() })
	go func() {
		for {
			conn, err := listener.Accept()
			if err != nil {
				return
			}
			t.Cleanup(func() { _ = conn.Close() })
		}
	}()
	server := httptest.NewServer(signaling)
	t.Cleanup(server.Close)
	return server.Listener.Addr().String()
}

// An http server prompts on its first join; trusting it persists the key and the next join is
// silent, while declining fails the join.
func TestAddressedHTTPServerTrustPromptsOnceAndPersists(t *testing.T) {
	address := trustedListenerAddress(t)
	file := ServerTrustFile(filepath.Join(t.TempDir(), "trusted_server_public_keys.json"))
	log := newPromptLog()
	prompts := NewServerTrustPrompts(log.publish)
	trust := &minecraft.FirstUseTrust{Store: file, Confirm: prompts.Confirm, Log: slog.New(slog.DiscardHandler)}
	join := func() error {
		target, err := resolveUpstreamTarget(t.Context(), address, nil, slog.New(slog.DiscardHandler), trust)
		if err != nil {
			return err
		}
		ctx, cancel := context.WithTimeout(t.Context(), 15*time.Second)
		defer cancel()
		conn, err := target.network.DialContext(ctx, target.address)
		if err == nil {
			_ = conn.Close()
		}
		return err
	}
	answer := func(trusted bool) {
		go func() { prompt := <-log.shown; prompts.Answer(prompt.ID, trusted) }()
	}

	answer(false)
	if err := join(); !errors.Is(err, minecraft.ErrServerNotTrusted) {
		t.Fatalf("declined join error = %v, want ErrServerNotTrusted", err)
	}
	answer(true)
	if err := join(); err != nil {
		t.Fatalf("trusted join: %v", err)
	}
	if keys, _ := file.LoadTrustedKeys(); len(keys) != 1 {
		t.Fatalf("persisted %d keys, want the trusted one", len(keys))
	}
	if err := join(); err != nil {
		t.Fatalf("second join: %v", err)
	}
	select {
	case prompt := <-log.shown:
		t.Fatalf("second join prompted again for %q", prompt.URL)
	default:
	}
}

// A declined server ends its join without taking the core down.
func TestDeclinedServerTrustIsNotProcessFatal(t *testing.T) {
	declined := fmt.Errorf("dial NetherNet: %w", minecraft.ErrServerNotTrusted)
	if shouldSurfacePreparationError(declined, context.Background()) {
		t.Fatal("a declined server stopped the core")
	}
	if !shouldSurfacePreparationError(errors.New("dial failed"), context.Background()) {
		t.Fatal("ordinary preparation failures no longer surface")
	}
}

// A signed-out join's identity is presented again by the redial after a slow trust answer, so it
// must outlive the question.
func TestSignedOutIdentityOutlivesTheTrustQuestion(t *testing.T) {
	now := time.Now()
	identity, err := selfSignedIdentity(now)
	if err != nil {
		t.Fatal(err)
	}
	parsed, err := jwt.ParseSigned(identity.Token, []jose.SignatureAlgorithm{jose.ES384})
	if err != nil {
		t.Fatal(err)
	}
	var claims jwt.Claims
	if err := parsed.UnsafeClaimsWithoutVerification(&claims); err != nil {
		t.Fatal(err)
	}
	if claims.Expiry.Time().Sub(now) < 10*time.Minute {
		t.Fatalf("identity expires after %v, sooner than a join may wait on the trust question", claims.Expiry.Time().Sub(now))
	}
}

// A transfer hop asks about the server whatever kind of target it left, local worlds included.
func TestTransferHopsCarryTheConfiguredTrust(t *testing.T) {
	trust := &minecraft.FirstUseTrust{}
	local := &resolvedUpstreamTarget{address: "http://127.0.0.1:19132", network: localNetherNetNetwork{}}
	hop, ok := networkForAddress(local, "play.example:19132", trust).(addressedServerNetwork)
	if !ok || hop.ServerTrust != trust {
		t.Fatalf("hop network = %#v, want the configured trust", hop)
	}
}

// Publications reach the client in prompt order, so an older join's prompt cannot replace a newer one.
func TestServerTrustPromptsPublishInOrder(t *testing.T) {
	var mu sync.Mutex
	var order []uint64
	prompts := NewServerTrustPrompts(func(prompt ServerTrustPrompt, pending bool) {
		if pending {
			mu.Lock()
			order = append(order, prompt.ID)
			mu.Unlock()
		}
	})
	ctx, cancel := context.WithCancel(t.Context())
	var wg sync.WaitGroup
	for range 20 {
		wg.Add(1)
		go func() { defer wg.Done(); _, _ = prompts.Confirm(ctx, "http://a:1") }()
	}
	for {
		mu.Lock()
		n := len(order)
		mu.Unlock()
		if n == 20 {
			break
		}
		time.Sleep(time.Millisecond)
	}
	cancel()
	wg.Wait()
	if !slices.IsSorted(order) {
		t.Fatalf("prompts published out of order: %v", order)
	}
}
