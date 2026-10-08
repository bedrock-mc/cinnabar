//go:build windows

package authcache

import (
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/sys/windows"
)

func TestPublishedCredentialsHavePrivateWindowsACL(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oauth.json")
	if err := save(path, token("test-access", "test-refresh"), ""); err != nil {
		t.Fatal(err)
	}
	descriptor, err := windows.GetNamedSecurityInfo(path, windows.SE_FILE_OBJECT, windows.DACL_SECURITY_INFORMATION)
	if err != nil {
		t.Fatal(err)
	}
	control, _, err := descriptor.Control()
	if err != nil || control&windows.SE_DACL_PROTECTED == 0 {
		t.Fatalf("credential file inherits ambient ACLs: %v", err)
	}
	dacl, _, err := descriptor.DACL()
	if err != nil || dacl == nil || dacl.AceCount != 3 {
		t.Fatalf("private credential ACL = %v, %v", dacl, err)
	}
	for _, trustee := range []string{";;;SY)", ";;;BA)", ";;;OW)"} {
		if !strings.Contains(descriptor.String(), trustee) {
			t.Fatalf("credential ACL is missing trustee %s", trustee)
		}
	}
}
