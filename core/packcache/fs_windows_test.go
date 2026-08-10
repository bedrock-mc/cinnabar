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
