//go:build darwin

package authcache

import (
	"context"
	"io"
	"path/filepath"
	"testing"

	"golang.org/x/oauth2"
)

func TestPersistentSourceCanonicalizesTrustedTopLevelAlias(t *testing.T) {
	raw := filepath.Join(t.TempDir(), "derived")
	canonical, err := canonicalizeCachePath(raw)
	if err != nil {
		t.Fatal(err)
	}
	if canonical == raw {
		t.Skip("temporary directory does not use a trusted top-level alias")
	}
	source := PersistentSource(context.Background(), raw, oauth2.StaticTokenSource(testOAuthToken("account-a")), io.Discard)
	persistent, ok := source.(*persistentAuthSource)
	if !ok {
		t.Fatal("persistent source was not constructed")
	}
	if persistent.path != canonical {
		t.Fatalf("persistent path = %q, want canonical path %q", persistent.path, canonical)
	}
}
