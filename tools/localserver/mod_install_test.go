package main

import (
	"bytes"
	"io"
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/world"
)

// TestInstalledModPackage exercises a real package produced by the Rust SDK integration test.
func TestInstalledModPackage(t *testing.T) {
	dir := os.Getenv("CINNABAR_TEST_CMOD_WORLD")
	if dir == "" {
		t.Skip("missing fixture: SDK-built .cmod installation")
	}
	socket, err := net.ListenPacket("udp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	addr := socket.LocalAddr().String()
	if err := socket.Close(); err != nil {
		t.Fatal(err)
	}
	var stdout, stderr bytes.Buffer
	err = run([]string{"-dir", dir, "-addr", addr, "-experience-runtime", os.Getenv("CINNABAR_EXPERIENCE_RUNTIME_BIN")}, strings.NewReader("stop\n"), &stdout, &stderr)
	if err != nil {
		t.Fatalf("load installed package: %v\n%s", err, stderr.String())
	}
	if stdout.String() != "ready\n" {
		t.Fatalf("server did not start: %q", stdout.String())
	}
	if _, ok := world.CustomBlocks()["space:cube"]; !ok {
		t.Fatal("declared block was not registered")
	}
	if _, ok := world.ItemByName("space:token", 0); !ok {
		t.Fatal("declared item was not registered")
	}
	if _, err := os.Stat(filepath.Join(dir, "resources", "cinnabar-extension-offer", "manifest.json")); err != nil {
		t.Fatal(err)
	}
}

// TestInstalledModSettings verifies that an ordinary world remains unchanged and incomplete mods fail closed.
func TestInstalledModSettings(t *testing.T) {
	s, err := parseSettings([]string{"-dir", t.TempDir(), "-addr", "127.0.0.1:19132"}, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	if err := s.loadMods(); err != nil {
		t.Fatal(err)
	}
	if s.runtime != "" || s.extensionKey != "" {
		t.Fatal("empty world enabled mods")
	}
	if err := os.MkdirAll(filepath.Join(s.dir, "mods", "broken"), 0o755); err != nil {
		t.Fatal(err)
	}
	if err := s.loadMods(); err == nil {
		t.Fatal("incomplete package accepted")
	}
}
