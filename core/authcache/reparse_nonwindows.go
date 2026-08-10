//go:build !windows

package authcache

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"syscall"
)

func isReparsePoint(fs.FileInfo) bool {
	return false
}

// canonicalizeCachePath resolves only a root-owned top-level Unix alias. macOS
// exposes standard temporary paths through aliases such as /var -> private/var;
// resolving that operating-system boundary before the directory-chain snapshot
// prevents the alias itself from being mistaken for attacker-controlled
// traversal. Links below the first path component remain untouched and are
// rejected by snapshotDirectoryChain.
func canonicalizeCachePath(path string) (string, error) {
	return canonicalizeCachePathWith(path, os.Lstat, os.Readlink)
}

func canonicalizeCachePathWith(
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
		return "", errors.New("inspect auth cache system path")
	}
	if info.Mode()&os.ModeSymlink == 0 {
		return path, nil
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != 0 {
		return "", errors.New("auth cache system path alias is not trusted")
	}
	target, err := readlink(alias)
	if err != nil {
		return "", errors.New("resolve auth cache system path alias")
	}
	if !filepath.IsAbs(target) {
		target = filepath.Join(filepath.Dir(alias), target)
	} else {
		target = filepath.Clean(target)
	}
	if !filepath.IsAbs(target) {
		return "", errors.New("resolve auth cache system path alias")
	}
	remainder := strings.TrimPrefix(path, alias)
	return filepath.Join(target, remainder), nil
}
