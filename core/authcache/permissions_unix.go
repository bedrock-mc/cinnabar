//go:build !windows

package authcache

import (
	"errors"
	"io/fs"
	"os"
)

// checkCachePermissions rejects credentials readable or writable by another user.
func checkCachePermissions(info fs.FileInfo) error {
	if info.Mode().Perm()&0o077 != 0 {
		return errors.New("auth cache file is not private to its owner")
	}
	return nil
}

// protectOpenedCacheFile limits a newly created credential file to its owner.
func protectOpenedCacheFile(file *os.File) error {
	return file.Chmod(0o600)
}
