//go:build !windows

package streamnet

import (
	"errors"
	"net"
	"os"
	"path/filepath"
	"testing"
)

func TestUnixActiveListenerCannotBeStolen(t *testing.T) {
	dir := t.TempDir()
	first, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("first Listen(): %v", err)
	}
	defer first.Close()

	second, err := New(dir).Listen("")
	if err == nil {
		_ = second.Close()
		t.Fatal("second Listen() stole an active Unix socket")
	}
}

func TestUnixOldListenerCannotDeleteSuccessorSocket(t *testing.T) {
	dir := t.TempDir()
	old, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("old Listen(): %v", err)
	}
	path := filepath.Join(dir, unixEndpointName)
	moved := path + ".old"
	if err := os.Rename(path, moved); err != nil {
		t.Fatalf("move old socket: %v", err)
	}
	defer os.Remove(moved)

	successor, err := net.Listen("unix", path)
	if err != nil {
		t.Fatalf("successor Listen(): %v", err)
	}
	if unix, ok := successor.(*net.UnixListener); ok {
		unix.SetUnlinkOnClose(false)
	}
	defer func() {
		_ = successor.Close()
		_ = os.Remove(path)
	}()

	closeErr := old.Close()
	if closeErr == nil {
		t.Fatal("old Close() did not report the changed endpoint identity")
	}
	info, err := os.Lstat(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			t.Fatal("old Close() deleted the successor socket")
		}
		t.Fatal(err)
	}
	if info.Mode()&os.ModeSocket == 0 {
		t.Fatalf("successor path mode = %v, want socket", info.Mode())
	}
}
