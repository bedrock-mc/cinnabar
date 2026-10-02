//go:build windows

package packcache

import (
	"errors"
	"os"
	"strings"
	"syscall"

	"golang.org/x/sys/windows"
)

// hasLinkAttribute reports Windows reparse points.
func hasLinkAttribute(info os.FileInfo) bool {
	data, ok := info.Sys().(*syscall.Win32FileAttributeData)
	return !ok || data.FileAttributes&syscall.FILE_ATTRIBUTE_REPARSE_POINT != 0
}

// syncDir is unnecessary on Windows because the directory cannot be synced this way.
func syncDir(string) error { return nil }

// canonicalPlatformPath folds case for the in-process root lease.
func canonicalPlatformPath(path string) string { return strings.ToLower(path) }

// openRegular opens an archive without following a reparse point.
func openRegular(path string) (*os.File, error) {
	p, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return nil, err
	}
	h, err := windows.CreateFile(p, windows.GENERIC_READ, windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE, nil, windows.OPEN_EXISTING, windows.FILE_ATTRIBUTE_NORMAL|windows.FILE_FLAG_OPEN_REPARSE_POINT, 0)
	if err != nil {
		return nil, err
	}
	f := os.NewFile(uintptr(h), path)
	info, err := f.Stat()
	if err != nil || !regularNoLink(info) {
		_ = f.Close()
		if err != nil {
			return nil, err
		}
		return nil, errors.New("not a regular object")
	}
	return f, nil
}
