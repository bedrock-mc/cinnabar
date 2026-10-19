package coreapp

import (
	"bytes"
	"errors"
	"testing"
)

func TestElevatedCoreStopsBeforeOpeningServices(t *testing.T) {
	var stderr bytes.Buffer
	check := func() error { return errors.New("administrator token refused") }
	if code := runAtUserLevel(check, &stderr, func() int {
		t.Fatal("an elevated core must not open accounts or services")
		return 0
	}); code != 1 {
		t.Fatalf("exit code = %d, want 1", code)
	}
	if stderr.String() != "administrator token refused\n" {
		t.Fatalf("unexpected startup output: %q", stderr.String())
	}
}

func TestNormalCorePreservesExitCode(t *testing.T) {
	var stderr bytes.Buffer
	code := runAtUserLevel(func() error { return nil }, &stderr, func() int { return 7 })
	if code != 7 || stderr.Len() != 0 {
		t.Fatalf("exit = %d, stderr = %q", code, stderr.String())
	}
}
