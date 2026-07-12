//go:build !windows

package authcache

import (
	"os"
	"testing"
)

func makeLinkedDirectory(t *testing.T, link, target string) {
	t.Helper()
	if err := os.Symlink(target, link); err != nil {
		t.Fatal(err)
	}
}
