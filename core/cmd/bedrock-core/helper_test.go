package main

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"strconv"
	"testing"
	"time"
)

func TestHelperModeIgnoresProxyArgs(t *testing.T) {
	if handled, _ := helperMode(context.Background(), []string{"-upstream", "x:1"}, &bytes.Buffer{}, &bytes.Buffer{}); handled {
		t.Fatal("proxy arguments must not select helper mode")
	}
}

func TestCheckUpdateRejectsMissingConfiguration(t *testing.T) {
	var stderr bytes.Buffer
	handled, code := helperMode(context.Background(), []string{"check-update"}, &bytes.Buffer{}, &stderr)
	if !handled || code != 1 {
		t.Fatalf("handled=%v code=%d stderr=%s", handled, code, stderr.String())
	}
}

// Crash reports stay on disk; no helper may upload them.
func TestUploadCrashIsNotAHelper(t *testing.T) {
	if handled, _ := helperMode(context.Background(), []string{"upload-crash", "-file", "x"}, &bytes.Buffer{}, &bytes.Buffer{}); handled {
		t.Fatal("upload-crash must not be a helper")
	}
}

// TestApplyUpdateRequiresParent proves a detached helper cannot silently skip the exit barrier.
func TestApplyUpdateRequiresParent(t *testing.T) {
	if err := runApplyUpdate(context.Background(), nil, &bytes.Buffer{}); err == nil {
		t.Fatal("apply accepted a missing parent PID")
	}
}

// TestApplyWaitsForPipeAndLivingParent verifies both barriers without any staged install.
func TestApplyWaitsForPipeAndLivingParent(t *testing.T) {
	input, output := io.Pipe()
	defer input.Close()
	defer output.Close()
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { done <- runApplyUpdate(ctx, []string{"--parent-pid", strconv.Itoa(os.Getppid())}, input) }()
	select {
	case err := <-done:
		t.Fatalf("helper passed live stdin: %v", err)
	case <-time.After(20 * time.Millisecond):
	}
	if err := output.Close(); err != nil {
		t.Fatal(err)
	}
	select {
	case err := <-done:
		t.Fatalf("helper passed a living parent: %v", err)
	case <-time.After(20 * time.Millisecond):
	}
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("expected parent-wait cancellation, got %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("helper did not stop waiting")
	}
}

// TestParentPipeWaitCanBeCancelled proves cancellation never requires the parent to exit.
func TestParentPipeWaitCanBeCancelled(t *testing.T) {
	input, output := io.Pipe()
	defer input.Close()
	defer output.Close()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if err := waitForInputClose(ctx, input); !errors.Is(err, context.Canceled) {
		t.Fatal(err)
	}
}
