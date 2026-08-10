//go:build !windows

package streamnet

import (
	"bytes"
	"errors"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestUnixLongSocketDirectoryUsesStableLengthSafeEndpoint(t *testing.T) {
	dir := filepath.Join(t.TempDir(), strings.Repeat("macos-runner-segment-", 8))
	listener, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("Listen() with long socket directory: %v", err)
	}
	defer listener.Close()

	network, address, err := Resolve(dir)
	if err != nil {
		t.Fatalf("Resolve() with long socket directory: %v", err)
	}
	if network != "unix" {
		t.Fatalf("network = %q, want unix", network)
	}
	first := unixEndpointPath(dir)
	second := unixEndpointPath(dir)
	if address != first || first != second {
		t.Fatalf("endpoint is not stable: got %q, then %q, want resolved %q", first, second, address)
	}
	if len(address) > maxUnixEndpointPathBytes {
		t.Fatalf("endpoint length = %d, want <= %d: %q", len(address), maxUnixEndpointPathBytes, address)
	}
	if !strings.HasPrefix(address, "/tmp/cinnabar-") {
		t.Fatalf("long endpoint = %q, want bounded /tmp path", address)
	}
}

func TestUnixLongEndpointDerivationMatchesRustBridge(t *testing.T) {
	dir := filepath.Join("/var/folders/zz", strings.Repeat("macos-runner-segment-", 8))
	want := "/tmp/cinnabar-7b260d1b166f7db809ce8c3d8bd42d1a.sock"
	if got := unixEndpointPath(dir); got != want {
		t.Fatalf("unixEndpointPath() = %q, want shared Rust endpoint %q", got, want)
	}
}

func TestUnixEndpointLexicalNormalizationMatchesRustBridge(t *testing.T) {
	repeatedParent := "/tmp/" + strings.Repeat("segment/../", 12)
	invalidBytes := append([]byte("/tmp/\xff/"), bytes.Repeat([]byte{'x'}, 100)...)
	vectors := []struct {
		name      string
		socketDir string
		want      string
	}{
		{
			name:      "duplicate separators and dot components",
			socketDir: "/tmp//alpha/./beta/../gamma",
			want:      "/tmp/alpha/gamma/game.sock",
		},
		{
			name:      "long raw path normalizes below limit",
			socketDir: repeatedParent,
			want:      "/tmp/game.sock",
		},
		{
			name:      "exact direct limit",
			socketDir: "/" + strings.Repeat("a", 92),
			want:      "/" + strings.Repeat("a", 92) + "/game.sock",
		},
		{
			name:      "first hashed length",
			socketDir: "/" + strings.Repeat("a", 93),
			want:      "/tmp/cinnabar-d32a5982698ad8de34829c65f893edf6.sock",
		},
		{
			name:      "unicode bytes",
			socketDir: "/tmp/" + strings.Repeat("路径/", 20),
			want:      "/tmp/cinnabar-08390d1ff13834e20abadae40eff1ce0.sock",
		},
		{
			name:      "non UTF-8 bytes",
			socketDir: string(invalidBytes),
			want:      "/tmp/cinnabar-32ec4a93b88918d1547cfbaf69f63a13.sock",
		},
	}
	for _, vector := range vectors {
		t.Run(vector.name, func(t *testing.T) {
			if got := unixEndpointPath(vector.socketDir); got != vector.want {
				t.Fatalf("unixEndpointPath() = %q, want shared Rust endpoint %q", got, vector.want)
			}
		})
	}
}

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
	path := unixEndpointPath(dir)
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
