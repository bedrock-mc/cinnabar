//go:build windows

package proxy

import (
	"errors"
	"io"
	"os"
	"sync"

	"golang.org/x/sys/windows"
)

type windowsRuntimeLease struct {
	file       *os.File
	overlapped windows.Overlapped
	once       sync.Once
	err        error
}

func tryRuntimeLease(path string) (io.Closer, bool, error) {
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, false, err
	}
	lease := &windowsRuntimeLease{file: file}
	err = windows.LockFileEx(
		windows.Handle(file.Fd()),
		windows.LOCKFILE_EXCLUSIVE_LOCK|windows.LOCKFILE_FAIL_IMMEDIATELY,
		0,
		1,
		0,
		&lease.overlapped,
	)
	if errors.Is(err, windows.ERROR_LOCK_VIOLATION) {
		_ = file.Close()
		return nil, true, nil
	}
	if err != nil {
		_ = file.Close()
		return nil, false, err
	}
	return lease, false, nil
}

func (lease *windowsRuntimeLease) Close() error {
	lease.once.Do(func() {
		unlockErr := windows.UnlockFileEx(windows.Handle(lease.file.Fd()), 0, 1, 0, &lease.overlapped)
		lease.err = errors.Join(unlockErr, lease.file.Close())
	})
	return lease.err
}
