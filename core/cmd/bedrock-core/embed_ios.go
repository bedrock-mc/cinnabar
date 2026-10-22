//go:build ios

package main

// iOS starts no processes, so the client links this package as a C archive
// (`go build -buildmode=c-archive`) and runs each core it would spawn on a thread of its own.

import "C"

import (
	"context"
	"io"
	"log/slog"
	"unsafe"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

func init() {
	// Every core shares the host process: a process-wide default logger or crash output would
	// follow whichever core started last, so each core logs only through its own stderr.
	installDiagnostics = func(options, *slog.Logger) {}
}

// CinnabarCoreInit returns once the Go runtime has initialized. A C archive initializes it on a
// thread of its own, and on iOS that initialization moves the working directory to the app
// bundle, so the host changes the working directory only after this returns.
//
//export CinnabarCoreInit
func CinnabarCoreInit() {}

// CinnabarCoreRun runs one core with the argc arguments in argv (no program name) and returns
// its exit code. It adopts the descriptors, which stand in for the core's stdin, stdout and
// stderr (-1 for none), and cancel, whose EOF stops the core; see runEmbedded.
//
//export CinnabarCoreRun
func CinnabarCoreRun(argc C.int, argv **C.char, stdinFD, stdoutFD, stderrFD, cancelFD C.int) C.int {
	args := make([]string, int(argc))
	for index, arg := range unsafe.Slice(argv, int(argc)) {
		args[index] = C.GoString(arg)
	}
	streams := embeddedStreams{
		stdin:  embeddedFile(int(stdinFD), "stdin"),
		stdout: embeddedFile(int(stdoutFD), "stdout"),
		stderr: embeddedFile(int(stderrFD), "stderr"),
		cancel: embeddedFile(int(cancelFD), "cancel"),
	}
	return C.int(runEmbedded(args, streams, shutdownGrace, func(ctx context.Context, args []string, stdout, stderr io.Writer) int {
		return execute(ctx, args, stdout, stderr, authcache.Source, proxy.Serve)
	}))
}
