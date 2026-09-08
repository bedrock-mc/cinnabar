//go:build !windows

package lockfile

import (
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"sync"
	"syscall"
)

type lease struct {
	file *os.File
	once sync.Once
	err  error
}

func (locked *lease) Identity() (fs.FileInfo, error) { return locked.file.Stat() }

func tryAcquire(path string, create bool) (io.Closer, bool, error) {
	flags := os.O_RDWR
	if create {
		flags |= os.O_CREATE
	}
	file, err := os.OpenFile(path, flags, 0o600)
	if err != nil {
		return nil, false, fmt.Errorf("lockfile: open %s: %w", path, err)
	}
	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		_ = file.Close()
		if errors.Is(err, syscall.EWOULDBLOCK) || errors.Is(err, syscall.EAGAIN) {
			return nil, true, nil
		}
		return nil, false, fmt.Errorf("lockfile: lock %s: %w", path, err)
	}
	return &lease{file: file}, false, nil
}

func (locked *lease) Close() error {
	locked.once.Do(func() {
		unlockErr := syscall.Flock(int(locked.file.Fd()), syscall.LOCK_UN)
		locked.err = errors.Join(unlockErr, locked.file.Close())
	})
	return locked.err
}
