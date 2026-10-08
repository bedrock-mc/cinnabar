package localworld

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"io"
	"log/slog"
	"net"
	"os"
	"os/exec"
	"strconv"
	"strings"
	"sync"
	"time"
)

const defaultStartTimeout = 30 * time.Second

// ProcessRunner hosts each Dragonfly world in a managed child running the local server binary.
// The child prints "ready" on stdout once listening and reads "pause", "resume" and "stop"
// lines on stdin; stdin EOF also stops it so it cannot outlive the core.
type ProcessRunner struct {
	Binary       string
	Env          []string // appended to the core's environment
	Log          *slog.Logger
	StartTimeout time.Duration // default 30s
}

func (r ProcessRunner) Start(ctx context.Context, spec StartSpec) (Instance, error) {
	if r.Binary == "" {
		return nil, errors.New("localworld: local server binary is not configured")
	}
	log := r.Log
	if log == nil {
		log = slog.Default()
	}
	timeout := r.StartTimeout
	if timeout <= 0 {
		timeout = defaultStartTimeout
	}
	address, err := freeLoopbackAddress()
	if err != nil {
		return nil, err
	}
	cmd := exec.Command(r.Binary,
		"-dir", spec.Dir,
		"-addr", address,
		"-name", spec.World.Name,
		"-game-mode", spec.World.GameMode,
		"-difficulty", spec.World.Difficulty,
		"-generator", spec.World.Generator,
		"-seed", strconv.FormatInt(spec.World.Seed, 10),
	)
	cmd.Env = append(os.Environ(), r.Env...)
	return launch(ctx, launchSpec{
		cmd: cmd, address: address, log: log.With("component", "local-server", "world", spec.World.ID),
		timeout: timeout, canPause: true,
		ready: func(line string) bool { return strings.TrimSpace(line) == "ready" },
	})
}

// launchSpec describes a child that speaks the stdin "stop" protocol and announces readiness on stdout.
type launchSpec struct {
	cmd      *exec.Cmd
	address  string
	log      *slog.Logger
	timeout  time.Duration
	canPause bool // false when the server has no faithful pause; SetPaused is then a no-op
	ready    func(stdoutLine string) bool
}

func launch(ctx context.Context, ls launchSpec) (Instance, error) {
	cmd := ls.cmd
	stdin, err := cmd.StdinPipe()
	if err != nil {
		return nil, err
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		return nil, err
	}
	stderr, err := cmd.StderrPipe()
	if err != nil {
		return nil, err
	}
	if err := cmd.Start(); err != nil {
		return nil, fmt.Errorf("localworld: start local server: %w", err)
	}
	proc := &process{cmd: cmd, stdin: stdin, address: ls.address, canPause: ls.canPause, done: make(chan struct{})}
	ready := make(chan struct{})
	var readers sync.WaitGroup
	readers.Add(2)
	go func() {
		defer readers.Done()
		var once sync.Once
		scanner := bufio.NewScanner(stdout)
		for scanner.Scan() {
			ls.log.Debug(scanner.Text())
			if ls.ready(scanner.Text()) {
				once.Do(func() { close(ready) })
			}
		}
	}()
	go func() {
		defer readers.Done()
		logLines(stderr, ls.log)
	}()
	go func() {
		readers.Wait()
		proc.waitErr = cmd.Wait()
		close(proc.done)
	}()

	timer := time.NewTimer(ls.timeout)
	defer timer.Stop()
	var startErr error
	select {
	case <-ready:
		return proc, nil
	case <-proc.done:
		startErr = fmt.Errorf("localworld: local server exited during startup: %v", proc.waitErr)
	case <-timer.C:
		startErr = errors.New("localworld: local server did not become ready in time")
	case <-ctx.Done():
		startErr = ctx.Err()
	}
	_ = cmd.Process.Kill()
	<-proc.done
	return nil, startErr
}

func freeLoopbackAddress() (string, error) {
	conn, err := net.ListenPacket("udp", net.JoinHostPort(localServerHost, "0"))
	if err != nil {
		return "", fmt.Errorf("localworld: reserve loopback port: %w", err)
	}
	defer conn.Close()
	return conn.LocalAddr().String(), nil
}

func logLines(reader io.Reader, log *slog.Logger) {
	scanner := bufio.NewScanner(reader)
	for scanner.Scan() {
		log.Info(scanner.Text())
	}
}

type process struct {
	cmd      *exec.Cmd
	address  string
	canPause bool
	done     chan struct{}
	waitErr  error
	mu       sync.Mutex // serialises stdin writes
	stdin    io.WriteCloser
}

func (p *process) Address() string       { return p.address }
func (p *process) Done() <-chan struct{} { return p.done }

func (p *process) send(line string) error {
	p.mu.Lock()
	defer p.mu.Unlock()
	_, err := io.WriteString(p.stdin, line+"\n")
	return err
}

// CanPause reports whether SetPaused has any effect.
func (p *process) CanPause() bool { return p.canPause }

func (p *process) SetPaused(paused bool) error {
	if !p.canPause {
		return nil
	}
	if paused {
		return p.send("pause")
	}
	return p.send("resume")
}

func (p *process) Stop(ctx context.Context) error {
	select {
	case <-p.done:
		return nil
	default:
	}
	_ = p.send("stop")
	select {
	case <-p.done:
		return nil
	case <-ctx.Done():
		_ = p.cmd.Process.Kill()
		<-p.done
		return fmt.Errorf("localworld: local server killed after shutdown timeout: %w", ctx.Err())
	}
}
