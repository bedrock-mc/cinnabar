//go:build !windows

package authcache

import (
	"io/fs"
	"os"
	"syscall"
)

// checkCacheSecurityByPath rejects a cache file that is group/world readable
// or writable, or owned by another user, before its contents are read.
func checkCacheSecurityByPath(_ string, info fs.FileInfo) error {
	return checkUnixCacheOwnership(info)
}

// checkOpenedCacheFileSecurity re-checks the same contract against the opened
// object so permission changes between stat and open are still caught.
func checkOpenedCacheFileSecurity(_ *os.File, info fs.FileInfo) error {
	return checkUnixCacheOwnership(info)
}

// protectOpenedCacheFile applies the owner-only mode through the already-open
// descriptor so a concurrent leaf-name substitution cannot redirect the
// protection operation to another inode.
func protectOpenedCacheFile(file *os.File) error {
	return file.Chmod(0o600)
}

// checkUnixCacheOwnership enforces owner-only access: no group or other
// permission bits may be set, and the file must belong to the effective user
// running this process. Ownership that cannot be verified fails closed rather
// than being accepted silently.
func checkUnixCacheOwnership(info fs.FileInfo) error {
	if info.Mode().Perm()&0o077 != 0 {
		return unsafePermissions("group_or_world_access", "file mode grants group or other access")
	}
	stat, ok := info.Sys().(*syscall.Stat_t)
	if !ok {
		return unsafePermissions("ownership_unverifiable", "file ownership could not be verified")
	}
	if uint32(stat.Uid) != uint32(os.Geteuid()) {
		return unsafePermissions("foreign_owner", "file is owned by another user")
	}
	return nil
}
