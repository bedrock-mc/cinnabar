package authcache

import (
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

// loadPrivate reads a bounded regular file. The user's cache directory is trusted;
// linked leaf files and overly broad Unix permissions are still rejected.
func loadPrivate(path string, limit int64) ([]byte, error) {
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.Mode().IsRegular() {
		return nil, errors.New("auth cache is not a regular file")
	}
	if err := checkCachePermissions(info); err != nil {
		return nil, err
	}
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	contents, err := io.ReadAll(io.LimitReader(file, limit+1))
	if err != nil {
		return nil, err
	}
	if int64(len(contents)) > limit {
		return nil, fmt.Errorf("private cache exceeds %d bytes", limit)
	}
	return contents, nil
}

// savePrivate writes a private temporary sibling, syncs and closes it, then
// replaces the cache atomically. Failed writes leave the previous cache intact.
func savePrivate(path string, serialized []byte) error {
	if len(serialized) == 0 || len(serialized) > maxCacheSize {
		return fmt.Errorf("private cache exceeds %d bytes", maxCacheSize)
	}
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	file, err := os.CreateTemp(dir, ".auth-cache-*.tmp")
	if err != nil {
		return err
	}
	defer os.Remove(file.Name())
	defer file.Close()
	if err := protectOpenedCacheFile(file); err != nil {
		return errors.New("protect temporary auth cache")
	}
	if _, err := file.Write(serialized); err != nil {
		return err
	}
	if err := file.Sync(); err != nil {
		return err
	}
	if err := file.Close(); err != nil {
		return err
	}
	return os.Rename(file.Name(), path)
}
