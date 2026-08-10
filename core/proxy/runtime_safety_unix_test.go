//go:build !windows

package proxy

import (
	"os"
	"path/filepath"
	"testing"
)

func canonicalExistingPath(path string) (string, error) {
	return filepath.EvalSymlinks(path)
}

func createDirectoryAlias(t *testing.T, alias, target string) {
	t.Helper()
	if err := os.Symlink(target, alias); err != nil {
		t.Skipf("directory symlink unavailable: %v", err)
	}
}
