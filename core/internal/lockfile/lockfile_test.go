package lockfile

import (
	"errors"
	"io/fs"
	"os"
	"path/filepath"
	"testing"
)

func TestAcquireExistingDoesNotCreateMissingParent(t *testing.T) {
	parent := filepath.Join(t.TempDir(), "missing")
	lease, err := AcquireExisting(filepath.Join(parent, "lease"), 0)
	if lease != nil {
		_ = lease.Close()
		t.Fatal("AcquireExisting returned a lease for a missing parent")
	}
	if !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("AcquireExisting error = %v, want missing", err)
	}
	if _, err := os.Lstat(parent); !errors.Is(err, fs.ErrNotExist) {
		t.Fatalf("AcquireExisting created its missing parent: %v", err)
	}
}

func TestAcquireStillCreatesMissingParentAndFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "missing", "lease")
	lease, err := Acquire(path, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	if info, err := os.Lstat(path); err != nil || !info.Mode().IsRegular() {
		t.Fatalf("Acquire did not create a regular lease file: info=%v error=%v", info, err)
	}
}
