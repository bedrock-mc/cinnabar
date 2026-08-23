//go:build windows

package authcache

import (
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

// A foreign-owned file cannot be produced without elevated privileges
// (SeRestorePrivilege or SeTakeOwnershipPrivilege), so owner rejection is
// exercised only through the typed policy on other platforms; this file
// proves the trustee allowlist against real ACLs available unprivileged.

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
