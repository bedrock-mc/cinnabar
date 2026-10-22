//go:build !windows

package main

import (
	"bytes"
	"context"
	"io"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

// pipe returns both ends of a pipe; the test closes whichever end runEmbedded does not adopt.
func pipe(t *testing.T) (*os.File, *os.File) {
	t.Helper()
	read, write, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		_ = read.Close()
		_ = write.Close()
	})
	return read, write
}

// runAsync starts runEmbedded with fresh stdin and cancel pipes and returns their host ends,
// the host end of stderr and the exit code once it returns.
func runAsync(t *testing.T, args []string, grace time.Duration, execute func(context.Context, []string, io.Writer, io.Writer) int) (stdin, cancel *os.File, stderr *os.File, code <-chan int) {
	t.Helper()
	stdinRead, stdinWrite := pipe(t)
	cancelRead, cancelWrite := pipe(t)
	stderrRead, stderrWrite := pipe(t)
	exit := make(chan int, 1)
	go func() {
		exit <- runEmbedded(args, embeddedStreams{stdin: stdinRead, stderr: stderrWrite, cancel: cancelRead}, grace, execute)
	}()
	return stdinWrite, cancelWrite, stderrRead, exit
}

func untilCancelled(ctx context.Context, _ []string, _, _ io.Writer) int {
	<-ctx.Done()
	return 0
}

func exitCode(t *testing.T, code <-chan int) int {
	t.Helper()
	select {
	case value := <-code:
		return value
	case <-time.After(testwait.DefaultTimeout):
		t.Fatal("embedded core did not return")
		return -1
	}
}

// Closing the host's stdin ends an embedded core the way stdin EOF ends a spawned one.
func TestEmbeddedCoreStopsOnStdinEOF(t *testing.T) {
	stdin, _, _, code := runAsync(t, []string{"-socket-dir", "run"}, time.Minute, untilCancelled)
	_ = stdin.Close()
	if got := exitCode(t, code); got != 0 {
		t.Fatalf("exit code = %d, want 0", got)
	}
}

// The sign-in helper never binds stdin, so only cancel stops it, as a signal stops the process.
func TestEmbeddedSignInIgnoresStdinAndStopsOnCancel(t *testing.T) {
	contexts := make(chan context.Context, 1)
	stdin, cancel, _, code := runAsync(t, []string{"-auth-events", "-auth-cache", "cache.json"}, time.Minute,
		func(ctx context.Context, args []string, stdout, stderr io.Writer) int {
			contexts <- ctx
			return untilCancelled(ctx, args, stdout, stderr)
		})
	ctx := <-contexts
	_ = stdin.Close()
	if testwait.WaitUntil(100*time.Millisecond, func() bool { return ctx.Err() != nil }) {
		t.Fatal("stdin EOF stopped the sign-in helper")
	}
	_ = cancel.Close()
	if got := exitCode(t, code); got != 0 {
		t.Fatalf("exit code = %d, want 0", got)
	}
}

// A core that ignores its context past the grace returns 1 instead of exiting the host, and its
// stderr reaches EOF with the shutdown records labelled by its role.
func TestEmbeddedCoreOutlivingGraceIsAbandonedNotExited(t *testing.T) {
	release := make(chan struct{})
	defer close(release)
	_, cancel, stderr, code := runAsync(t, []string{"-socket-dir", "run", "-upstream", "localhost:19132"}, 10*time.Millisecond,
		func(context.Context, []string, io.Writer, io.Writer) int {
			<-release
			return 0
		})
	_ = cancel.Close()
	if got := exitCode(t, code); got != 1 {
		t.Fatalf("exit code = %d, want 1", got)
	}
	var log bytes.Buffer
	if _, err := io.Copy(&log, stderr); err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{`msg="core shutting down"`, `msg="core exiting"`, "process=game"} {
		if !strings.Contains(log.String(), want) {
			t.Fatalf("stderr lacks %s:\n%s", want, log.String())
		}
	}
}
