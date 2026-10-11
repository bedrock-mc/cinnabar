//go:build !windows

package lifeline

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"strconv"
	"syscall"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

const helperEnv = "LIFELINE_TEST_HELPER"

// TestMain lets the test binary act as a wedged core or as the client that spawns one.
func TestMain(m *testing.M) {
	switch os.Getenv(helperEnv) {
	case "core":
		ctx, _ := Start(context.Background(), Config{ParentGone: WatchParent(ParentFromEnv(), 20*time.Millisecond), Grace: 300 * time.Millisecond})
		fmt.Println("ready")
		<-ctx.Done()
		select {} // a shutdown wait that never observes ctx
	case "client":
		core := helper("core")
		core.Env = append(core.Env, fmt.Sprintf("%s=%d", ParentEnv, os.Getpid()))
		core.Stdout = os.Stdout
		if err := core.Start(); err != nil {
			os.Exit(2)
		}
		fmt.Println(core.Process.Pid)
		time.Sleep(time.Hour)
	}
	os.Exit(m.Run())
}

func helper(role string) *exec.Cmd {
	cmd := exec.Command(os.Args[0], "-test.run=^$")
	cmd.Env = append(os.Environ(), helperEnv+"="+role)
	return cmd
}

func alive(pid int) bool {
	return !errors.Is(syscall.Kill(pid, 0), syscall.ESRCH)
}

// A core whose client is SIGKILLed notices the reparent and exits even with a wedged shutdown.
func TestCoreExitsWhenItsClientIsKilled(t *testing.T) {
	client := helper("client")
	stdout, err := client.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := client.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = client.Process.Kill(); _ = client.Wait() })
	lines := bufio.NewScanner(stdout)
	var core int
	if lines.Scan() {
		core, _ = strconv.Atoi(lines.Text())
	}
	if core == 0 || !lines.Scan() || lines.Text() != "ready" {
		t.Fatal("client helper did not report a running core")
	}
	t.Cleanup(func() {
		if alive(core) {
			_ = syscall.Kill(core, syscall.SIGKILL)
		}
	})
	_ = client.Process.Kill()
	_ = client.Wait()
	if !testwait.WaitUntil(5*time.Second, func() bool { return !alive(core) }) {
		t.Fatalf("core %d outlived its killed client", core)
	}
}

// A core whose named parent is already gone at startup exits too.
func TestCoreExitsWhenItsClientDiedBeforeItStarted(t *testing.T) {
	core := helper("core")
	core.Env = append(core.Env, ParentEnv+"=999999")
	done := make(chan error, 1)
	if err := core.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = core.Process.Kill() })
	go func() { done <- core.Wait() }()
	select {
	case <-done:
	case <-time.After(3 * time.Second):
		t.Fatal("core outlived a parent that was gone before it started")
	}
}

// SIGTERM ends a core stuck mid-shutdown within the grace period.
func TestSIGTERMExitsWithinTheGrace(t *testing.T) {
	core := helper("core")
	stdout, err := core.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := core.Start(); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = core.Process.Kill() })
	if !bufio.NewScanner(stdout).Scan() {
		t.Fatal("core helper did not start")
	}
	started := time.Now()
	_ = core.Process.Signal(syscall.SIGTERM)
	waited := make(chan error, 1)
	go func() { waited <- core.Wait() }()
	select {
	case err := <-waited:
		var exit *exec.ExitError
		if !errors.As(err, &exit) || exit.ExitCode() != 1 {
			t.Fatalf("core exit = %v, want the hard-exit code 1", err)
		}
		if elapsed := time.Since(started); elapsed < 300*time.Millisecond {
			t.Fatalf("core exited after %v, before its grace", elapsed)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("core ignored SIGTERM")
	}
}
