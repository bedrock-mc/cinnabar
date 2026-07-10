//go:build !windows

package streamnet

import (
	"errors"
	"fmt"
	"os"
	"syscall"
)

func validateSocketDirOwner(info os.FileInfo) error {
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != uint32(os.Geteuid()) {
		return fmt.Errorf("streamnet: socket directory is not owned by the current user")
	}
	return nil
}

func validateUnixEndpoint(path string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return fmt.Errorf("streamnet: inspect Unix endpoint: %w", err)
	}
	if info.Mode()&os.ModeSocket == 0 {
		return fmt.Errorf("streamnet: endpoint is not a Unix socket: %s", path)
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != uint32(os.Geteuid()) {
		return fmt.Errorf("streamnet: Unix endpoint is not owned by the current user")
	}
	return nil
}

func prepareUnixEndpoint(path string) error {
	if err := validateUnixEndpoint(path); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil
		}
		return err
	}
	if err := os.Remove(path); err != nil {
		return fmt.Errorf("streamnet: remove stale Unix endpoint: %w", err)
	}
	return nil
}

func removeUnixEndpoint(path string) error {
	if err := validateUnixEndpoint(path); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil
		}
		return err
	}
	if err := os.Remove(path); err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("streamnet: remove Unix endpoint: %w", err)
	}
	return nil
}
