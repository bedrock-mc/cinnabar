// Package corelog configures the diagnostics every core binary shares: one key=value text logger
// on stderr whose records name the process that wrote them, and an optional file that also
// receives the runtime's fatal crash output.
package corelog

import (
	"flag"
	"fmt"
	"io"
	"log/slog"
	"os"
	"runtime/debug"
)

// LevelFlagName is the flag that selects the minimum record level.
const LevelFlagName = "log-level"

// ParseLevel maps a -log-level value (debug, info, warn or error) to its slog level.
func ParseLevel(name string) (slog.Level, error) {
	switch name {
	case "debug":
		return slog.LevelDebug, nil
	case "info":
		return slog.LevelInfo, nil
	case "warn":
		return slog.LevelWarn, nil
	case "error":
		return slog.LevelError, nil
	}
	return slog.LevelInfo, fmt.Errorf("unknown log level %q (want debug, info, warn or error)", name)
}

// LevelFlag registers -log-level on flags. level keeps its current value, info when zero, unless
// the flag is given; an unknown name fails parsing.
func LevelFlag(flags *flag.FlagSet, level *slog.Level) {
	flags.Func(LevelFlagName, "minimum level written to stderr: debug, info, warn or error (default info)", func(name string) error {
		parsed, err := ParseLevel(name)
		if err != nil {
			return err
		}
		*level = parsed
		return nil
	})
}

// New returns a logger writing text records at level and above to w, each carrying
// process=<process> before its own attributes.
func New(w io.Writer, process string, level slog.Level) *slog.Logger {
	return slog.New(slog.NewTextHandler(w, &slog.HandlerOptions{Level: level})).With("process", process)
}

// OpenCrashOutput opens path for appending, creating it readable and writable by its owner only.
func OpenCrashOutput(path string) (*os.File, error) {
	return os.OpenFile(path, os.O_WRONLY|os.O_CREATE|os.O_APPEND, 0o600)
}

// SetCrashOutput makes the runtime append every unhandled panic and fatal error, with the stacks of
// all goroutines, to the file at path in addition to stderr. It affects the whole process.
func SetCrashOutput(path string) error {
	file, err := OpenCrashOutput(path)
	if err != nil {
		return fmt.Errorf("open crash output: %w", err)
	}
	// The runtime keeps its own duplicate of the descriptor.
	defer func() { _ = file.Close() }()
	debug.SetTraceback("all")
	if err := debug.SetCrashOutput(file, debug.CrashOptions{}); err != nil {
		return fmt.Errorf("set crash output: %w", err)
	}
	return nil
}
