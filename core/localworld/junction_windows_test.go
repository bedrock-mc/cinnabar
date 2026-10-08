//go:build windows

package localworld

import (
	"os"
	"path/filepath"
	"testing"
)

// The world link must survive folder names cmd.exe mangles, and unlinking must keep the world data.
func TestLinkWorldJunctionHandlesShellMetacharacters(t *testing.T) {
	root := filepath.Join(t.TempDir(), "A & B ^ C % D %PATH%")
	target := filepath.Join(root, "world & data")
	if err := os.MkdirAll(target, 0o700); err != nil {
		t.Fatal(err)
	}
	link := filepath.Join(root, "install", "worlds", "id ^&%")
	if err := linkWorld(link, target); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(link, "level.dat"), []byte("x"), 0o600); err != nil {
		t.Fatal(err)
	}
	if raw, err := os.ReadFile(filepath.Join(target, "level.dat")); err != nil || string(raw) != "x" {
		t.Fatalf("write through junction: %q, %v", raw, err)
	}
	unlinkWorld(link)
	if _, err := os.Lstat(link); !os.IsNotExist(err) {
		t.Fatalf("link survived: %v", err)
	}
	if _, err := os.Stat(filepath.Join(target, "level.dat")); err != nil {
		t.Fatalf("unlink removed world data: %v", err)
	}
}
