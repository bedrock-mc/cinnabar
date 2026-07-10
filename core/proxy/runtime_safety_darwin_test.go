//go:build darwin

package proxy

import (
	"os"
	"path/filepath"
	"testing"
)

func TestValidateRuntimeSeparationRejectsDarwinCaseVariantParent(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "BedrockSource")
	if err := os.Mkdir(source, 0o700); err != nil {
		t.Fatal(err)
	}
	caseVariant := filepath.Join(root, "bedrocksource")
	actualInfo, err := os.Stat(source)
	if err != nil {
		t.Fatal(err)
	}
	variantInfo, err := os.Stat(caseVariant)
	if os.IsNotExist(err) {
		t.Skip("test volume is case-sensitive")
	}
	if err != nil {
		t.Fatalf("stat case-variant source: %v", err)
	}
	if !os.SameFile(actualInfo, variantInfo) {
		t.Skip("case variant does not resolve to the same directory")
	}

	runtimeDir := filepath.Join(caseVariant, "not-yet-created")
	if _, _, err := validateRuntimeSeparation(source, runtimeDir); err == nil {
		t.Fatal("case-variant source descendant was accepted as a separate runtime")
	}
}
