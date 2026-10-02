//go:build windows

package authcache

import (
	"errors"
	"io/fs"
	"os"

	"golang.org/x/sys/windows"
)

// checkCachePermissions leaves existing Windows ACL policy to the user's profile.
func checkCachePermissions(_ fs.FileInfo) error { return nil }

// protectOpenedCacheFile makes a fresh credential file private before any bytes
// are written. Windows ignores mode 0600, so this small ACL stamp is required.
func protectOpenedCacheFile(file *os.File) error {
	descriptor, err := windows.SecurityDescriptorFromString("D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;OW)")
	if err != nil {
		return err
	}
	dacl, _, err := descriptor.DACL()
	if err != nil || dacl == nil {
		return errors.New("create private credential ACL")
	}
	return windows.SetNamedSecurityInfo(file.Name(), windows.SE_FILE_OBJECT,
		windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION,
		nil, nil, dacl, nil)
}
