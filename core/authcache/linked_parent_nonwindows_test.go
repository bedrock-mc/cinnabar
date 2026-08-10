//go:build !windows

package authcache

import (
	"context"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"testing"

	"golang.org/x/oauth2"
)

type rootOwnedFileInfo struct {
	fs.FileInfo
}

func (rootOwnedFileInfo) Sys() any {
	return &syscall.Stat_t{Uid: 0}
}

func makeLinkedDirectory(t *testing.T, link, target string) {
	t.Helper()
	if err := os.Symlink(target, link); err != nil {
		t.Fatal(err)
	}
}

func TestSourceAcceptsMacOSStandardTemporaryDirectoryAlias(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("macOS standard temporary-directory alias regression")
	}
	raw := filepath.Join(t.TempDir(), "auth", "microsoft-token.json")
	canonical, err := canonicalizeCachePath(raw)
	if err != nil {
		t.Fatalf("canonicalizeCachePath() error = %v", err)
	}
	if strings.HasPrefix(raw, "/var/") && strings.HasPrefix(canonical, "/var/") {
		t.Fatalf("canonical path retained the /var alias: %q", canonical)
	}

	_, err = Source(context.Background(), Config{
		Path: raw,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			return token("new-access", "new-refresh"), nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatalf("Source() through standard macOS temporary alias: %v", err)
	}
}

func TestCanonicalizeCachePathPreservesNestedAliasForRejection(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "target")
	if err := os.Mkdir(target, 0o700); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(root, "linked-parent")
	makeLinkedDirectory(t, alias, target)
	raw := filepath.Join(alias, "microsoft-token.json")

	canonical, err := canonicalizeCachePath(raw)
	if err != nil {
		t.Fatalf("canonicalizeCachePath() error = %v", err)
	}
	canonicalRoot, err := canonicalizeCachePath(root)
	if err != nil {
		t.Fatalf("canonicalizeCachePath(root) error = %v", err)
	}
	want := filepath.Join(canonicalRoot, "linked-parent", "microsoft-token.json")
	if canonical != want {
		t.Fatalf("canonicalizeCachePath() = %q, want nested alias retained as %q", canonical, want)
	}
	if _, err := snapshotDirectoryChain(filepath.Dir(canonical)); err == nil {
		t.Fatal("snapshotDirectoryChain() accepted the retained nested alias")
	}
}

func TestCanonicalizeCachePathDoesNotEraseAliasTargetChainLinks(t *testing.T) {
	root := t.TempDir()
	realTarget := filepath.Join(root, "real-target")
	if err := os.Mkdir(realTarget, 0o700); err != nil {
		t.Fatal(err)
	}
	nestedAlias := filepath.Join(root, "nested-alias")
	makeLinkedDirectory(t, nestedAlias, realTarget)
	aliasInfo, err := os.Lstat(nestedAlias)
	if err != nil {
		t.Fatal(err)
	}

	raw := filepath.Join(string(filepath.Separator), "trusted-alias", "auth", "microsoft-token.json")
	canonical, err := canonicalizeCachePathWith(
		raw,
		func(path string) (fs.FileInfo, error) {
			if path != string(filepath.Separator)+"trusted-alias" {
				t.Fatalf("lstat path = %q", path)
			}
			return rootOwnedFileInfo{FileInfo: aliasInfo}, nil
		},
		func(path string) (string, error) {
			if path != string(filepath.Separator)+"trusted-alias" {
				t.Fatalf("readlink path = %q", path)
			}
			return nestedAlias, nil
		},
	)
	if err != nil {
		t.Fatalf("canonicalizeCachePathWith() error = %v", err)
	}
	want := filepath.Join(nestedAlias, "auth", "microsoft-token.json")
	if canonical != want {
		t.Fatalf("canonicalizeCachePathWith() = %q, want %q", canonical, want)
	}
	if _, err := snapshotDirectoryChain(filepath.Dir(canonical)); err == nil {
		t.Fatal("snapshotDirectoryChain() accepted a link in the trusted alias target chain")
	}
}
