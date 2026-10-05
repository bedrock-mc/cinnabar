//go:build !windows

package packcache

import (
	"errors"
	"os"

	"golang.org/x/sys/unix"
)

// hasLinkAttribute reports symbolic links.
func hasLinkAttribute(info os.FileInfo) bool { return info.Mode()&os.ModeSymlink != 0 }

// canonicalPlatformPath preserves case-sensitive Unix paths.
func canonicalPlatformPath(path string) string { return path }

// openRegular opens only an ordinary archive, without following a leaf link.
func openRegular(path string) (*os.File, error) {
	fd, err := unix.Open(path, unix.O_RDONLY|unix.O_CLOEXEC|unix.O_NOFOLLOW, 0)
	if err != nil {
		return nil, err
	}
	f := os.NewFile(uintptr(fd), path)
	info, err := f.Stat()
	if err != nil || !regularNoLink(info) {
		_ = f.Close()
		if err != nil {
			return nil, err
		}
		return nil, errors.New("not a regular object")
	}
	return f, nil
}
