//go:build !windows

package experience

import (
	"errors"
	"os"
	"path/filepath"
)

// commitAtomicFile renames a synced file and persists the directory entry on Unix.
func commitAtomicFile(temporary, path string) error {
	if err := os.Rename(temporary, path); err != nil {
		return err
	}
	dir, err := os.Open(filepath.Dir(path))
	if err != nil {
		return err
	}
	return errors.Join(dir.Sync(), dir.Close())
}
