//go:build !windows

package main

import (
	"context"
	"io"
	"os"
	"sync"
	"syscall"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/corelog"
	"github.com/hashimthearab/rust-mcbe/core/internal/lifeline"
)

// embeddedStreams stand in for an in-process core's standard streams; nil is none.
type embeddedStreams struct {
	stdin, stdout, stderr *os.File
	// EOF stops the core as SIGTERM stops a process; the host keeps the write end until then.
	cancel *os.File
}

// embeddedFile adopts a descriptor the host handed over, or returns nil for a negative one. The
// descriptor is made non-blocking so closing it also ends a read blocked on it.
func embeddedFile(fd int, name string) *os.File {
	if fd < 0 {
		return nil
	}
	_ = syscall.SetNonblock(fd, true)
	return os.NewFile(uintptr(fd), name)
}

var embeddedRuntime sync.Once

// runEmbedded runs one core inside a host process that cannot start processes (the iOS C
// archive): stdin EOF (unless the arguments leave stdin unbound) and cancel EOF begin shutdown as
// they would for a child, signals stay with the host, and a shutdown that outlives grace returns
// 1 while its goroutines finish in the background instead of exiting the host. Every stream is
// closed on return, so the host sees EOF on stdout and stderr once the core is done.
func runEmbedded(
	args []string,
	streams embeddedStreams,
	grace time.Duration,
	execute func(ctx context.Context, args []string, stdout, stderr io.Writer) int,
) int {
	embeddedRuntime.Do(func() { configureRuntime(os.Getenv) })
	defer func() {
		for _, file := range []*os.File{streams.stdin, streams.stdout, streams.stderr, streams.cancel} {
			if file != nil {
				_ = file.Close()
			}
		}
	}()
	stdout, stderr := io.Discard, io.Discard
	if streams.stdout != nil {
		stdout = streams.stdout
	}
	if streams.stderr != nil {
		stderr = streams.stderr
	}
	parent, cancel := context.WithCancel(context.Background())
	defer cancel()
	if streams.cancel != nil {
		go func() {
			_, _ = io.Copy(io.Discard, streams.cancel)
			cancel()
		}()
	}
	var stdin io.Reader
	if streams.stdin != nil && bindsStdin(args) {
		stdin = streams.stdin
	}
	// Invalid arguments leave opts zero, as run logs them: bedrock-core at info.
	opts, _ := parseFlags(args, io.Discard)
	abandoned := make(chan struct{})
	ctx, disarm := lifeline.Start(parent, lifeline.Config{
		Stdin:   stdin,
		Signals: make(chan os.Signal),
		Grace:   grace,
		Log:     corelog.New(stderr, opts.process(), opts.logLevel),
		Stderr:  stderr,
		Exit:    func(int) { close(abandoned) },
	})
	defer disarm()
	done := make(chan int, 1)
	go func() { done <- execute(ctx, args, stdout, stderr) }()
	select {
	case code := <-done:
		return code
	case <-abandoned:
		return 1
	}
}
