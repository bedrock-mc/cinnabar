//go:build windows

package streamnet

import (
	"net"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestWindowsLivePublicationCannotBeStolen(t *testing.T) {
	dir := t.TempDir()
	live, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	defer live.Close()
	path := filepath.Join(dir, windowsEndpointName)
	if err := os.WriteFile(path, []byte(live.Addr().String()+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	listener, err := New(dir).Listen("")
	if err == nil {
		_ = listener.Close()
		t.Fatal("Listen() stole an active game.addr publication")
	}
	data, readErr := os.ReadFile(path)
	if readErr != nil {
		t.Fatalf("read live publication after refusal: %v", readErr)
	}
	if string(data) != live.Addr().String()+"\n" {
		t.Fatalf("live publication changed to %q", data)
	}
}

func TestWindowsStalePublicationIsRecoveredAtomically(t *testing.T) {
	dir := t.TempDir()
	stale, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	staleAddress := stale.Addr().String()
	if err := stale.Close(); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, windowsEndpointName)
	if err := os.WriteFile(path, []byte(staleAddress+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}

	listener, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("Listen() with stale publication: %v", err)
	}
	defer listener.Close()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	publication := string(data)
	if !strings.HasSuffix(publication, "\n") || strings.TrimSpace(publication) != listener.Addr().String() {
		t.Fatalf("publication = %q, listener = %q", publication, listener.Addr())
	}
}
