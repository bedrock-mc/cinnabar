//go:build !windows

package main

import (
	"bufio"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"strconv"
	"strings"
	"syscall"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"

	"github.com/hashimthearab/rust-mcbe/core/internal/lifeline"
)

// Set in the test binary re-executed as a stand-in client: the path of the core to run as a
// launcher core, as the Rust client does, with the rest of the arguments after it.
const clientHelperEnv = "BEDROCK_CORE_TEST_CLIENT"

func init() {
	core := os.Getenv(clientHelperEnv)
	if core == "" {
		return
	}
	cmd := exec.Command(core, os.Args[len(os.Args)-5:]...)
	// The client's own stdin stays open in the test, so only the parent-death watch can end the core.
	cmd.Stdin = os.Stdin
	if log, err := os.Create(os.Getenv(clientHelperEnv + "_LOG")); err == nil {
		cmd.Stderr = log
	}
	cmd.Env = append(os.Environ(), fmt.Sprintf("%s=%d", lifeline.ParentEnv, os.Getpid()))
	if err := cmd.Start(); err != nil {
		os.Exit(2)
	}
	fmt.Println(cmd.Process.Pid)
	time.Sleep(time.Hour)
	os.Exit(3)
}

// A launcher core whose client is SIGKILLed exits within a few seconds, releasing its pack-cache lease.
func TestLauncherCoreExitsWhenItsClientIsKilled(t *testing.T) {
	if testing.Short() {
		t.Skip("builds bedrock-core")
	}
	dir := t.TempDir()
	core := filepath.Join(dir, "bedrock-core")
	if out, err := exec.Command("go", "build", "-o", core, ".").CombinedOutput(); err != nil {
		t.Fatalf("build bedrock-core: %v\n%s", err, out)
	}
	socketDir, packs := filepath.Join(dir, "s"), filepath.Join(dir, "packs", "objects")
	if err := os.MkdirAll(filepath.Dir(packs), 0o700); err != nil {
		t.Fatal(err)
	}
	client := exec.Command(os.Args[0], "-test.run=^$", "-socket-dir", socketDir, "-control-status", "-resource-pack-cache-dir", packs)
	log := filepath.Join(dir, "core.log")
	client.Env = append(os.Environ(), clientHelperEnv+"="+core, clientHelperEnv+"_LOG="+log)
	// Held open here (Cmd.StdinPipe would close it in Wait), so stdin EOF never ends the core.
	stdinRead, stdinWrite, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	defer stdinWrite.Close()
	client.Stdin = stdinRead
	stdout, err := client.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := client.Start(); err != nil {
		t.Fatal(err)
	}
	_ = stdinRead.Close()
	t.Cleanup(func() { _ = client.Process.Kill(); _ = client.Wait() })
	lines := bufio.NewScanner(stdout)
	if !lines.Scan() {
		t.Fatal("client helper did not start a core")
	}
	pid, err := strconv.Atoi(lines.Text())
	if err != nil {
		t.Fatalf("client helper reported %q", lines.Text())
	}
	alive := func() bool { return !errors.Is(syscall.Kill(pid, 0), syscall.ESRCH) }
	t.Cleanup(func() {
		if alive() {
			_ = syscall.Kill(pid, syscall.SIGKILL)
		}
	})
	testwait.Eventually(t, 10*time.Second, "the launcher core to publish its endpoint", func() bool {
		if text, _ := os.ReadFile(log); strings.Contains(string(text), "listener ready") {
			return true
		}
		if !alive() {
			t.Fatal("launcher core exited before publishing its endpoint")
		}
		return false
	})
	_ = client.Process.Kill()
	_ = client.Wait()
	if !testwait.WaitUntil(5*time.Second, func() bool { return !alive() }) {
		t.Fatalf("launcher core %d outlived its killed client", pid)
	}
}
