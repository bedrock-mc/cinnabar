//go:build !windows

package authcache

import "io/fs"

func isReparsePoint(fs.FileInfo) bool {
	return false
}
