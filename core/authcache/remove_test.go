package authcache

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
)

func TestRemoveWaitsForRefreshAndDeletesItsRotatedToken(t *testing.T) {
	path := seedRemovalCaches(t)
	lease, err := lockfile.Acquire(path+cacheLockSuffix, 0)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = lease.Close() })
	started := make(chan struct{})
	done := make(chan error, 1)
	ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
	defer cancel()
	go func() {
		close(started)
		done <- Remove(ctx, path, os.Remove)
	}()
	<-started
	select {
	case err := <-done:
		t.Fatalf("removal passed the active refresh lease: %v", err)
	case <-time.After(50 * time.Millisecond):
	}
	if err := os.WriteFile(path, []byte("rotated token"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	for _, cache := range []string{path, DerivedCachePath(path)} {
		if _, err := os.Stat(cache); !errors.Is(err, os.ErrNotExist) {
			t.Fatalf("cache survived sign-out: %v", err)
		}
		if _, err := os.Stat(cache + cacheLockSuffix); err != nil {
			t.Fatalf("stable lease was removed: %v", err)
		}
	}
}

func TestRemoveNeverDeletesWithoutBothLeases(t *testing.T) {
	for _, derived := range []bool{false, true} {
		path := seedRemovalCaches(t)
		lockPath := path
		if derived {
			lockPath = DerivedCachePath(path)
		}
		lease, err := lockfile.Acquire(lockPath+cacheLockSuffix, 0)
		if err != nil {
			t.Fatal(err)
		}
		ctx, cancel := context.WithTimeout(context.Background(), 30*time.Millisecond)
		err = Remove(ctx, path, func(string) error { t.Fatal("removed cache without both leases"); return nil })
		cancel()
		_ = lease.Close()
		if !errors.Is(err, context.DeadlineExceeded) {
			t.Fatalf("contended removal = %v", err)
		}
		for _, cache := range []string{path, DerivedCachePath(path)} {
			if _, err := os.Stat(cache); err != nil {
				t.Fatalf("cache removed after timeout: %v", err)
			}
		}
		ctx, cancel = context.WithCancel(context.Background())
		cancel()
		if err := Remove(ctx, path, os.Remove); !errors.Is(err, context.Canceled) {
			t.Fatalf("cancelled removal = %v", err)
		}
	}
}

// seedRemovalCaches writes synthetic files for the sign-out lease tests.
func seedRemovalCaches(t *testing.T) string {
	t.Helper()
	path := filepath.Join(t.TempDir(), "token.json")
	for _, cache := range []string{path, DerivedCachePath(path)} {
		if err := os.WriteFile(cache, []byte("synthetic token"), 0o600); err != nil {
			t.Fatal(err)
		}
	}
	return path
}
