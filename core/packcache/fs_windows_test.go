//go:build windows

package packcache

import (
	"context"
	"os"
	"path/filepath"
	"testing"

	"github.com/google/uuid"
	"golang.org/x/sys/windows"
)

func TestWindowsRejectsPermissiveParentRootAndObjectDACL(t *testing.T) {
	t.Run("parent", func(t *testing.T) {
		base := secureTempDir(t)
		parent := filepath.Join(base, "parent")
		if err := os.Mkdir(parent, 0o700); err != nil {
			t.Fatal(err)
		}
		setEveryoneDACL(t, parent)
		if _, err := New(filepath.Join(parent, "objects")); err == nil {
			t.Fatal("permissive parent DACL accepted")
		}
	})
	t.Run("root", func(t *testing.T) {
		base := secureTempDir(t)
		root := filepath.Join(base, "objects")
		if err := os.Mkdir(root, 0o700); err != nil {
			t.Fatal(err)
		}
		setEveryoneDACL(t, root)
		if _, err := New(root); err == nil {
			t.Fatal("permissive root DACL accepted")
		}
	})
	t.Run("object", func(t *testing.T) {
		c := newTestCache(t, 1<<20)
		pack, key, _ := testPack(t, uuid.New(), "1.0.0", "acl")
		if err := c.Store(context.Background(), key, pack); err != nil {
			t.Fatal(err)
		}
		name, _ := objectName(key)
		setEveryoneDACL(t, filepath.Join(c.root, name))
		if got, err := c.Load(context.Background(), key); err != nil || got != nil {
			t.Fatalf("Load = %v, %v; want miss", got, err)
		}
	})
}

func TestWindowsRejectsReparseParent(t *testing.T) {
	base := secureTempDir(t)
	realParent := filepath.Join(base, "real")
	if err := os.Mkdir(realParent, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := secureCreatedPath(realParent, true); err != nil {
		t.Fatal(err)
	}
	linked := filepath.Join(base, "linked")
	if err := os.Symlink(realParent, linked); err != nil {
		t.Skipf("symlink privilege unavailable: %v", err)
	}
	if _, err := New(filepath.Join(linked, "objects")); err == nil {
		t.Fatal("reparse parent accepted")
	}
}

func TestWindowsOwnerOnlySecurityTakesCurrentUserOwnership(t *testing.T) {
	user, err := currentUserSID()
	if err != nil {
		t.Fatal(err)
	}
	called := false
	err = applyOwnerOnlySecurity("unused", true, user, func(
		path string,
		objectType windows.SE_OBJECT_TYPE,
		securityInfo windows.SECURITY_INFORMATION,
		owner, group *windows.SID,
		dacl, sacl *windows.ACL,
	) error {
		called = true
		if path != "unused" || objectType != windows.SE_FILE_OBJECT {
			t.Fatalf("security target = (%q, %v)", path, objectType)
		}
		wantInfo := windows.SECURITY_INFORMATION(windows.OWNER_SECURITY_INFORMATION | windows.DACL_SECURITY_INFORMATION | windows.PROTECTED_DACL_SECURITY_INFORMATION)
		if securityInfo != wantInfo {
			t.Fatalf("security information = %#x, want %#x", securityInfo, wantInfo)
		}
		if owner == nil || !owner.Equals(user) {
			t.Fatal("current user SID was not installed as owner")
		}
		if group != nil || dacl == nil || sacl != nil {
			t.Fatalf("security values = owner %v group %v dacl %v sacl %v", owner, group, dacl, sacl)
		}
		return nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if !called {
		t.Fatal("security setter was not called")
	}
}

func TestWindowsSecureCreatedPathRemovesPermissiveInheritedACL(t *testing.T) {
	base := t.TempDir()
	everyone, err := windows.CreateWellKnownSid(windows.WinWorldSid)
	if err != nil {
		t.Fatal(err)
	}
	acl, err := windows.ACLFromEntries([]windows.EXPLICIT_ACCESS{{
		AccessPermissions: windows.GENERIC_ALL,
		AccessMode:        windows.SET_ACCESS,
		Inheritance:       windows.SUB_CONTAINERS_AND_OBJECTS_INHERIT,
		Trustee: windows.TRUSTEE{
			TrusteeForm: windows.TRUSTEE_IS_SID, TrusteeType: windows.TRUSTEE_IS_WELL_KNOWN_GROUP,
			TrusteeValue: windows.TrusteeValueFromSID(everyone),
		},
	}}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if err := windows.SetNamedSecurityInfo(base, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION, nil, nil, acl, nil); err != nil {
		t.Fatal(err)
	}
	child := filepath.Join(base, "child")
	if err := os.Mkdir(child, 0o700); err != nil {
		t.Fatal(err)
	}
	if ownerOnlyPath(child, nil) {
		t.Fatal("permissive inherited child unexpectedly passed owner-only validation")
	}
	if err := secureCreatedPath(child, true); err != nil {
		t.Fatalf("secureCreatedPath() error = %v", err)
	}
	if !ownerOnlyPath(child, nil) {
		t.Fatal("secured child did not pass owner-only validation")
	}
}

func setEveryoneDACL(t *testing.T, path string) {
	t.Helper()
	everyone, err := windows.CreateWellKnownSid(windows.WinWorldSid)
	if err != nil {
		t.Fatal(err)
	}
	acl, err := windows.ACLFromEntries([]windows.EXPLICIT_ACCESS{{
		AccessPermissions: windows.GENERIC_ALL,
		AccessMode:        windows.SET_ACCESS,
		Trustee:           windows.TRUSTEE{TrusteeForm: windows.TRUSTEE_IS_SID, TrusteeType: windows.TRUSTEE_IS_WELL_KNOWN_GROUP, TrusteeValue: windows.TrusteeValueFromSID(everyone)},
	}}, nil)
	if err != nil {
		t.Fatal(err)
	}
	if err := windows.SetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION|windows.PROTECTED_DACL_SECURITY_INFORMATION, nil, nil, acl, nil); err != nil {
		t.Fatal(err)
	}
}
