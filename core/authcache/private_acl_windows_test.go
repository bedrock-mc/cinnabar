//go:build windows

package authcache

import (
	"os"
	"testing"
)

// stampPrivateACL makes a test-written file match what save() publishes on
// Windows: a protected trusted-cache ACL instead of ambient directory
// inheritance.
func stampPrivateACL(t *testing.T, path string) {
	t.Helper()
	file, err := os.OpenFile(path, os.O_RDWR, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer file.Close()
	if err := protectOpenedCacheFile(file); err != nil {
		t.Fatalf("protectOpenedCacheFile(%q) error = %v", path, err)
	}
}
