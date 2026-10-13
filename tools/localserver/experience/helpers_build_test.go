package experience

import (
	"context"
	"fmt"
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

// helperBuildContext cancels setup on timeout or interrupt; stop it after all builds are reaped.
func helperBuildContext(parent context.Context, budget time.Duration) (context.Context, context.CancelFunc) {
	interrupted, stop := signal.NotifyContext(parent, os.Interrupt, syscall.SIGTERM)
	ctx, cancel := context.WithTimeout(interrupted, budget)
	return ctx, func() {
		cancel()
		stop()
	}
}

// helperBuildJobs resolves the configured Cargo budget, including CPU-relative values.
func helperBuildJobs(cpus int, configured string) (int, error) {
	if configured == "" || configured == "default" {
		return cpus, nil
	}
	jobs, err := strconv.Atoi(configured)
	if err != nil || jobs == 0 {
		return 0, fmt.Errorf("invalid CARGO_BUILD_JOBS %q: expected a nonzero integer or default", configured)
	}
	if jobs < 0 {
		jobs = max(cpus+jobs, 1)
	}
	return jobs, nil
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
		"--jobs", "1", "--target-dir", target,
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

func TestHelperBuildJobs(t *testing.T) {
	for _, test := range []struct {
		configured string
		want       int
		wantErr    bool
	}{
		{configured: "", want: 8},
		{configured: "default", want: 8},
		{configured: "1", want: 1},
		{configured: "2", want: 2},
		{configured: "4", want: 4},
		{configured: "16", want: 16},
		{configured: "-2", want: 6},
		{configured: "-16", want: 1},
		{configured: "0", wantErr: true},
		{configured: "invalid", wantErr: true},
	} {
		t.Run(test.configured, func(t *testing.T) {
			jobs, err := helperBuildJobs(8, test.configured)
			if jobs != test.want || (err != nil) != test.wantErr {
				t.Fatalf("helperBuildJobs(8, %q) = %d, %v; want %d, error %v", test.configured, jobs, err, test.want, test.wantErr)
			}
		})
	}
}
