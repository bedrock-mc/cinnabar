//go:build linux

package packcache

import "golang.org/x/sys/unix"

// publishNoReplace moves a complete archive without overwriting any existing entry.
// It avoids hard links denied in Android app sandboxes and fails safely if unsupported.
func publishNoReplace(temp, dest string) error {
	return unix.Renameat2(unix.AT_FDCWD, temp, unix.AT_FDCWD, dest, unix.RENAME_NOREPLACE)
}
