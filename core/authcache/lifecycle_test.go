package authcache

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync/atomic"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
	"golang.org/x/oauth2"
)

// oauthSourceFunc supplies observable credentials for lifecycle tests.
type oauthSourceFunc func() (*oauth2.Token, error)

// Token delegates the synthetic OAuth response to the test.
func (f oauthSourceFunc) Token() (*oauth2.Token, error) { return f() }

func TestClosedAccountRefusesEveryCredential(t *testing.T) {
	path := filepath.Join(t.TempDir(), "derived")
	oauthToken := testOAuthToken("closed-account")
	writeDerivedState(t, path, oauthToken, time.Now().Add(time.Hour))
	var calls atomic.Int32
	source := newAccount(context.Background(), path, oauthSourceFunc(func() (*oauth2.Token, error) {
		calls.Add(1)
		return oauthToken, nil
	}), nil, derivedDeps{})
	if err := source.Close(); err != nil {
		t.Fatal(err)
	}
	calls.Store(0)
	key, err := ecdsa.GenerateKey(elliptic.P256(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	ctx := context.Background()
	checks := map[string]func() error{
		"OAuth":       func() error { _, err := source.Token(); return err },
		"device":      func() error { _, err := source.DeviceToken(ctx); return err },
		"XSTS":        func() error { _, err := source.XSTSToken(ctx, cachedRelyingParty); return err },
		"service":     func() error { _, err := source.ServiceToken(ctx); return err },
		"environment": func() error { _, err := source.Environment(ctx); return err },
		"PlayFab":     func() error { _, err := source.PlayFab(ctx); return err },
		"multiplayer": func() error { _, err := source.MultiplayerToken(ctx, &key.PublicKey); return err },
	}
	for name, call := range checks {
		t.Run(name, func(t *testing.T) {
			if err := call(); !errors.Is(err, ErrAccountClosed) {
				t.Fatalf("credential error = %v, want ErrAccountClosed", err)
			}
		})
	}
	if source.ProofKey() != nil {
		t.Fatal("closed account exposed its proof key")
	}
	if calls.Load() != 0 {
		t.Fatal("closed account consulted its OAuth source")
	}
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	state, err := loadDerived(path)
	if err != nil {
		t.Fatal(err)
	}
	source.InvalidateXSTSToken(cachedRelyingParty, state.SISU.XSTSTokens[cachedRelyingParty])
	source.InvalidateServiceToken(state.ServiceToken)
	after, err := os.ReadFile(path)
	if err != nil || string(before) != string(after) {
		t.Fatalf("closed account rewrote its cache: %v", err)
	}
}

func TestAccountCloseWaitsForUncancellableOAuthRead(t *testing.T) {
	started, release := make(chan struct{}), make(chan struct{})
	var calls atomic.Int32
	source := newAccount(context.Background(), "", oauthSourceFunc(func() (*oauth2.Token, error) {
		if calls.Add(1) == 2 {
			close(started)
			<-release
		}
		return testOAuthToken("account"), nil
	}), nil, derivedDeps{})
	tokenDone := make(chan error, 1)
	go func() {
		_, err := source.Token()
		tokenDone <- err
	}()
	<-started
	closed := make(chan error, 1)
	go func() { closed <- source.Close() }()
	select {
	case err := <-closed:
		close(release)
		t.Fatalf("Close completed while OAuth was still active: %v", err)
	case <-time.After(50 * time.Millisecond):
	}
	close(release)
	if err := <-tokenDone; !errors.Is(err, context.Canceled) {
		t.Fatalf("active OAuth read after Close began = %v, want cancellation", err)
	}
	if err := <-closed; err != nil {
		t.Fatal(err)
	}
	if _, err := source.Token(); !errors.Is(err, ErrAccountClosed) {
		t.Fatalf("Token after Close = %v", err)
	}
	if calls.Load() != 2 {
		t.Fatal("OAuth refreshed after Close returned")
	}
}

func TestAccountCloseInterruptsCredentialLeaseWait(t *testing.T) {
	for _, derived := range []bool{false, true} {
		name := "OAuth"
		if derived {
			name = "derived"
		}
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "oauth.json")
			token := testOAuthToken("account")
			oauth, err := Source(context.Background(), Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
				return token, nil
			}})
			if err != nil {
				t.Fatal(err)
			}
			account := newAccount(context.Background(), DerivedCachePath(path), oauth, nil, derivedDeps{})
			lockedPath := path
			if derived {
				lockedPath = DerivedCachePath(path)
			}
			lease, err := lockfile.Acquire(lockedPath+cacheLockSuffix, 0)
			if err != nil {
				t.Fatal(err)
			}
			defer lease.Close()
			operation := make(chan error, 1)
			go func() {
				var err error
				if derived {
					_, err = account.DeviceToken(context.Background())
				} else {
					_, err = account.Token()
				}
				operation <- err
			}()
			waitForAccountOperation(t, account)
			closed := make(chan error, 1)
			go func() { closed <- account.Close() }()
			select {
			case err := <-closed:
				if err != nil {
					t.Fatal(err)
				}
			case <-time.After(time.Second):
				t.Fatal("Close waited for another process's credential lease")
			}
			if err := <-operation; !errors.Is(err, context.Canceled) {
				t.Fatalf("credential wait = %v, want cancellation", err)
			}
			if _, err := account.Token(); !errors.Is(err, ErrAccountClosed) {
				t.Fatalf("credential after Close = %v", err)
			}
		})
	}
}

func TestAccountLifetimeCancellationInterruptsOAuthWait(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oauth.json")
	oauth, err := Source(context.Background(), Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
		return testOAuthToken("account"), nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	account := newAccount(ctx, "", oauth, nil, derivedDeps{})
	lease, err := lockfile.Acquire(path+cacheLockSuffix, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	done := make(chan error, 1)
	go func() { _, err := account.Token(); done <- err }()
	waitForAccountOperation(t, account)
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("Token = %v, want cancellation", err)
		}
	case <-time.After(time.Second):
		t.Fatal("Token ignored account lifetime cancellation")
	}
	if err := account.Close(); err != nil {
		t.Fatal(err)
	}
}

// waitForAccountOperation waits until the test call has begun on the account;
// the competing lease stays held until after shutdown has finished.
func waitForAccountOperation(t *testing.T, account *Account) {
	t.Helper()
	testwait.Eventually(t, time.Second, "a credential operation to begin", func() bool {
		account.activeMu.Lock()
		defer account.activeMu.Unlock()
		return account.active != 0
	})
}

func TestAccountQueuedCallRespectsDeadline(t *testing.T) {
	for _, name := range []string{"XSTS", "device", "service"} {
		t.Run(name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "oauth.json")
			oauth, err := Source(context.Background(), Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) { return testOAuthToken("account"), nil }})
			if err != nil {
				t.Fatal(err)
			}
			account := newAccount(context.Background(), "", oauth, nil, derivedDeps{})
			defer account.Close()
			lease, err := lockfile.Acquire(path+cacheLockSuffix, 0)
			if err != nil {
				t.Fatal(err)
			}
			defer lease.Close()
			first := make(chan error, 1)
			go func() { _, err := account.Token(); first <- err }()
			waitForAccountOperation(t, account)
			ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
			defer cancel()
			second := make(chan error, 1)
			go func() {
				var err error
				switch name {
				case "XSTS":
					_, err = account.XSTSToken(ctx, cachedRelyingParty)
				case "device":
					_, err = account.DeviceToken(ctx)
				case "service":
					_, err = account.ServiceToken(ctx)
				}
				second <- err
			}()
			select {
			case err := <-second:
				if !errors.Is(err, context.DeadlineExceeded) {
					t.Fatalf("error = %v", err)
				}
			case <-time.After(250 * time.Millisecond):
				t.Error("caller stayed blocked on the account gate for 250ms despite its 50ms deadline")
			}
			if err := lease.Close(); err != nil {
				t.Fatal(err)
			}
			if err := <-first; err != nil {
				t.Fatal(err)
			}
		})
	}
}
