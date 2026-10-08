//go:build !windows

package authcache

import (
	"context"
	"io"
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/oauth2"
)

func TestSourceRejectsPublicCredentialFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oauth.json")
	writeToken(t, path, token("test-access", "test-refresh"))
	if err := os.Chmod(path, 0o644); err != nil {
		t.Fatal(err)
	}
	_, err := Source(context.Background(), Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
		t.Error("an unreadable cache must not start sign-in or replace credentials")
		return token("new", "new-refresh"), nil
	}})
	if err == nil {
		t.Fatal("accepted credential file readable by other users")
	}
	assertCachedToken(t, path, token("test-access", "test-refresh"))
}
