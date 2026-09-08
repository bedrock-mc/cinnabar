// Package lockfile provides process-wide exclusive leases backed by stable OS lock files.
package lockfile

import (
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"time"
)

// ErrBusy reports that another process currently holds a lease.
var ErrBusy = errors.New("lockfile: lease is already held")

// Identity returns the opened file identity held by lease.
func Identity(lease io.Closer) (fs.FileInfo, error) {
	identified, ok := lease.(interface{ Identity() (fs.FileInfo, error) })
	if !ok {
		return nil, errors.New("lockfile: lease identity unavailable")
	}
	return identified.Identity()
}

// Acquire exclusively leases path. The lock file remains in place after release.
// A non-positive timeout makes one non-blocking acquisition attempt.
func Acquire(path string, timeout time.Duration) (io.Closer, error) {
	return acquire(path, timeout, true)
}

// AcquireExisting exclusively leases an existing path without creating it.
func AcquireExisting(path string, timeout time.Duration) (io.Closer, error) {
	return acquire(path, timeout, false)
}

func acquire(path string, timeout time.Duration, create bool) (io.Closer, error) {
	if create {
		if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
			return nil, fmt.Errorf("lockfile: create parent directory: %w", err)
		}
	}
	deadline := time.Now().Add(timeout)
	for {
		lease, busy, err := tryAcquire(path, create)
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
