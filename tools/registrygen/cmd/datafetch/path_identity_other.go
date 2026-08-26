//go:build !windows

package main

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"strings"
	"syscall"
)

func isRedirectingDirectory(info os.FileInfo) bool {
	return info.Mode()&os.ModeSymlink != 0
}

func sameResolvedDirectoryPath(left, right string) bool {
	left, err := canonicalizeTrustedTopLevelAlias(filepath.Clean(left), os.Lstat, os.Readlink)
	return err == nil && left == filepath.Clean(right)
}

// canonicalizeTrustedTopLevelAlias resolves a root-owned symlink in the first
// path component while preserving all lower components for link rejection.
func canonicalizeTrustedTopLevelAlias(
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
		return "", errors.New("inspect system path alias")
	}
	if info.Mode()&os.ModeSymlink == 0 {
		return path, nil
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok || stat.Uid != 0 {
		return "", errors.New("system path alias is not trusted")
	}
	target, err := readlink(alias)
	if err != nil {
		return "", errors.New("resolve system path alias")
	}
	if !filepath.IsAbs(target) {
		target = filepath.Join(filepath.Dir(alias), target)
	} else {
		target = filepath.Clean(target)
	}
	if !filepath.IsAbs(target) {
		return "", errors.New("resolve system path alias")
	}
	return filepath.Join(target, strings.TrimPrefix(path, alias)), nil
}
