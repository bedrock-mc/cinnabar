//go:build windows

package authcache

import (
	"io/fs"
	"os"
	"unsafe"

	"golang.org/x/sys/windows"
)

// checkCacheSecurityByPath rejects a cache file whose security descriptor
// grants access to any principal beyond the owner, SYSTEM, and Administrators,
// or whose owner is not one of those trusted principals, before its contents
// are read. The group SID is requested alongside owner and DACL so both
// retrieval sites fetch identical descriptor sections, even though the policy
// itself judges only the owner and the ACE trustees.
func checkCacheSecurityByPath(path string, _ fs.FileInfo) error {
	descriptor, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION|windows.GROUP_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		return unsafePermissions("descriptor_unavailable", "security descriptor could not be read")
	}
	return verifyCacheDescriptor(descriptor)
}

// checkOpenedCacheFileSecurity re-checks the same contract against the open
// handle so descriptor changes between the path read and the open are still
// caught without another TOCTOU window. The fs.FileInfo parameter exists only
// for cross-platform signature parity and is deliberately unused here.
func checkOpenedCacheFileSecurity(file *os.File, _ fs.FileInfo) error {
	descriptor, err := windows.GetSecurityInfo(windows.Handle(file.Fd()), windows.SE_FILE_OBJECT,
		windows.OWNER_SECURITY_INFORMATION|windows.GROUP_SECURITY_INFORMATION|windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		return unsafePermissions("descriptor_unavailable", "security descriptor could not be read")
	}
	return verifyCacheDescriptor(descriptor)
}

// trustedCacheACL is the protected discretionary ACL every cache file carries:
// SYSTEM, Administrators, and the file owner hold full control while ordinary
// directory inheritance is disabled, so ambient grants on the surrounding
// profile directories cannot expose the token.
const trustedCacheACL = "D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;OW)"

// verifyCacheDescriptor enforces one deterministic policy: the only trustees
// that may appear as owner or in any access control entry are the current
// process user, LocalSystem, the Administrators group, and the well-known
// OWNER RIGHTS principal that names whoever owns the file. A missing or
// unreadable DACL fails closed instead of being accepted silently.
func verifyCacheDescriptor(descriptor *windows.SECURITY_DESCRIPTOR) error {
	if descriptor == nil {
		return unsafePermissions("descriptor_unavailable", "security descriptor could not be read")
	}
	owner, _, err := descriptor.Owner()
	if err != nil || owner == nil {
		return unsafePermissions("ownership_unverifiable", "file owner could not be resolved")
	}
	trusted, err := trustedCacheTrustees()
	if err != nil {
		return unsafePermissions("trustees_unavailable", "trusted principals could not be resolved")
	}
	if !sidInTrusted(owner, trusted) {
		return unsafePermissions("foreign_owner", "file is owned by an untrusted principal")
	}
	dacl, _, err := descriptor.DACL()
	if err != nil || dacl == nil {
		return unsafePermissions("acl_missing", "discretionary access control list is absent")
	}
	for index := uint32(0); index < uint32(dacl.AceCount); index++ {
		var ace *windows.ACCESS_ALLOWED_ACE
		if err := windows.GetAce(dacl, index, &ace); err != nil || ace == nil {
			return unsafePermissions("ace_unreadable", "access control entry could not be read")
		}
		trustee := (*windows.SID)(unsafe.Pointer(&ace.SidStart))
		if !sidInTrusted(trustee, trusted) {
			return unsafePermissions("broad_acl", "an access control entry grants an untrusted principal")
		}
	}
	return nil
}

// trustedCacheTrustees returns the current process token user plus the
// well-known principals allowed to appear on a private cache file.
func trustedCacheTrustees() ([]*windows.SID, error) {
	token, err := windows.OpenCurrentProcessToken()
	if err != nil {
		return nil, err
	}
	defer token.Close()
	user, err := token.GetTokenUser()
	if err != nil {
		return nil, err
	}
	system, err := windows.CreateWellKnownSid(windows.WinLocalSystemSid)
	if err != nil {
		return nil, err
	}
	administrators, err := windows.CreateWellKnownSid(windows.WinBuiltinAdministratorsSid)
	if err != nil {
		return nil, err
	}
	ownerRights, err := windows.CreateWellKnownSid(windows.WinCreatorOwnerRightsSid)
	if err != nil {
		return nil, err
	}
	return []*windows.SID{user.User.Sid, system, administrators, ownerRights}, nil
}

// protectCacheFile replaces a cache file's inherited ACL with the bounded
// trusted-cache ACL before any token bytes are written, mirroring the Unix
// owner-only chmod performed during save.
func protectCacheFile(path string) error {
	descriptor, err := windows.SecurityDescriptorFromString(trustedCacheACL)
	if err != nil {
		return err
	}
	dacl, _, err := descriptor.DACL()
	if err != nil || dacl == nil {
		return unsafePermissions("acl_missing", "trusted cache ACL could not be built")
	}
	return windows.SetNamedSecurityInfo(path, windows.SE_FILE_OBJECT,
		windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION,
		nil, nil, dacl, nil)
}

// stampCachePrivacy replaces a quarantined cache's ambient grants with the
// protected trusted-cache ACL so the moved-aside bytes stop carrying whatever
// broad access caused the rejection.
func stampCachePrivacy(path string) error {
	return protectCacheFile(path)
}

func sidInTrusted(sid *windows.SID, trusted []*windows.SID) bool {
	for _, candidate := range trusted {
		if windows.EqualSid(sid, candidate) {
			return true
		}
	}
	return false
}
