package corelog

import (
	"bytes"
	"context"
	"errors"
	"flag"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"
	"regexp"
	"runtime"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/testwait"
)

const crashFixtureEnv = "CORELOG_CRASH_FIXTURE"

func TestParseLevelAcceptsOnlyTheFourNames(t *testing.T) {
	for name, want := range map[string]slog.Level{
		"debug": slog.LevelDebug,
		"info":  slog.LevelInfo,
		"warn":  slog.LevelWarn,
		"error": slog.LevelError,
	} {
		if got, err := ParseLevel(name); err != nil || got != want {
			t.Fatalf("ParseLevel(%q) = %v, %v; want %v", name, got, err, want)
		}
	}
	for _, name := range []string{"", "INFO", "warning", "info+2", "trace"} {
		if _, err := ParseLevel(name); err == nil {
			t.Fatalf("ParseLevel(%q) accepted an unknown level", name)
		}
	}
}

func TestLevelFlagDefaultsToInfoAndRejectsUnknownNames(t *testing.T) {
	parse := func(args ...string) (slog.Level, error) {
		flags := flag.NewFlagSet("fixture", flag.ContinueOnError)
		flags.SetOutput(io.Discard)
		var level slog.Level
		LevelFlag(flags, &level)
		return level, flags.Parse(args)
	}
	if level, err := parse(); err != nil || level != slog.LevelInfo {
		t.Fatalf("default level = %v, %v; want info", level, err)
	}
	if level, err := parse("-log-level", "debug"); err != nil || level != slog.LevelDebug {
		t.Fatalf("-log-level debug = %v, %v", level, err)
	}
	if level, err := parse("-log-level=error"); err != nil || level != slog.LevelError {
		t.Fatalf("-log-level=error = %v, %v", level, err)
	}
	if _, err := parse("-log-level", "loud"); err == nil {
		t.Fatal("an unknown level parsed")
	}
}

// Every record, including those of derived loggers, has one text format labelled with the process.
func TestNewWritesUniformProcessLabelledRecords(t *testing.T) {
	var output bytes.Buffer
	logger := New(&output, "game", slog.LevelWarn)
	logger.Info("below the level")
	logger.Warn("upstream slow", "elapsed_ms", 12)
	logger.With("component", "upstream-dialer").Error("dial failed", "attempt", 2)
	lines := strings.Split(strings.TrimSuffix(output.String(), "\n"), "\n")
	want := []*regexp.Regexp{
		regexp.MustCompile(`^time=\S+ level=WARN msg="upstream slow" process=game elapsed_ms=12$`),
		regexp.MustCompile(`^time=\S+ level=ERROR msg="dial failed" process=game component=upstream-dialer attempt=2$`),
	}
	if len(lines) != len(want) {
		t.Fatalf("records = %q, want %d", lines, len(want))
	}
	for index, pattern := range want {
		if !pattern.MatchString(lines[index]) {
			t.Fatalf("record %d = %q, want %s", index, lines[index], pattern)
		}
	}
}

func TestOpenCrashOutputAppendsToAnOwnerOnlyFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "core-crash.txt")
	for _, chunk := range []string{"first\n", "second\n"} {
		file, err := OpenCrashOutput(path)
		if err != nil {
			t.Fatal(err)
		}
		if _, err := file.WriteString(chunk); err != nil {
			t.Fatal(err)
		}
		if err := file.Close(); err != nil {
			t.Fatal(err)
		}
	}
	contents, err := os.ReadFile(path)
	if err != nil || string(contents) != "first\nsecond\n" {
		t.Fatalf("crash output = %q, %v; want both writes appended", contents, err)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	// Windows reports only the read-only attribute, not owner-only access.
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("crash output mode = %v, want 0600", info.Mode().Perm())
	}
}

// An unhandled panic lands in the crash output with every goroutine's stack, not only the panicking one.
func TestCrashOutputReceivesUnhandledPanics(t *testing.T) {
	if path := os.Getenv(crashFixtureEnv); path != "" {
		if err := SetCrashOutput(path); err != nil {
			t.Fatal(err)
		}
		go func() { panic("crash fixture panic") }()
		// The unrecovered panic ends the process; this goroutine shows in an all-goroutine trace.
		select {}
	}
	path := filepath.Join(t.TempDir(), "core-crash.txt")
	ctx, cancel := context.WithTimeout(context.Background(), testwait.DefaultTimeout)
	defer cancel()
	fixture := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestCrashOutputReceivesUnhandledPanics$")
	fixture.Env = append(os.Environ(), crashFixtureEnv+"="+path)
	output, err := fixture.CombinedOutput()
	var exit *exec.ExitError
	if !errors.As(err, &exit) || exit.ExitCode() != 2 {
		t.Fatalf("fixture exit = %v, want the runtime's panic status 2\n%s", err, output)
	}
	crash, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"panic: crash fixture panic", "goroutine 1 ["} {
		if !bytes.Contains(crash, []byte(want)) {
			t.Fatalf("crash output lacks %q:\n%s", want, crash)
		}
	}
}
