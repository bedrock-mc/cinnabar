//go:build windows

package proxy

import (
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"

	"golang.org/x/sys/windows"
)

func canonicalExistingPath(path string) (string, error) {
	pathPointer, err := windows.UTF16PtrFromString(path)
	if err != nil {
		return "", err
	}
	handle, err := windows.CreateFile(
		pathPointer,
		0,
		windows.FILE_SHARE_READ|windows.FILE_SHARE_WRITE|windows.FILE_SHARE_DELETE,
		nil,
		windows.OPEN_EXISTING,
		windows.FILE_FLAG_BACKUP_SEMANTICS,
		0,
	)
	if err != nil {
		return "", err
	}
	defer windows.CloseHandle(handle)

	buffer := make([]uint16, 512)
	for {
		length, err := windows.GetFinalPathNameByHandle(handle, &buffer[0], uint32(len(buffer)), 0)
		if err != nil {
			return "", err
		}
		if length < uint32(len(buffer)) {
			if length == 0 {
				return "", fmt.Errorf("empty final path for %s", path)
			}
			return filepath.Clean(windows.UTF16ToString(buffer[:length])), nil
		}
		buffer = make([]uint16, length+1)
	}
}

func TestValidateRuntimeSeparationResolvesJunctionParent(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "source")
	if err := os.Mkdir(source, 0o700); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(root, "source-junction")
	if output, err := exec.Command("cmd.exe", "/c", "mklink", "/J", alias, source).CombinedOutput(); err != nil {
		t.Skipf("directory junction unavailable: %v: %s", err, output)
	}
	t.Cleanup(func() {
		if err := os.Remove(alias); err != nil && !os.IsNotExist(err) {
			t.Errorf("remove junction: %v", err)
		}
	})

	runtimeDir := filepath.Join(alias, "not-yet-created")
	if _, _, err := validateRuntimeSeparation(source, runtimeDir); err == nil {
		t.Fatal("junction alias descendant was accepted")
	}
	if _, err := os.Stat(runtimeDir); !os.IsNotExist(err) {
		t.Fatalf("validation mutated junction target: %v", err)
	}
}
