package proxy

import (
	"bufio"
	"bytes"
	"context"
	"errors"
	"fmt"
	"io"
	"net"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strconv"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

func TestProxyJoin(t *testing.T) {
	sourceDir := os.Getenv("BEDROCK_BDS_DIR")
	if sourceDir == "" {
		t.Skip("BEDROCK_BDS_DIR is not set")
	}
	info, err := os.Stat(sourceDir)
	if err != nil || !info.IsDir() {
		t.Fatalf("BEDROCK_BDS_DIR %q is not a directory: %v", sourceDir, err)
	}

	runDir := filepath.Join(t.TempDir(), "bds")
	if err := copyTree(sourceDir, runDir); err != nil {
		t.Fatalf("copy BDS: %v", err)
	}
	port := reserveUDPPort(t)
	portV6 := reserveUDPPort(t)
	if err := setServerPorts(filepath.Join(runDir, "server.properties"), port, portV6); err != nil {
		t.Fatalf("configure BDS ports: %v", err)
	}

	bds := startTestBDS(t, runDir)
	t.Cleanup(func() {
		if err := bds.stop(20 * time.Second); err != nil {
			t.Errorf("stop BDS cleanly: %v\nBDS output:\n%s", err, bds.output())
		}
	})
	if err := bds.waitReady(45 * time.Second); err != nil {
		t.Fatalf("wait for BDS: %v\nBDS output:\n%s", err, bds.output())
	}

	ctx, cancel := context.WithTimeout(context.Background(), 60*time.Second)
	defer cancel()
	socketDir := filepath.Join(t.TempDir(), "socket")
	core := startTestCore(t, socketDir, net.JoinHostPort("127.0.0.1", strconv.Itoa(port)))
	t.Cleanup(func() {
		if err := core.stop(10 * time.Second); err != nil {
			t.Errorf("stop core cleanly: %v\nCore output:\n%s", err, core.output.String())
		}
	})
	waitForEndpoint(t, ctx, socketDir, core)

	client, err := (minecraft.Dialer{
		IdentityData: login.IdentityData{DisplayName: "RustMCBEPhase0"},
		Protocol:     minecraft.DefaultProtocol,
	}).DialContextNetwork(ctx, streamnet.New(socketDir), "")
	if err != nil {
		t.Fatalf("dial core: %v\nCore status: %s\nBDS output:\n%s", err, core.status(), bds.output())
	}
	defer client.Close()
	if err := client.DoSpawnContext(ctx); err != nil {
		t.Fatalf("complete spawn: %v\nBDS output:\n%s", err, bds.output())
	}
	if got := client.Proto().ID(); got != pinnedProtocol {
		t.Fatalf("protocol ID = %d, want %d", got, pinnedProtocol)
	}
	if got := client.GameData().EntityRuntimeID; got == 0 {
		t.Fatal("StartGame runtime entity ID = 0, want non-zero")
	}

	_ = client.Close()
	if err := core.stop(10 * time.Second); err != nil {
		t.Fatalf("stop core cleanly: %v\nCore output:\n%s", err, core.output.String())
	}
	if err := bds.stop(20 * time.Second); err != nil {
		t.Fatalf("stop BDS cleanly: %v\nBDS output:\n%s", err, bds.output())
	}
}

func TestProxyHelperProcess(t *testing.T) {
	if os.Getenv("RUST_MCBE_PROXY_HELPER") != "1" {
		t.Skip("proxy subprocess helper")
	}
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	go func() {
		_, _ = io.Copy(io.Discard, os.Stdin)
		cancel()
	}()
	if err := Serve(ctx, Config{
		SocketDir: os.Getenv("RUST_MCBE_PROXY_SOCKET_DIR"),
		Upstream:  os.Getenv("RUST_MCBE_PROXY_UPSTREAM"),
	}); err != nil {
		t.Fatal(err)
	}
}

type testCore struct {
	stdin    io.WriteCloser
	process  *os.Process
	done     chan struct{}
	output   lockedBuffer
	waitMu   sync.Mutex
	waitErr  error
	stopOnce sync.Once
	stopErr  error
}

func startTestCore(t *testing.T, socketDir, upstream string) *testCore {
	t.Helper()
	core := &testCore{done: make(chan struct{})}
	cmd := exec.Command(os.Args[0], "-test.run=^TestProxyHelperProcess$", "-test.v")
	cmd.Env = append(os.Environ(),
		"RUST_MCBE_PROXY_HELPER=1",
		"RUST_MCBE_PROXY_SOCKET_DIR="+socketDir,
		"RUST_MCBE_PROXY_UPSTREAM="+upstream,
	)
	stdin, err := cmd.StdinPipe()
	if err != nil {
		t.Fatalf("core stdin: %v", err)
	}
	core.stdin = stdin
	cmd.Stdout = &core.output
	cmd.Stderr = &core.output
	if err := cmd.Start(); err != nil {
		t.Fatalf("start core: %v", err)
	}
	core.process = cmd.Process
	go func() {
		err := cmd.Wait()
		core.waitMu.Lock()
		core.waitErr = err
		core.waitMu.Unlock()
		close(core.done)
	}()
	return core
}

func (c *testCore) stop(timeout time.Duration) error {
	c.stopOnce.Do(func() {
		_ = c.stdin.Close()
		select {
		case <-c.done:
		case <-time.After(timeout):
			killErr := c.process.Kill()
			c.stopErr = errors.Join(c.stopErr, errors.New("timed out waiting for core exit"), killErr)
			select {
			case <-c.done:
			case <-time.After(5 * time.Second):
				c.stopErr = errors.Join(c.stopErr, errors.New("core did not exit after kill"))
			}
		}
		c.waitMu.Lock()
		c.stopErr = errors.Join(c.stopErr, c.waitErr)
		c.waitMu.Unlock()
	})
	return c.stopErr
}

func (c *testCore) status() string {
	select {
	case <-c.done:
		c.waitMu.Lock()
		err := c.waitErr
		c.waitMu.Unlock()
		return fmt.Sprintf("exited (%v)\n%s", err, c.output.String())
	default:
		return "still running\n" + c.output.String()
	}
}

type lockedBuffer struct {
	mu sync.Mutex
	b  bytes.Buffer
}

func (b *lockedBuffer) Write(p []byte) (int, error) {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.b.Write(p)
}

func (b *lockedBuffer) String() string {
	b.mu.Lock()
	defer b.mu.Unlock()
	return b.b.String()
}

type testBDS struct {
	stdin     io.WriteCloser
	process   *os.Process
	ready     chan struct{}
	done      chan error
	readyOnce sync.Once
	stopOnce  sync.Once
	stopErr   error
	outputMu  sync.Mutex
	lines     []string
}

func startTestBDS(t *testing.T, runDir string) *testBDS {
	t.Helper()
	executable := "bedrock_server"
	if runtime.GOOS == "windows" {
		executable += ".exe"
	}
	path := filepath.Join(runDir, executable)
	if _, err := os.Stat(path); err != nil {
		t.Fatalf("BDS executable %q: %v", path, err)
	}

	cmd := exec.Command(path)
	cmd.Dir = runDir
	stdin, err := cmd.StdinPipe()
	if err != nil {
		t.Fatalf("BDS stdin: %v", err)
	}
	stdout, err := cmd.StdoutPipe()
	if err != nil {
		t.Fatalf("BDS stdout: %v", err)
	}
	cmd.Stderr = cmd.Stdout
	bds := &testBDS{
		stdin: stdin,
		ready: make(chan struct{}),
		done:  make(chan error, 1),
	}
	if err := cmd.Start(); err != nil {
		t.Fatalf("start BDS: %v", err)
	}
	bds.process = cmd.Process
	go func() {
		scanner := bufio.NewScanner(stdout)
		scanner.Buffer(make([]byte, 64*1024), 1024*1024)
		for scanner.Scan() {
			line := scanner.Text()
			bds.outputMu.Lock()
			bds.lines = append(bds.lines, line)
			bds.outputMu.Unlock()
			if strings.Contains(line, "Server started.") {
				bds.readyOnce.Do(func() { close(bds.ready) })
			}
		}
	}()
	go func() { bds.done <- cmd.Wait() }()
	return bds
}

func (b *testBDS) waitReady(timeout time.Duration) error {
	select {
	case <-b.ready:
		return nil
	case err := <-b.done:
		b.done <- err
		return fmt.Errorf("BDS exited before readiness: %w", err)
	case <-time.After(timeout):
		return errors.New("timed out waiting for Server started")
	}
}

func (b *testBDS) stop(timeout time.Duration) error {
	b.stopOnce.Do(func() {
		if _, err := io.WriteString(b.stdin, "stop\n"); err != nil && !errors.Is(err, os.ErrClosed) {
			b.stopErr = fmt.Errorf("send stop: %w", err)
		}
		_ = b.stdin.Close()
		select {
		case err := <-b.done:
			if err != nil {
				b.stopErr = errors.Join(b.stopErr, fmt.Errorf("BDS exit: %w", err))
			}
		case <-time.After(timeout):
			killErr := b.process.Kill()
			b.stopErr = errors.Join(b.stopErr, errors.New("timed out waiting for BDS exit"), killErr)
			select {
			case <-b.done:
			case <-time.After(5 * time.Second):
				b.stopErr = errors.Join(b.stopErr, errors.New("BDS did not exit after kill"))
			}
		}
	})
	return b.stopErr
}

func (b *testBDS) output() string {
	b.outputMu.Lock()
	defer b.outputMu.Unlock()
	return strings.Join(b.lines, "\n")
}

func waitForEndpoint(t *testing.T, ctx context.Context, socketDir string, core *testCore) {
	t.Helper()
	ticker := time.NewTicker(10 * time.Millisecond)
	defer ticker.Stop()
	for {
		if _, _, err := streamnet.Resolve(socketDir); err == nil {
			return
		}
		select {
		case <-ctx.Done():
			t.Fatalf("wait for core endpoint: %v", ctx.Err())
		case <-core.done:
			t.Fatalf("core exited before publishing endpoint: %s", core.status())
		case <-ticker.C:
		}
	}
}

func reserveUDPPort(t *testing.T) int {
	t.Helper()
	conn, err := net.ListenUDP("udp4", &net.UDPAddr{IP: net.ParseIP("127.0.0.1")})
	if err != nil {
		t.Fatalf("reserve UDP port: %v", err)
	}
	port := conn.LocalAddr().(*net.UDPAddr).Port
	if err := conn.Close(); err != nil {
		t.Fatalf("release UDP port: %v", err)
	}
	return port
}

func setServerPorts(path string, port, portV6 int) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	lines := strings.Split(string(data), "\n")
	for index, line := range lines {
		switch {
		case strings.HasPrefix(line, "server-port="):
			lines[index] = "server-port=" + strconv.Itoa(port)
		case strings.HasPrefix(line, "server-portv6="):
			lines[index] = "server-portv6=" + strconv.Itoa(portV6)
		}
	}
	return os.WriteFile(path, []byte(strings.Join(lines, "\n")), 0o600)
}

func copyTree(source, destination string) error {
	return filepath.WalkDir(source, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		relative, err := filepath.Rel(source, path)
		if err != nil {
			return err
		}
		target := filepath.Join(destination, relative)
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if entry.Type()&os.ModeSymlink != 0 {
			return fmt.Errorf("refuse to copy symlink %s", path)
		}
		if entry.IsDir() {
			return os.MkdirAll(target, info.Mode().Perm())
		}
		input, err := os.Open(path)
		if err != nil {
			return err
		}
		defer input.Close()
		output, err := os.OpenFile(target, os.O_CREATE|os.O_WRONLY|os.O_TRUNC, info.Mode().Perm())
		if err != nil {
			return err
		}
		_, copyErr := io.Copy(output, input)
		closeErr := output.Close()
		return errors.Join(copyErr, closeErr)
	})
}
