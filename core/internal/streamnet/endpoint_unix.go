//go:build !windows

package streamnet

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"net"
	"os"
	"path/filepath"
	"syscall"
	"time"
)

// sockaddr_un.sun_path is 104 bytes on macOS and 108 bytes on Linux. Keep one
// conservative limit, including room for the trailing NUL used by pathname
// sockets, so a socket directory accepted on one desktop Unix remains usable
// on the other.
const maxUnixEndpointPathBytes = 103

func unixEndpointPath(socketDir string) string {
	return unixEndpointPathNamed(socketDir, unixEndpointName)
}

func unixEndpointPathNamed(socketDir, endpointName string) string {
	// filepath.Clean is the cross-language contract: normalize separators and
	// dot components lexically, without resolving anything in the filesystem,
	// before measuring or hashing the endpoint bytes.
	direct := filepath.Clean(filepath.Join(socketDir, endpointName))
	if len(direct) <= maxUnixEndpointPathBytes {
		return direct
	}
	digest := sha256.Sum256([]byte(direct))
	return filepath.Join("/tmp", "cinnabar-"+hex.EncodeToString(digest[:16])+".sock")
}

type unixEndpointIdentity struct {
	device uint64
	inode  uint64
}

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
	identity, err := unixEndpointIdentityAt(path)
	if err != nil {
		return err
	}
	conn, dialErr := net.DialTimeout("unix", path, 250*time.Millisecond)
	if dialErr == nil {
		_ = conn.Close()
		return fmt.Errorf("streamnet: Unix endpoint is active: %s", path)
	}
	if !isConnectionRefused(dialErr) {
		return fmt.Errorf("streamnet: cannot prove Unix endpoint stale: %w", dialErr)
	}
	return removeUnixEndpoint(path, identity)
}

func unixEndpointIdentityAt(path string) (unixEndpointIdentity, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return unixEndpointIdentity{}, fmt.Errorf("streamnet: inspect Unix endpoint identity: %w", err)
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok {
		return unixEndpointIdentity{}, fmt.Errorf("streamnet: Unix endpoint identity is unavailable")
	}
	return unixEndpointIdentity{device: uint64(stat.Dev), inode: uint64(stat.Ino)}, nil
}

func removeUnixEndpoint(path string, expected unixEndpointIdentity) error {
	if err := validateUnixEndpoint(path); err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil
		}
		return err
	}
	actual, err := unixEndpointIdentityAt(path)
	if err != nil {
		return err
	}
	if actual != expected {
		return fmt.Errorf("streamnet: Unix endpoint identity changed before cleanup")
	}
	if err := os.Remove(path); err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("streamnet: remove Unix endpoint: %w", err)
	}
	return nil
}

func isConnectionRefused(err error) bool { return errors.Is(err, syscall.ECONNREFUSED) }
