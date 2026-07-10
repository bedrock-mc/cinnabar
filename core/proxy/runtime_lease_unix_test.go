//go:build !windows

package proxy

import (
	"errors"
	"io"
	"os"
	"sync"
	"syscall"
)

type unixRuntimeLease struct {
	file *os.File
	once sync.Once
	err  error
}

func tryRuntimeLease(path string) (io.Closer, bool, error) {
	file, err := os.OpenFile(path, os.O_CREATE|os.O_RDWR, 0o600)
	if err != nil {
		return nil, false, err
	}
	if err := syscall.Flock(int(file.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		_ = file.Close()
		if errors.Is(err, syscall.EWOULDBLOCK) || errors.Is(err, syscall.EAGAIN) {
			return nil, true, nil
		}
		return nil, false, err
	}
	return &unixRuntimeLease{file: file}, false, nil
}

func (lease *unixRuntimeLease) Close() error {
	lease.once.Do(func() {
		unlockErr := syscall.Flock(int(lease.file.Fd()), syscall.LOCK_UN)
		lease.err = errors.Join(unlockErr, lease.file.Close())
	})
	return lease.err
}
