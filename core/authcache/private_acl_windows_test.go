//go:build windows

package authcache

import "testing"

// stampPrivateACL makes a test-written file match what save() publishes on
// Windows: a protected trusted-cache ACL instead of ambient directory
// inheritance.
func stampPrivateACL(t *testing.T, path string) {
	t.Helper()
	if err := protectCacheFile(path); err != nil {
		t.Fatalf("protectCacheFile(%q) error = %v", path, err)
	}
}
