//go:build !windows

package packcache

import (
	"io/fs"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"testing"
)

type rootOwnedAliasInfo struct {
	fs.FileInfo
}

func (rootOwnedAliasInfo) Sys() any {
	return &syscall.Stat_t{Uid: 0}
}

func TestCanonicalizeTopLevelAliasResolvesOneTrustedHop(t *testing.T) {
	aliasInfo, err := os.Lstat(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	aliasInfo = symlinkFileInfo{FileInfo: aliasInfo}
	raw := filepath.Join(string(filepath.Separator), "trusted-alias", "cache", "objects")

	canonical, err := canonicalizeTopLevelAliasWith(
		raw,
		func(path string) (fs.FileInfo, error) {
			if path != string(filepath.Separator)+"trusted-alias" {
				t.Fatalf("lstat path = %q", path)
			}
			return rootOwnedAliasInfo{FileInfo: aliasInfo}, nil
		},
		func(path string) (string, error) {
			if path != string(filepath.Separator)+"trusted-alias" {
				t.Fatalf("readlink path = %q", path)
			}
			return filepath.Join(string(filepath.Separator), "real", "root"), nil
		},
	)
	if err != nil {
		t.Fatalf("canonicalizeTopLevelAliasWith() error = %v", err)
	}
	want := filepath.Join(string(filepath.Separator), "real", "root", "cache", "objects")
	if canonical != want {
		t.Fatalf("canonicalizeTopLevelAliasWith() = %q, want %q", canonical, want)
	}
}

func TestNewAcceptsMacOSStandardTemporaryDirectoryAlias(t *testing.T) {
	if runtime.GOOS != "darwin" {
		t.Skip("macOS standard temporary-directory alias regression")
	}
	// testing.TempDir creates its numbered child with 0777 before umask. The
	// hosted macOS umask leaves that parent at 0755, so apply the same explicit
	// owner-only setup required of a production cache parent.
	raw := filepath.Join(secureTempDir(t), "objects")
	cache, err := New(raw)
	if err != nil {
		t.Fatalf("New() through standard macOS temporary alias: %v", err)
	}
	t.Cleanup(func() {
		if err := cache.Close(); err != nil {
			t.Errorf("Close() error = %v", err)
		}
	})
	if strings.HasPrefix(raw, "/var/") && strings.HasPrefix(cache.root, "/var/") {
		t.Fatalf("cache root retained the /var alias: %q", cache.root)
	}
}

func TestSecureCreatedPathRepairsPermissiveUnixDirectory(t *testing.T) {
	dir := t.TempDir()
	if err := os.Chmod(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	info, err := os.Lstat(dir)
	if err != nil {
		t.Fatal(err)
	}
	if ownerOnlyPath(dir, info) {
		t.Fatal("permissive directory unexpectedly passed owner-only validation")
	}
	if err := secureCreatedPath(dir, true); err != nil {
		t.Fatalf("secureCreatedPath() error = %v", err)
	}
	if err := validateOwnerOnlyPath(dir, true); err != nil {
		t.Fatalf("secured directory did not pass owner-only validation: %v", err)
	}
}

func TestCanonicalizeTopLevelAliasRejectsUntrustedOwner(t *testing.T) {
	info, err := os.Lstat(t.TempDir())
	if err != nil {
		t.Fatal(err)
	}
	info = symlinkFileInfo{FileInfo: info}
	raw := filepath.Join(string(filepath.Separator), "untrusted-alias", "cache")

	_, err = canonicalizeTopLevelAliasWith(
		raw,
		func(string) (fs.FileInfo, error) { return info, nil },
		func(string) (string, error) {
			t.Fatal("readlink called for untrusted alias")
			return "", nil
		},
	)
	if err == nil {
		t.Fatal("canonicalizeTopLevelAliasWith() accepted an untrusted alias")
	}
}

func TestCanonicalizeTopLevelAliasLeavesNestedLinkForValidation(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "target")
	if err := os.Mkdir(target, 0o700); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(root, "nested-alias")
	if err := os.Symlink(target, alias); err != nil {
		t.Fatal(err)
	}
	raw := filepath.Join(alias, "objects")

	canonical, err := canonicalizeTopLevelAlias(raw)
	if err != nil {
		t.Fatalf("canonicalizeTopLevelAlias() error = %v", err)
	}
	canonicalRoot, err := canonicalizeTopLevelAlias(root)
	if err != nil {
		t.Fatalf("canonicalizeTopLevelAlias(root) error = %v", err)
	}
	want := filepath.Join(canonicalRoot, "nested-alias", "objects")
	if canonical != want {
		t.Fatalf("canonicalizeTopLevelAlias() = %q, want nested alias retained as %q", canonical, want)
	}
	if err := validatePathComponents(filepath.Dir(canonical), true); err == nil {
		t.Fatal("validatePathComponents() accepted the retained nested alias")
	}
}

type symlinkFileInfo struct {
	fs.FileInfo
}

func (info symlinkFileInfo) Mode() fs.FileMode {
	return info.FileInfo.Mode() | os.ModeSymlink
}
