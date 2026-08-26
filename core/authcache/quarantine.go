package authcache

import (
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
)

// invalidCacheSuffix marks a cache file that was moved aside after failing a
// safety check. The suffix mirrors the saved-server quarantine convention.
const invalidCacheSuffix = ".invalid"

type quarantineHooks struct {
	protect     func(*os.File) error
	afterRename func(targetPath string)
}

// quarantineCacheFile renames a rejected cache to a private sibling with the
// .invalid suffix. If privacy or identity verification fails, the opened
// token-bearing inode is scrubbed and every name for it in the parent is
// removed before the operation fails closed.
func quarantineCacheFile(path string) (string, error) {
	return quarantineCacheFileWith(path, quarantineHooks{})
}

// quarantineCacheFileWith performs quarantine with deterministic test hooks.
// Security-sensitive work always targets the opened inode, never an unverified
// pathname that a concurrent writer could substitute.
func quarantineCacheFileWith(path string, hooks quarantineHooks) (target string, returnErr error) {
	path, err := canonicalizeCachePath(filepath.Clean(path))
	if err != nil {
		return "", errors.New("resolve rejected auth cache path")
	}
	dir := filepath.Dir(path)
	parents, err := snapshotDirectoryChain(dir)
	if err != nil || !parents.complete {
		return "", errors.New("inspect rejected auth cache parent")
	}
	root, err := os.OpenRoot(dir)
	if err != nil {
		return "", errors.New("open rejected auth cache parent")
	}
	defer root.Close()

	sourceName := filepath.Base(path)
	sourceInfo, err := root.Lstat(sourceName)
	if err != nil {
		return "", err
	}
	if err := checkRegular(sourceInfo); err != nil {
		return "", err
	}
	file, err := root.OpenFile(sourceName, os.O_RDWR, 0)
	if err != nil {
		return "", err
	}
	openedInfo, err := file.Stat()
	if err != nil || !os.SameFile(sourceInfo, openedInfo) {
		_ = file.Close()
		return "", errors.New("rejected auth cache changed while opening")
	}
	secure := false
	defer func() {
		if secure {
			return
		}
		if cleanupErr := cleanupRejectedCacheIdentity(root, file, openedInfo); cleanupErr != nil {
			returnErr = errors.New("secure rejected auth cache after privacy failure")
		}
	}()

	targetName := sourceName + invalidCacheSuffix
	if err := root.Rename(sourceName, targetName); err != nil {
		return "", err
	}
	target = filepath.Join(dir, targetName)
	if hooks.afterRename != nil {
		hooks.afterRename(target)
	}
	if err := parents.revalidate(); err != nil {
		return "", errors.New("rejected auth cache parent changed during quarantine")
	}
	targetInfo, err := root.Lstat(targetName)
	if err != nil || !os.SameFile(openedInfo, targetInfo) {
		return "", errors.New("rejected auth cache changed during quarantine")
	}
	protect := protectOpenedCacheFile
	if hooks.protect != nil {
		protect = hooks.protect
	}
	if err := protect(file); err != nil {
		return "", errors.New("secure rejected auth cache after privacy failure")
	}
	protectedInfo, err := file.Stat()
	if err != nil || !os.SameFile(openedInfo, protectedInfo) {
		return "", errors.New("verify rejected auth cache identity")
	}
	if err := checkOpenedCacheFileSecurity(file, protectedInfo); err != nil {
		return "", errors.New("verify rejected auth cache privacy")
	}
	targetInfo, err = root.Lstat(targetName)
	if err != nil || !os.SameFile(openedInfo, targetInfo) {
		return "", errors.New("rejected auth cache changed after privacy stamp")
	}
	if err := file.Close(); err != nil {
		return "", errors.New("close quarantined auth cache")
	}
	secure = true
	return target, nil
}

// cleanupRejectedCacheIdentity scrubs the opened inode before removing only
// parent entries that still name it, preserving any foreign replacement.
func cleanupRejectedCacheIdentity(root *os.Root, file *os.File, identity fs.FileInfo) error {
	scrubErr := scrubOpenTemp(file)
	closeErr := file.Close()
	names, scanErr := identityNames(root, identity)
	removeErr := removeTempIdentityNames(root, identity, names)
	remaining, verifyErr := identityNames(root, identity)
	if scrubErr != nil || closeErr != nil || scanErr != nil || removeErr != nil || verifyErr != nil || len(remaining) != 0 {
		return errors.New("rejected auth cache cleanup could not be verified")
	}
	return nil
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
