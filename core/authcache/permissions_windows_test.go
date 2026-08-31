//go:build windows

package authcache

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"golang.org/x/sys/windows"
)

// A foreign-owned file cannot be produced without elevated privileges
// (SeRestorePrivilege or SeTakeOwnershipPrivilege), so a real foreign-owned
// cache is not available to an unprivileged test. The owner rejection is
// instead exercised through descriptors constructed directly from SDDL, which
// needs no rights at all; the remaining tests prove the trustee allowlist
// against real ACLs available unprivileged.

func TestVerifyCacheDescriptorRejectsForeignOwnerWithTrustedDACL(t *testing.T) {
	// AU (Authenticated Users) sits outside the trustee allowlist while every
	// ACE in the DACL is trusted, so the only possible rejection source is the
	// owner check.
	descriptor, err := windows.SecurityDescriptorFromString(
		"O:AUD:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;FA;;;OW)")
	if err != nil {
		t.Fatalf("SecurityDescriptorFromString() error = %v", err)
	}
	assertUnsafePermissionsCode(t, verifyCacheDescriptor(descriptor), "foreign_owner")
}

func TestVerifyCacheDescriptorRejectsBroadAceUnderTrustedOwner(t *testing.T) {
	// LocalSystem owns this descriptor, so ownership passes and the lone
	// Everyone (WD) allow ACE must be what trips the broad_acl rejection.
	descriptor, err := windows.SecurityDescriptorFromString("O:SYD:P(A;;FA;;;WD)")
	if err != nil {
		t.Fatalf("SecurityDescriptorFromString() error = %v", err)
	}
	assertUnsafePermissionsCode(t, verifyCacheDescriptor(descriptor), "broad_acl")
}

func TestCheckCacheSecurityByPathAcceptsProtectedTrustedACL(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeFile(t, path, []byte(`{"refresh_token":"sentinel"}`))
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}

	if err := checkCacheSecurityByPath(path, info); err != nil {
		t.Fatalf("checkCacheSecurityByPath() error = %v, want acceptance of the protected trusted-cache ACL", err)
	}
}

func TestCheckOpenedCacheFileSecurityAcceptsProtectedTrustedACL(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeFile(t, path, []byte(`{"refresh_token":"sentinel"}`))
	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	openInfo, err := file.Stat()
	if err != nil {
		t.Fatal(err)
	}

	if err := checkOpenedCacheFileSecurity(file, openInfo); err != nil {
		t.Fatalf("checkOpenedCacheFileSecurity() error = %v, want acceptance", err)
	}
}

func TestReopenCacheSecurityHandleAcceptsGoFileHandle(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	file, err := os.OpenFile(path, os.O_RDWR|os.O_CREATE|os.O_EXCL, 0o600)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()

	securityHandle, err := reopenCacheSecurityHandle(file)
	if err != nil {
		t.Fatalf("reopenCacheSecurityHandle() error = %v", err)
	}
	if err := windows.CloseHandle(securityHandle); err != nil {
		t.Fatalf("CloseHandle(reopened security handle) error = %v", err)
	}
	if _, err := file.Write([]byte("still-open")); err != nil {
		t.Fatalf("original Go file handle was disturbed: %v", err)
	}
}

func TestCheckCacheSecurityRejectsEveryoneGrantWithoutAdminRights(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeFile(t, path, []byte(`{"refresh_token":"sentinel"}`))
	broadenACLWithEveryone(t, path)
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}

	assertUnsafePermissionsCode(t, checkCacheSecurityByPath(path, info), "broad_acl")

	file, err := os.Open(path)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	openInfo, err := file.Stat()
	if err != nil {
		t.Fatal(err)
	}
	assertUnsafePermissionsCode(t, checkOpenedCacheFileSecurity(file, openInfo), "broad_acl")
}

// broadenACLWithEveryone grants the Everyone group (S-1-1-0) full control
// through icacls. The current user owns the freshly created file, so the
// DACL change needs no elevated rights; environments without icacls skip
// with a reason instead of failing.
func broadenACLWithEveryone(t *testing.T, path string) {
	t.Helper()
	output, err := exec.Command("icacls", path, "/grant", "*S-1-1-0:F").CombinedOutput()
	if err != nil {
		t.Skipf("broaden ACL with icacls requires the utility on PATH: %v: %s", err, output)
	}
}

func TestCheckCacheSecurityRejectsUnreadableDescriptorTarget(t *testing.T) {
	missing := filepath.Join(t.TempDir(), "missing-token.json")
	err := checkCacheSecurityByPath(missing, nil)
	var unsafe *unsafePermissionsError
	if err == nil || !errors.As(err, &unsafe) || unsafe.code != "descriptor_unavailable" {
		t.Fatalf("checkCacheSecurityByPath() error = %v, want descriptor_unavailable", err)
	}
}
