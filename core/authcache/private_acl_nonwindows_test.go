//go:build !windows

package authcache

import "testing"

// stampPrivateACL makes a test-written file match what save() publishes on
// this platform. Unix caches are already owner-only through the 0o600 mode
// applied by the shared write helpers.
func stampPrivateACL(_ *testing.T, _ string) {}
