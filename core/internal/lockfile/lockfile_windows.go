//go:build windows

package lockfile

import (
	"errors"
	"io"
	"os"
	"sync"

	"golang.org/x/sys/windows"
)

type lease struct {
	file       *os.File
	overlapped windows.Overlapped
	once       sync.Once
	err        error
}

func tryAcquire(path string) (io.Closer, bool, error) {
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, false, fmtLockError(path, err)
	}
	locked := &lease{file: file}
	err = windows.LockFileEx(
		windows.Handle(file.Fd()),
		windows.LOCKFILE_EXCLUSIVE_LOCK|windows.LOCKFILE_FAIL_IMMEDIATELY,
		0,
		1,
		0,
		&locked.overlapped,
	)
	if errors.Is(err, windows.ERROR_LOCK_VIOLATION) {
		_ = file.Close()
		return nil, true, nil
	}
	if err != nil {
		_ = file.Close()
		return nil, false, fmtLockError(path, err)
	}
	return locked, false, nil
}

func (locked *lease) Close() error {
	locked.once.Do(func() {
		unlockErr := windows.UnlockFileEx(windows.Handle(locked.file.Fd()), 0, 1, 0, &locked.overlapped)
		locked.err = errors.Join(unlockErr, locked.file.Close())
	})
	return locked.err
}

func fmtLockError(path string, err error) error {
	return &lockError{path: path, err: err}
}

type lockError struct {
	path string
	err  error
}

func (err *lockError) Error() string { return "lockfile: lock " + err.path + ": " + err.err.Error() }
func (err *lockError) Unwrap() error { return err.err }
