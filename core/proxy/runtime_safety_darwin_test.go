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

func TestRuntimeOwnershipMarkerDistinguishesDarwinCaseSensitiveSources(t *testing.T) {
	root := t.TempDir()
	firstSource := filepath.Join(root, "BedrockSource")
	secondSource := filepath.Join(root, "bedrocksource")
	if err := os.Mkdir(firstSource, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(secondSource, 0o700); os.IsExist(err) {
		t.Skip("test volume is case-insensitive")
	} else if err != nil {
		t.Fatalf("create case-only source: %v", err)
	}

	canonicalFirst, err := canonicalExistingPath(firstSource)
	if err != nil {
		t.Fatal(err)
	}
	canonicalSecond, err := canonicalExistingPath(secondSource)
	if err != nil {
		t.Fatal(err)
	}
	if runtimeOwnershipMarker(canonicalFirst) == runtimeOwnershipMarker(canonicalSecond) {
		t.Fatal("distinct case-sensitive Darwin sources produced the same ownership marker")
	}

	name := "bedrock_server.test"
	if err := os.WriteFile(filepath.Join(firstSource, name), []byte("first source"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(secondSource, name), []byte("second source"), 0o700); err != nil {
		t.Fatal(err)
	}
	runtimeDir := filepath.Join(root, "runtime")
	if _, err := prepareStableRuntime(firstSource, runtimeDir, name); err != nil {
		t.Fatalf("prepare first source: %v", err)
	}
	if _, err := prepareStableRuntime(secondSource, runtimeDir, name); err == nil {
		t.Fatal("runtime ownership marker accepted a distinct case-only Darwin source")
	}
}
