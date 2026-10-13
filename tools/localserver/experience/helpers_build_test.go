package experience

import (
	"context"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"slices"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"
)

// helperBuildContext forwards interrupts and adds a deadline only for a nonzero budget.
// Stop it after all builds are reaped.
func helperBuildContext(parent context.Context, budget time.Duration) (context.Context, context.CancelFunc) {
	interrupted, stop := signal.NotifyContext(parent, os.Interrupt, syscall.SIGTERM)
	if budget == 0 {
		return interrupted, stop
	}
	ctx, cancel := context.WithTimeout(interrupted, budget)
	return ctx, func() {
		cancel()
		stop()
	}
}

// helperBuildCommand cancels the owned build tree; Run, Output or Wait reaps the command.
func helperBuildCommand(ctx context.Context, name string, args ...string) *exec.Cmd {
	cmd := exec.CommandContext(ctx, name, args...)
	cmd.WaitDelay = helperPipeWaitDelay
	configureHelperBuildCancellation(cmd)
	return cmd
}

// helperGuestBuildArgs reuses CI's guest intermediates or isolates them beside local artifacts.
func helperGuestBuildArgs(root string) []string {
	target := filepath.Join(root, "target", "experience-guests")
	buildDir := os.Getenv("EXPERIENCE_GUEST_BUILD_DIR")
	if buildDir == "" {
		buildDir = filepath.Join(target, "build")
	}
	return []string{
		"build", "--locked", "--target", "wasm32-unknown-unknown", "-p", "experience-probe",
		"--target-dir", target,
		"--config", "build.build-dir=" + strconv.Quote(filepath.ToSlash(buildDir)),
	}
}

func TestHelperGuestBuildDirectory(t *testing.T) {
	root := t.TempDir()
	// Native fixture intermediates must not override the guest's separate directory.
	t.Setenv("CARGO_BUILD_BUILD_DIR", filepath.Join(root, "host-build"))
	for _, test := range []struct {
		name       string
		configured string
		want       string
	}{
		{name: "local", want: filepath.Join(root, "target", "experience-guests", "build")},
		{name: "prewarmed", configured: "{workspace-root}/prewarmed-guests", want: "{workspace-root}/prewarmed-guests"},
		{name: "quoted path", configured: filepath.Join(root, "guest build \"cache\""), want: filepath.Join(root, "guest build \"cache\"")},
	} {
		t.Run(test.name, func(t *testing.T) {
			t.Setenv("EXPERIENCE_GUEST_BUILD_DIR", test.configured)
			args := helperGuestBuildArgs(root)
			i := slices.Index(args, "--config")
			if i < 0 || i+1 == len(args) || !strings.HasPrefix(args[i+1], "build.build-dir=") {
				t.Fatalf("guest build has no build directory override: %q", args)
			}
			buildDir, err := strconv.Unquote(strings.TrimPrefix(args[i+1], "build.build-dir="))
			if err != nil || buildDir != filepath.ToSlash(test.want) {
				t.Fatalf("guest build directory = %q, %v; want %q", buildDir, err, filepath.ToSlash(test.want))
			}
		})
	}
}

func TestHelperBuildContextDeadline(t *testing.T) {
	for _, test := range []struct {
		name         string
		budget       time.Duration
		wantDeadline bool
	}{
		{name: "disabled"},
		{name: "enabled", budget: time.Minute, wantDeadline: true},
	} {
		t.Run(test.name, func(t *testing.T) {
			ctx, cancel := helperBuildContext(context.Background(), test.budget)
			defer cancel()
			if _, deadline := ctx.Deadline(); deadline != test.wantDeadline {
				t.Fatalf("setup deadline = %v; want %v", deadline, test.wantDeadline)
			}
			if err := ctx.Err(); err != nil {
				t.Fatalf("new setup context is canceled: %v", err)
			}
		})
	}
}
