// Package lifeline bounds the core's lifetime by its parent client and by shutdown signals, so a
// wedged shutdown can never leave an orphan holding the auth lease or the pack-cache lock.
package lifeline

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/signal"
	"runtime/pprof"
	"strconv"
	"sync"
	"syscall"
	"time"
)

// Config selects the shutdown triggers; zero fields take the process defaults. A nil Log records
// through slog.Default() as it stands at each record, so a default installed after Start applies.
type Config struct {
	Stdin      io.Reader       // EOF cancels; nil when the parent does not pipe stdin
	ParentGone <-chan struct{} // closed once the parent process has died
	Signals    <-chan os.Signal
	Grace      time.Duration // after cancellation, how long shutdown may run before a hard exit
	Log        *slog.Logger  // records why shutdown began and why the process hard-exits
	Stderr     io.Writer     // receives the goroutine dump when shutdown outlives its grace
	Exit       func(int)
}

// Start returns a context cancelled by the first trigger. A later signal, or the grace period
// running out, exits the process; stop disarms both once shutdown has finished.
func Start(parent context.Context, cfg Config) (context.Context, func()) {
	if cfg.Stderr == nil {
		cfg.Stderr = os.Stderr
	}
	if cfg.Exit == nil {
		cfg.Exit = os.Exit
	}
	stopSignals := func() {}
	if cfg.Signals == nil {
		signals := make(chan os.Signal, 2)
		signal.Notify(signals, os.Interrupt, syscall.SIGTERM)
		cfg.Signals = signals
		stopSignals = func() { signal.Stop(signals) }
	}
	stdinEOF := make(chan struct{})
	if cfg.Stdin != nil {
		go func() {
			_, _ = io.Copy(io.Discard, cfg.Stdin)
			close(stdinEOF)
		}()
	}
	ctx, cancel := context.WithCancel(parent)
	l := &line{cfg: cfg, cancel: cancel, done: make(chan struct{})}
	go l.run(ctx, stdinEOF)
	return ctx, func() {
		l.mu.Lock()
		if !l.stopped {
			l.stopped = true
			close(l.done)
			if l.timer != nil {
				l.timer.Stop()
			}
		}
		l.mu.Unlock()
		stopSignals()
		cancel()
	}
}

type line struct {
	cfg     Config
	cancel  context.CancelFunc
	done    chan struct{}
	mu      sync.Mutex
	stopped bool
	timer   *time.Timer
}

func (l *line) run(ctx context.Context, stdinEOF <-chan struct{}) {
	parentGone, ctxDone, cancelled := l.cfg.ParentGone, ctx.Done(), false
	for {
		select {
		case <-l.done:
			return
		case sig := <-l.cfg.Signals:
			if cancelled {
				l.exit(fmt.Sprintf("second shutdown request (%v)", sig), false)
				return
			}
			l.begin(fmt.Sprintf("signal %v", sig))
		case <-parentGone:
			parentGone = nil
			l.begin("parent process exited")
		case <-stdinEOF:
			stdinEOF = nil
			l.begin("stdin closed")
		case <-ctxDone:
			// Cancelled by the parent context; shutdown is still bounded.
			l.begin("context cancelled")
		}
		if !cancelled {
			cancelled, ctxDone = true, nil
			l.cancel()
		}
	}
}

func (l *line) log() *slog.Logger {
	if l.cfg.Log != nil {
		return l.cfg.Log
	}
	return slog.Default()
}

// begin arms the hard-exit timer on the first trigger.
func (l *line) begin(reason string) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.stopped || l.timer != nil {
		return
	}
	l.log().Info("core shutting down", "reason", reason)
	l.timer = time.AfterFunc(l.cfg.Grace, func() { l.exit("shutdown exceeded its grace period", true) })
}

func (l *line) exit(reason string, dump bool) {
	l.mu.Lock()
	defer l.mu.Unlock()
	if l.stopped {
		return
	}
	l.stopped = true
	l.log().Error("core exiting", "reason", reason)
	if dump {
		// Names the wait that wedged shutdown.
		_ = pprof.Lookup("goroutine").WriteTo(l.cfg.Stderr, 1)
	}
	l.cfg.Exit(1)
}

// ParentEnv names the variable a spawning client sets to its own PID, so a core started after the
// client already died still notices.
const ParentEnv = "BEDROCK_CORE_PARENT_PID"

// ParentFromEnv returns the PID named by ParentEnv, else the current parent.
func ParentFromEnv() int {
	if pid, err := strconv.Atoi(os.Getenv(ParentEnv)); err == nil && pid > 0 {
		return pid
	}
	return os.Getppid()
}
