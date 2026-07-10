//go:build !windows

package proxy

import "path/filepath"

func canonicalExistingPath(path string) (string, error) {
	return filepath.EvalSymlinks(path)
}
