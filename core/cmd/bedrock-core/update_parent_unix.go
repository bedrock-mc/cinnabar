//go:build !windows

package main

import (
	"errors"
	"syscall"
)

// parentAlive probes process existence without sending a signal to the client.
func parentAlive(pid int) (bool, error) {
	err := syscall.Kill(pid, 0)
	if errors.Is(err, syscall.ESRCH) {
		return false, nil
	}
	if errors.Is(err, syscall.EPERM) {
		return true, nil
	}
	return err == nil, err
}
