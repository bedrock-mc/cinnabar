//go:build !windows

package authcache

import (
	"io/fs"
	"os"
	"path/filepath"
	"syscall"
	"testing"
)

func TestCheckUnixCacheOwnershipRejectsGroupAndWorldAccess(t *testing.T) {
	tests := []struct {
		name string
		mode os.FileMode
		code string
	}{
		{name: "owner read-write", mode: 0o600},
		{name: "owner read-only", mode: 0o400},
		{name: "no access bits", mode: 0o000},
		{name: "group read", mode: 0o640, code: "group_or_world_access"},
		{name: "group write", mode: 0o620, code: "group_or_world_access"},
		{name: "group read-write", mode: 0o660, code: "group_or_world_access"},
		{name: "world read", mode: 0o604, code: "group_or_world_access"},
		{name: "world write", mode: 0o602, code: "group_or_world_access"},
		{name: "world read-write", mode: 0o666, code: "group_or_world_access"},
		{name: "group and world traverse", mode: 0o755, code: "group_or_world_access"},
	}
	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "microsoft-token.json")
			writeFile(t, path, []byte(`{"refresh_token":"sentinel"}`))
			if err := os.Chmod(path, tt.mode); err != nil {
				t.Fatal(err)
			}
			info, err := os.Lstat(path)
			if err != nil {
				t.Fatal(err)
			}

			err = checkUnixCacheOwnership(info)

			if tt.code == "" {
				if err != nil {
					t.Fatalf("checkUnixCacheOwnership() error = %v, want acceptance", err)
				}
				return
			}
			assertUnsafePermissionsCode(t, err, tt.code)
		})
	}
}

// fixedUIDFileInfo reports a real file's metadata while overriding the
// syscall owner identity, so foreign-owner rejection is exercised without
// privileged chown support.
type fixedUIDFileInfo struct {
	fs.FileInfo
	sys any
}

func (f fixedUIDFileInfo) Sys() any { return f.sys }

func TestCheckUnixCacheOwnershipRejectsForeignOwnerWithoutRoot(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeFile(t, path, []byte(`{"refresh_token":"sentinel"}`))
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}

	foreign := fixedUIDFileInfo{FileInfo: info, sys: &syscall.Stat_t{Uid: uint32(notCurrentUID())}}
	assertUnsafePermissionsCode(t, checkUnixCacheOwnership(foreign), "foreign_owner")

	if os.Geteuid() != 0 {
		t.Skip("real ownership change requires root; synthetic-owner rejection is proven above")
	}
	foreignPath := filepath.Join(t.TempDir(), "foreign.json")
	writeFile(t, foreignPath, []byte(`{"refresh_token":"sentinel"}`))
	if err := os.Chown(foreignPath, notCurrentUID(), os.Getegid()); err != nil {
		t.Skipf("chown to another user unavailable even as root: %v", err)
	}
	foreignInfo, err := os.Lstat(foreignPath)
	if err != nil {
		t.Fatal(err)
	}
	assertUnsafePermissionsCode(t, checkUnixCacheOwnership(foreignInfo), "foreign_owner")
}

func TestCheckUnixCacheOwnershipRejectsUnverifiableOwner(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeFile(t, path, []byte(`{"refresh_token":"sentinel"}`))
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}

	opaque := fixedUIDFileInfo{FileInfo: info}
	assertUnsafePermissionsCode(t, checkUnixCacheOwnership(opaque), "ownership_unverifiable")
}

// notCurrentUID returns one user id guaranteed to differ from this process.
func notCurrentUID() int {
	if os.Geteuid() == 0 {
		return 1
	}
	return 0
}
