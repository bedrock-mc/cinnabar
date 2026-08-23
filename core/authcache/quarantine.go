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

// quarantineCacheFile renames a rejected cache to a sibling with the
// .invalid suffix so its bytes survive for inspection while the session
// proceeds exactly as if no cache existed. An earlier quarantine is replaced;
// the original file is never deleted. The moved-aside sibling is restamped
// private on a best-effort basis so the rejected bytes stop carrying the
// group/world access or ambient grants that provoked the rejection; a stamp
// failure is tolerated because the session already treats the cache as
// absent and startup must not fail over retained diagnostic bytes.
func quarantineCacheFile(path string) (string, error) {
	target := path + invalidCacheSuffix
	if _, err := os.Lstat(target); err != nil && !errors.Is(err, fs.ErrNotExist) {
		return "", err
	}
	if err := os.Rename(path, target); err != nil {
		return "", err
	}
	_ = stampCachePrivacy(target)
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
