//go:build !windows

package packcache

import (
	"errors"
	"os"

	"golang.org/x/sys/unix"
)

func hasLinkAttribute(info os.FileInfo) bool        { return info.Mode()&os.ModeSymlink != 0 }
func ownerOnlyPath(_ string, info os.FileInfo) bool { return info.Mode().Perm()&0o077 == 0 }
func secureOwnerOnlyPath(path string, directory bool) error {
	if directory {
		return os.Chmod(path, 0o700)
	}
	return os.Chmod(path, 0o600)
}
func canonicalPlatformPath(path string) string { return path }
func syncDir(path string) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return f.Sync()
}

func openRegular(path string) (*os.File, error) {
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0)
	if err != nil {
		return nil, err
	}
	f := os.NewFile(uintptr(fd), path)
	info, err := f.Stat()
	if err != nil || !secureRegular(info) {
		_ = f.Close()
		if err != nil {
			return nil, err
		}
		return nil, errors.New("not a regular object")
	}
	return f, nil
}
