// Command crashcore stands in for bedrock-core in Rust process-supervision tests. It accepts the
// core's diagnostics flags and logs through corelog like the core, writes one raw stderr line,
// then ends as -mode says: clean exits 0, hold exits 0 once stdin closes like the core, fail logs
// "core failed" and exits 1 like the core's own error path, and panic dies in an unrecovered
// goroutine panic.
package main

import (
	"flag"
	"fmt"
	"io"
	"log/slog"
	"os"

	"github.com/hashimthearab/rust-mcbe/core/internal/corelog"
)

func main() {
	flags := flag.NewFlagSet("crashcore", flag.ExitOnError)
	level := slog.LevelInfo
	corelog.LevelFlag(flags, &level)
	crashOutput := flags.String("crash-output", "", "file that also receives the runtime's crash output")
	mode := flags.String("mode", "clean", "clean, hold, fail or panic")
	process := flags.String("process", "fixture", "process attribute of every record")
	_ = flags.Parse(os.Args[1:])
	logger := corelog.New(os.Stderr, *process, level)
	if *crashOutput != "" {
		if err := corelog.SetCrashOutput(*crashOutput); err != nil {
			logger.Error("crash output unavailable", "error", err)
			os.Exit(3)
		}
	}
	logger.Debug("fixture debug record")
	logger.Info("fixture started", "mode", *mode)
	fmt.Fprintln(os.Stderr, "raw fixture line")
	switch *mode {
	case "hold":
		_, _ = io.Copy(io.Discard, os.Stdin)
	case "fail":
		logger.Error("core failed")
		os.Exit(1)
	case "panic":
		go func() { panic("fixture core panic") }()
		select {}
	}
}
