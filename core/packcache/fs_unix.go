//go:build !windows

package packcache

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"syscall"

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

// canonicalizeTopLevelAlias resolves only a root-owned first Unix path
// component. macOS exposes standard temporary paths through a system-owned
// alias such as /var -> private/var. Links below that boundary remain
// untouched so validatePathComponents rejects them.
func canonicalizeTopLevelAlias(path string) (string, error) {
	return canonicalizeTopLevelAliasWith(path, os.Lstat, os.Readlink)
}

func canonicalizeTopLevelAliasWith(
	path string,
	lstat func(string) (fs.FileInfo, error),
	readlink func(string) (string, error),
) (string, error) {
	if !filepath.IsAbs(path) || path == string(filepath.Separator) {
		return path, nil
	}
	relative := strings.TrimPrefix(path, string(filepath.Separator))
	component, _, _ := strings.Cut(relative, string(filepath.Separator))
	if component == "" {
		return path, nil
	}
	alias := string(filepath.Separator) + component
	info, err := lstat(alias)
	if err != nil {
		return "", errors.New("inspect cache system path")
	}
	if info.Mode()&os.ModeSymlink == 0 {
		return path, nil
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != 0 {
		return "", errors.New("cache system path alias is not trusted")
	}
	target, err := readlink(alias)
	if err != nil {
		return "", errors.New("resolve cache system path alias")
	}
	if !filepath.IsAbs(target) {
		target = filepath.Join(filepath.Dir(alias), target)
	} else {
		target = filepath.Clean(target)
	}
	if !filepath.IsAbs(target) {
		return "", errors.New("resolve cache system path alias")
	}
	remainder := strings.TrimPrefix(path, alias)
	return filepath.Join(target, remainder), nil
}

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
