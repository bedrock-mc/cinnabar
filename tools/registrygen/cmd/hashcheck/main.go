package main

import (
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"strings"
)

func main() {
	artifact := flag.String("file", "", "artifact to verify")
	digest := flag.String("sha256-file", "", "file containing the expected lowercase SHA-256")
	flag.Parse()
	if err := run(*artifact, *digest); err != nil {
		fmt.Fprintln(os.Stderr, "hashcheck:", err)
		os.Exit(1)
	}
}

func run(artifactPath, digestPath string) error {
	if artifactPath == "" || digestPath == "" {
		return errors.New("both -file and -sha256-file are required")
	}
	expectedBytes, err := os.ReadFile(digestPath)
	if err != nil {
		return fmt.Errorf("read expected SHA-256: %w", err)
	}
	expected := strings.TrimSpace(string(expectedBytes))
	decoded, err := hex.DecodeString(expected)
	if err != nil || len(decoded) != sha256.Size || strings.ToLower(expected) != expected {
		return errors.New("expected SHA-256 must be exactly 64 lowercase hexadecimal characters")
	}
	artifact, err := os.Open(artifactPath)
	if err != nil {
		return fmt.Errorf("open artifact: %w", err)
	}
	defer artifact.Close()
	digest := sha256.New()
	if _, err := io.Copy(digest, artifact); err != nil {
		return fmt.Errorf("hash artifact: %w", err)
	}
	actual := hex.EncodeToString(digest.Sum(nil))
	if actual != expected {
		return fmt.Errorf("SHA-256 mismatch: expected %s, got %s", expected, actual)
	}
	return nil
}
