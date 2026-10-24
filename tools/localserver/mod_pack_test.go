package main

import (
	"archive/zip"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

// TestExportRegisteredPack exercises the offline server lifecycle in its own registration process.
func TestExportRegisteredPack(t *testing.T) {
	const childOutput = "CINNABAR_TEST_EXPORT_MOD_PACK"
	if out := os.Getenv(childOutput); out != "" {
		if err := exportRegisteredPack(out, "empty_mod"); err != nil {
			t.Fatal(err)
		}
		return
	}
	executable, err := os.Executable()
	if err != nil {
		t.Fatal(err)
	}
	out := filepath.Join(t.TempDir(), "resource.mcpack")
	command := exec.CommandContext(t.Context(), executable, "-test.run=^TestExportRegisteredPack$")
	command.Env = append(os.Environ(), childOutput+"="+out)
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("export registered pack: %v\n%s", err, output)
	}
	archive, err := zip.OpenReader(out)
	if err != nil {
		t.Fatal(err)
	}
	defer archive.Close()
	for _, entry := range archive.File {
		if entry.Name == "manifest.json" {
			return
		}
	}
	t.Fatal("exported pack has no manifest")
}
