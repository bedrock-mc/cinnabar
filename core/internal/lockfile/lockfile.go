// Package lockfile provides process-wide exclusive leases backed by stable OS lock files.
package lockfile

import (
	"context"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"
)

// ErrBusy reports that another process currently holds a lease.
var ErrBusy = errors.New("lockfile: lease is already held")

// Acquire exclusively leases path. The lock file remains in place after release.
// A non-positive timeout makes one non-blocking acquisition attempt.
func Acquire(path string, timeout time.Duration) (io.Closer, error) {
	if timeout <= 0 {
		return acquire(context.Background(), path, false)
	}
	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	lease, err := AcquireContext(ctx, path)
	if errors.Is(err, context.DeadlineExceeded) {
		return nil, fmt.Errorf("%w: %s", ErrBusy, path)
	}
	return lease, err
}

// AcquireContext waits for an exclusive lease until ctx is cancelled. It never
// replaces or removes the stable lock file, including after the holder exits.
func AcquireContext(ctx context.Context, path string) (io.Closer, error) {
	return acquire(ctx, path, true)
}

// acquire opens the stable lock file and optionally retries contention.
func acquire(ctx context.Context, path string, wait bool) (io.Closer, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return nil, fmt.Errorf("lockfile: create parent directory: %w", err)
	}
	for {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		lease, busy, err := tryAcquire(path)
		if err != nil || !busy {
			return lease, err
		}
		if !wait {
			return nil, fmt.Errorf("%w: %s", ErrBusy, path)
		}
		retry := time.NewTimer(20 * time.Millisecond)
		select {
		case <-ctx.Done():
			retry.Stop()
			return nil, ctx.Err()
		case <-retry.C:
		}
	}
}
