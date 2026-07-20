package main

import (
	"crypto/sha256"
	"fmt"
	"os"
	"path/filepath"
	"testing"
)

func TestRunAcceptsExactDigestAndRejectsMissingOrStaleFiles(t *testing.T) {
	root := t.TempDir()
	artifact := filepath.Join(root, "artifact.bin")
	digest := filepath.Join(root, "artifact.sha256")
	bytes := []byte("exact artifact")
	want := sha256.Sum256(bytes)
	if err := os.WriteFile(artifact, bytes, 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(digest, []byte(fmt.Sprintf("%x\n", want)), 0o600); err != nil {
		t.Fatal(err)
	}

	if err := run(artifact, digest); err != nil {
		t.Fatalf("exact digest failed: %v", err)
	}
	if err := os.WriteFile(artifact, []byte("stale artifact"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := run(artifact, digest); err == nil {
		t.Fatal("stale artifact unexpectedly passed")
	}
	if err := os.Remove(artifact); err != nil {
		t.Fatal(err)
	}
	if err := run(artifact, digest); err == nil {
		t.Fatal("missing artifact unexpectedly passed")
	}
}
