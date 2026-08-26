package authcache

import (
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
)

// invalidCacheSuffix marks a cache file that was moved aside after failing a
// safety check. The suffix mirrors the saved-server quarantine convention.
const invalidCacheSuffix = ".invalid"

// quarantineCacheFile renames a rejected cache to a private sibling with the
// .invalid suffix. An earlier quarantine is replaced. If the moved bytes
// cannot be made private, they are removed and the operation fails closed.
func quarantineCacheFile(path string) (string, error) {
	return quarantineCacheFileWith(path, stampCachePrivacy)
}

// quarantineCacheFileWith performs quarantine using the supplied privacy
// stamp so failure cleanup can be exercised deterministically in tests.
func quarantineCacheFileWith(path string, stamp func(string) error) (string, error) {
	target := path + invalidCacheSuffix
	if _, err := os.Lstat(target); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return "", err
	}
	if err := os.Rename(path, target); err != nil {
		return "", err
	}
	if err := stamp(target); err != nil {
		if removeErr := os.Remove(target); removeErr != nil && !errors.Is(removeErr, fs.ErrNotExist) {
			return "", errors.New("secure rejected auth cache after privacy failure")
		}
		return "", errors.New("secure rejected auth cache after privacy failure")
	}
	return target, nil
}

// notifyQuarantinedCache surfaces the recovery reason through the writer the
// caller already gave this package (standard output for the core lifecycle),
// as one bounded line carrying only the fixed rejection code and the cache
// path. It never contains token material.
func notifyQuarantinedCache(writer io.Writer, path string, reason error) {
	if writer == nil {
		return
	}
	var unsafe *unsafePermissionsError
	code := "unknown"
	if errors.As(reason, &unsafe) {
		code = unsafe.code
	}
	fmt.Fprintf(writer, "AUTH_CACHE_QUARANTINED code=%s path=%s\n", code, path)
}
