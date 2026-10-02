package authcache

import (
	"context"
	"crypto/ecdsa"
	"errors"
	"io"
	"path/filepath"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
	"golang.org/x/oauth2"
)

// leaseDeviceSource takes the OAuth lease after the account's initial token check,
// immediately before SISU asks its context-free OAuth source for a token.
type leaseDeviceSource struct {
	path     string
	acquired chan io.Closer
	calls    atomic.Int32
}

// DeviceToken returns a valid test token after another process takes the lease.
func (d *leaseDeviceSource) DeviceToken(context.Context) (*xasd.Token, error) {
	if d.calls.Add(1) == 1 {
		lease, err := lockfile.Acquire(d.path+cacheLockSuffix, 0)
		if err != nil {
			return nil, err
		}
		d.acquired <- lease
	}
	return &xasd.Token{Token: "device", NotAfter: time.Now().Add(time.Hour)}, nil
}

// ProofKey stops the test flow before HTTP once the OAuth callback succeeds.
func (d *leaseDeviceSource) ProofKey() *ecdsa.PrivateKey { return nil }

// accountWithContendedSISU installs a real cached OAuth source and observable SISU flow.
func accountWithContendedSISU(t *testing.T) (*Account, <-chan io.Closer) {
	t.Helper()
	path := filepath.Join(t.TempDir(), "oauth.json")
	oauth, err := Source(context.Background(), Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
		return testOAuthToken("account"), nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	account := newAccount(context.Background(), "", oauth, nil, derivedDeps{})
	t.Cleanup(func() { _ = account.Close() })
	device := &leaseDeviceSource{path: path, acquired: make(chan io.Closer, 1)}
	account.device = device
	account.session = newAccountSession(account, &sisu.SessionConfig{DeviceTokenSource: device})
	return account, device.acquired
}

func TestNestedOAuthLeaseRespectsCallerDeadline(t *testing.T) {
	for _, retained := range []bool{false, true} {
		name := "account"
		if retained {
			name = "retained PlayFab session"
		}
		t.Run(name, func(t *testing.T) {
			account, acquired := accountWithContendedSISU(t)
			call := account.XSTSToken
			if retained {
				// PlayFab retains this source and calls it without the account gate.
				call = account.session.XSTSToken
			}
			ctx, cancel := context.WithTimeout(context.Background(), 200*time.Millisecond)
			defer cancel()
			done := make(chan error, 1)
			go func() { _, err := call(ctx, cachedRelyingParty); done <- err }()
			select {
			case lease := <-acquired:
				defer lease.Close()
			case <-time.After(time.Second):
				t.Fatal("SISU never reached the nested OAuth callback")
			}
			select {
			case err := <-done:
				if !errors.Is(err, context.DeadlineExceeded) {
					t.Fatalf("nested OAuth wait = %v, want caller deadline", err)
				}
			case <-time.After(time.Second):
				t.Fatal("nested OAuth lease wait outlived the caller deadline")
			}
		})
	}
}

func TestSISUSessionKeepsConcurrentCallContextsSeparate(t *testing.T) {
	for _, accountCall := range []bool{false, true} {
		name := "same session"
		if accountCall {
			name = "account OAuth source"
		}
		t.Run(name, func(t *testing.T) {
			account, acquired := accountWithContendedSISU(t)
			session := account.session
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			first := make(chan error, 1)
			go func() { _, err := session.XSTSToken(ctx, cachedRelyingParty); first <- err }()
			var lease io.Closer
			select {
			case lease = <-acquired:
				defer lease.Close()
			case <-time.After(time.Second):
				t.Fatal("SISU never reached the nested OAuth callback")
			}

			waitForOAuthLeaseWait(t, account)
			call := session.XSTSToken
			if accountCall {
				call = account.XSTSToken
			}
			short, stop := context.WithTimeout(context.Background(), 50*time.Millisecond)
			defer stop()
			second := make(chan error, 1)
			go func() { _, err := call(short, cachedRelyingParty); second <- err }()
			select {
			case err := <-second:
				if !errors.Is(err, context.DeadlineExceeded) {
					t.Fatalf("concurrent SISU wait = %v, want its own deadline", err)
				}
			case <-time.After(time.Second):
				t.Fatal("concurrent SISU call waited beyond its deadline")
			}
			select {
			case err := <-first:
				t.Fatalf("shorter request canceled the active request: %v", err)
			default:
			}
			cancel()
			select {
			case err := <-first:
				if !errors.Is(err, context.Canceled) {
					t.Fatalf("active SISU wait = %v, want its own cancellation", err)
				}
			case <-time.After(time.Second):
				t.Fatal("active SISU call ignored cancellation")
			}
			if err := lease.Close(); err != nil {
				t.Fatal(err)
			}
			// A later request must reach the next SISU step, with neither canceled context retained.
			if _, err := session.XSTSToken(context.Background(), cachedRelyingParty); err == nil || !strings.Contains(err.Error(), "proof key is absent") {
				t.Fatalf("subsequent SISU call = %v, want OAuth success before missing test proof key", err)
			}
		})
	}
}

// waitForOAuthLeaseWait confirms the background call owns the source gate before
// another caller starts, so the test exercises local serialization deterministically.
func waitForOAuthLeaseWait(t *testing.T, account *Account) {
	t.Helper()
	source := account.oauth.(*persistingSource)
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		if len(source.gate) != 0 {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("OAuth callback never acquired the source gate")
}
