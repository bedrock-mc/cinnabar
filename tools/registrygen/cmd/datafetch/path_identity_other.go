//go:build !windows

package main

import (
	"os"
	"path/filepath"
)

func isRedirectingDirectory(info os.FileInfo) bool {
	return info.Mode()&os.ModeSymlink != 0
}

func sameResolvedDirectoryPath(left, right string) bool {
	return filepath.Clean(left) == filepath.Clean(right)
}
