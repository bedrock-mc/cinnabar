package experience

import "os"

// commitAtomicFile renames a synced file without trying to flush a read-only Windows directory.
func commitAtomicFile(temporary, path string) error {
	return os.Rename(temporary, path)
}
