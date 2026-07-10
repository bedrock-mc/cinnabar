// Package lockfile provides process-wide exclusive leases backed by stable OS lock files.
package lockfile

import (
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
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return nil, fmt.Errorf("lockfile: create parent directory: %w", err)
	}
	deadline := time.Now().Add(timeout)
	for {
		lease, busy, err := tryAcquire(path)
		if err != nil {
			return nil, err
		}
		if !busy {
			return lease, nil
		}
		if timeout <= 0 || !time.Now().Before(deadline) {
			return nil, fmt.Errorf("%w: %s", ErrBusy, path)
		}
		delay := 20 * time.Millisecond
		if remaining := time.Until(deadline); remaining < delay {
			delay = remaining
		}
		if delay > 0 {
			time.Sleep(delay)
		}
	}
}
