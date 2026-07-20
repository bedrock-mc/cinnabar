package main

import (
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
)

func TestRequireRealDirectoryAcceptsEquivalentShortPath(t *testing.T) {
	longPath := filepath.Join(t.TempDir(), "directory component with a deliberately long name")
	if err := os.Mkdir(longPath, 0o700); err != nil {
		t.Fatal(err)
	}
	shortPath := windowsShortPath(t, longPath)
	if shortPath == "" || strings.EqualFold(shortPath, longPath) || !strings.Contains(shortPath, "~") {
		t.Skip("8.3 short path aliases are unavailable on this volume")
	}
	if err := requireRealDirectory(shortPath); err != nil {
		t.Fatalf("equivalent short path was rejected: %v", err)
	}
}

func windowsShortPath(t *testing.T, path string) string {
	t.Helper()
	longPath, err := syscall.UTF16PtrFromString(path)
	if err != nil {
		t.Fatalf("encode long path: %v", err)
	}
	buffer := make([]uint16, 32_768)
	written, err := syscall.GetShortPathName(longPath, &buffer[0], uint32(len(buffer)))
	if err != nil {
		t.Fatalf("resolve short path: %v", err)
	}
	if written == 0 || written >= uint32(len(buffer)) {
		t.Fatalf("resolve short path returned invalid length %d", written)
	}
	return syscall.UTF16ToString(buffer[:written])
}

func TestRequireRealDirectoryAcceptsVolumeRoot(t *testing.T) {
	volume := filepath.VolumeName(t.TempDir())
	if volume == "" {
		t.Fatal("temporary directory has no Windows volume")
	}
	root := volume + string(filepath.Separator)
	if err := requireRealDirectory(root); err != nil {
		t.Fatalf("volume root was rejected: %v", err)
	}
}

func TestRequireRealDirectoryRejectsFinalJunction(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "target")
	if err := os.Mkdir(target, 0o700); err != nil {
		t.Fatal(err)
	}
	junction := filepath.Join(root, "junction")
	createTestJunction(t, junction, target)

	if err := requireRealDirectory(junction); err == nil {
		t.Fatal("final junction was accepted")
	}
}

func TestRequireRealDirectoryRejectsAncestorJunction(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "target")
	child := filepath.Join(target, "child")
	if err := os.MkdirAll(child, 0o700); err != nil {
		t.Fatal(err)
	}
	junction := filepath.Join(root, "junction")
	createTestJunction(t, junction, target)

	if err := requireRealDirectory(filepath.Join(junction, "child")); err == nil {
		t.Fatal("ancestor junction was accepted")
	}
}

func createTestJunction(t *testing.T, junction, target string) {
	t.Helper()
	output, err := exec.Command("cmd.exe", "/d", "/c", "mklink", "/J", junction, target).CombinedOutput()
	if err != nil {
		t.Skipf("directory junctions are unavailable: %v: %s", err, strings.TrimSpace(string(output)))
	}
}
