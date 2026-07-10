package proxy

import (
	"bufio"
	"bytes"
	"context"
	"crypto/sha256"
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

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
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

	runDir, err := stableRuntimeDirectory(sourceDir)
	if err != nil {
		t.Fatalf("resolve stable BDS runtime: %v", err)
	}
	lease, err := acquireRuntimeLease(runDir+".lock", 30*time.Second)
	if err != nil {
		t.Fatalf("acquire stable BDS runtime lease: %v", err)
	}
	t.Cleanup(func() {
		if err := lease.Close(); err != nil {
			t.Errorf("release stable BDS runtime lease: %v", err)
		}
	})
	if _, err := prepareStableRuntime(sourceDir, runDir, bedrockExecutableName()); err != nil {
		t.Fatalf("prepare stable BDS runtime: %v", err)
	}
	port := reserveUDPPort(t)
	portV6 := reserveUDPPort(t)
	if err := configureServerProperties(filepath.Join(runDir, "server.properties"), port, portV6); err != nil {
		t.Fatalf("configure BDS properties: %v", err)
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
	if got := client.Proto().ID(); got != 1001 {
		t.Fatalf("protocol ID = %d, want %d", got, 1001)
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
	canonicalSource, canonicalRuntime, err := validateRuntimeSeparation(sourceDir, runDir)
	if err != nil {
		t.Fatalf("revalidate stable runtime paths after BDS shutdown: %v", err)
	}
	if err := validateRuntimeOwnershipMarker(
		filepath.Join(canonicalRuntime, runtimeOwnerMarker),
		runtimeOwnershipMarker(canonicalSource),
	); err != nil {
		t.Fatalf("revalidate stable runtime ownership after BDS shutdown: %v", err)
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
	path := filepath.Join(runDir, bedrockExecutableName())

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

func TestConfigureServerProperties(t *testing.T) {
	path := filepath.Join(t.TempDir(), "server.properties")
	input := strings.Join([]string{
		"server-port=19132",
		"server-portv6=19133",
		"online-mode=true",
		"allow-list=true",
		"enable-lan-visibility=true",
		"motd=keep me",
		"",
	}, "\n")
	if err := os.WriteFile(path, []byte(input), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := configureServerProperties(path, 20001, 20002); err != nil {
		t.Fatalf("configureServerProperties(): %v", err)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	got := string(data)
	for _, line := range []string{
		"server-port=20001",
		"server-portv6=20002",
		"online-mode=false",
		"allow-list=false",
		"enable-lan-visibility=false",
		"motd=keep me",
	} {
		if !strings.Contains(got, line+"\n") {
			t.Errorf("configured properties missing %q:\n%s", line, got)
		}
	}
}

func TestConfigureServerPropertiesRequiresEveryProperty(t *testing.T) {
	path := filepath.Join(t.TempDir(), "server.properties")
	input := "server-port=19132\nserver-portv6=19133\nonline-mode=true\nallow-list=true\n"
	if err := os.WriteFile(path, []byte(input), 0o600); err != nil {
		t.Fatal(err)
	}
	err := configureServerProperties(path, 20001, 20002)
	if err == nil || !strings.Contains(err.Error(), "enable-lan-visibility") {
		t.Fatalf("configureServerProperties() error = %v, want missing-property error", err)
	}
}

func TestPrepareStableRuntimeKeepsExecutableIdentityAndResetsMutableData(t *testing.T) {
	sourceDir := t.TempDir()
	runtimeDir := filepath.Join(t.TempDir(), "stable-runtime")
	name := "bedrock_server.test"
	source := filepath.Join(sourceDir, name)
	if err := os.WriteFile(source, []byte("stable executable"), 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(sourceDir, "server.properties"), []byte("source properties"), 0o600); err != nil {
		t.Fatal(err)
	}
	executable, err := prepareStableRuntime(sourceDir, runtimeDir, name)
	if err != nil {
		t.Fatalf("prepareStableRuntime() first call: %v", err)
	}
	markerBefore, err := os.ReadFile(filepath.Join(runtimeDir, runtimeOwnerMarker))
	if err != nil {
		t.Fatalf("read runtime ownership marker: %v", err)
	}
	firstExecutableInfo, err := os.Stat(executable)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(runtimeDir, "generated.log"), []byte("remove me"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(runtimeDir, "server.properties"), []byte("mutated"), 0o600); err != nil {
		t.Fatal(err)
	}
	executableAgain, err := prepareStableRuntime(sourceDir, runtimeDir, name)
	if err != nil {
		t.Fatalf("prepareStableRuntime() second call: %v", err)
	}
	secondExecutableInfo, err := os.Stat(executableAgain)
	if err != nil {
		t.Fatal(err)
	}
	if executableAgain != executable || !os.SameFile(firstExecutableInfo, secondExecutableInfo) {
		t.Fatal("stable runtime replaced or moved an unchanged executable")
	}
	markerAfter, err := os.ReadFile(filepath.Join(runtimeDir, runtimeOwnerMarker))
	if err != nil || !bytes.Equal(markerBefore, markerAfter) {
		t.Fatalf("runtime ownership marker changed: before=%q after=%q err=%v", markerBefore, markerAfter, err)
	}
	if _, err := os.Stat(filepath.Join(runtimeDir, "generated.log")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("generated runtime file survived reset: %v", err)
	}
	data, err := os.ReadFile(filepath.Join(runtimeDir, "server.properties"))
	if err != nil || string(data) != "source properties" {
		t.Fatalf("runtime mutable data = %q, %v", data, err)
	}
	if data, err := os.ReadFile(source); err != nil || string(data) != "stable executable" {
		t.Fatalf("source executable changed: %q, %v", data, err)
	}
	if data, err := os.ReadFile(filepath.Join(sourceDir, "server.properties")); err != nil || string(data) != "source properties" {
		t.Fatalf("source mutable data changed: %q, %v", data, err)
	}
}

func TestRuntimeLeaseIsExclusiveAndReleased(t *testing.T) {
	path := filepath.Join(t.TempDir(), "bds-runtime.lock")
	first, err := acquireRuntimeLease(path, time.Second)
	if err != nil {
		t.Fatalf("first acquireRuntimeLease(): %v", err)
	}
	if second, err := acquireRuntimeLease(path, 50*time.Millisecond); err == nil {
		_ = second.Close()
		t.Fatal("second acquireRuntimeLease() succeeded while first held lease")
	}
	if err := first.Close(); err != nil {
		t.Fatalf("release first lease: %v", err)
	}
	third, err := acquireRuntimeLease(path, time.Second)
	if err != nil {
		t.Fatalf("acquire after release: %v", err)
	}
	if err := third.Close(); err != nil {
		t.Fatalf("release third lease: %v", err)
	}
}

func TestValidateRuntimeSeparationRejectsRelatedPaths(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "source", "install")
	if err := os.MkdirAll(source, 0o700); err != nil {
		t.Fatal(err)
	}
	for name, runtimeDir := range map[string]string{
		"equal":              source,
		"runtime ancestor":   filepath.Join(root, "source"),
		"runtime descendant": filepath.Join(source, "cache", "not-yet-created"),
	} {
		t.Run(name, func(t *testing.T) {
			if _, _, err := validateRuntimeSeparation(source, runtimeDir); err == nil {
				t.Fatalf("validateRuntimeSeparation(%q, %q) succeeded", source, runtimeDir)
			}
		})
	}
}

func TestPrepareStableRuntimeRejectsRelatedPathsWithoutMutation(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "source", "install")
	if err := os.MkdirAll(source, 0o700); err != nil {
		t.Fatal(err)
	}
	name := "bedrock_server.test"
	executable := filepath.Join(source, name)
	if err := os.WriteFile(executable, []byte("source executable"), 0o700); err != nil {
		t.Fatal(err)
	}
	ancestorSentinel := filepath.Join(root, "source", "keep.txt")
	if err := os.WriteFile(ancestorSentinel, []byte("keep ancestor"), 0o600); err != nil {
		t.Fatal(err)
	}
	for testName, runtimeDir := range map[string]string{
		"equal":              source,
		"runtime ancestor":   filepath.Join(root, "source"),
		"runtime descendant": filepath.Join(source, "cache", "not-yet-created"),
	} {
		t.Run(testName, func(t *testing.T) {
			if _, err := prepareStableRuntime(source, runtimeDir, name); err == nil {
				t.Fatalf("prepareStableRuntime(%q, %q) succeeded", source, runtimeDir)
			}
			if data, err := os.ReadFile(executable); err != nil || string(data) != "source executable" {
				t.Fatalf("source executable changed: %q, %v", data, err)
			}
			if data, err := os.ReadFile(ancestorSentinel); err != nil || string(data) != "keep ancestor" {
				t.Fatalf("ancestor sentinel changed: %q, %v", data, err)
			}
		})
	}
}

func TestValidateRuntimeSeparationResolvesNonExistingAliasParent(t *testing.T) {
	root := t.TempDir()
	source := filepath.Join(root, "source")
	if err := os.Mkdir(source, 0o700); err != nil {
		t.Fatal(err)
	}
	alias := filepath.Join(root, "source-alias")
	if err := os.Symlink(source, alias); err != nil {
		t.Skipf("directory symlink unavailable: %v", err)
	}
	runtimeDir := filepath.Join(alias, "non-existing-cache")
	if _, _, err := validateRuntimeSeparation(source, runtimeDir); err == nil {
		t.Fatal("symlink alias descendant was accepted")
	}
}

func TestPrepareStableRuntimeRefusesUnmarkedNonEmptyDirectory(t *testing.T) {
	sourceDir := t.TempDir()
	runtimeDir := t.TempDir()
	name := "bedrock_server.test"
	if err := os.WriteFile(filepath.Join(sourceDir, name), []byte("source executable"), 0o700); err != nil {
		t.Fatal(err)
	}
	sentinel := filepath.Join(runtimeDir, "do-not-delete.txt")
	if err := os.WriteFile(sentinel, []byte("owned by somebody else"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, err := prepareStableRuntime(sourceDir, runtimeDir, name); err == nil {
		t.Fatal("prepareStableRuntime() claimed an unmarked non-empty directory")
	}
	if data, err := os.ReadFile(sentinel); err != nil || string(data) != "owned by somebody else" {
		t.Fatalf("unmarked sentinel changed: %q, %v", data, err)
	}
}

func TestPrepareStableRuntimeRejectsMarkerSourceMismatch(t *testing.T) {
	root := t.TempDir()
	firstSource := filepath.Join(root, "source-one")
	secondSource := filepath.Join(root, "source-two")
	runtimeDir := filepath.Join(root, "runtime")
	for _, source := range []string{firstSource, secondSource} {
		if err := os.Mkdir(source, 0o700); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(filepath.Join(source, "bedrock_server.test"), []byte(source), 0o700); err != nil {
			t.Fatal(err)
		}
	}
	if _, err := prepareStableRuntime(firstSource, runtimeDir, "bedrock_server.test"); err != nil {
		t.Fatalf("first prepareStableRuntime(): %v", err)
	}
	if _, err := prepareStableRuntime(secondSource, runtimeDir, "bedrock_server.test"); err == nil {
		t.Fatal("runtime ownership marker accepted a different source")
	}
	data, err := os.ReadFile(filepath.Join(runtimeDir, "bedrock_server.test"))
	if err != nil || string(data) != firstSource {
		t.Fatalf("stable executable changed after marker mismatch: %q, %v", data, err)
	}
}

func bedrockExecutableName() string {
	if runtime.GOOS == "windows" {
		return "bedrock_server.exe"
	}
	return "bedrock_server"
}

func stableRuntimeDirectory(sourceDir string) (string, error) {
	if configured := os.Getenv("RUST_MCBE_BDS_RUNTIME_DIR"); configured != "" {
		return filepath.Abs(configured)
	}
	source, err := filepath.Abs(sourceDir)
	if err != nil {
		return "", err
	}
	localDir := filepath.Dir(filepath.Dir(source))
	return filepath.Join(localDir, "bds-runtime", filepath.Base(source)), nil
}

func acquireRuntimeLease(path string, timeout time.Duration) (io.Closer, error) {
	return lockfile.Acquire(path, timeout)
}

const runtimeOwnerMarker = ".rust-mcbe-runtime-owner"

func validateRuntimeSeparation(sourceDir, runtimeDir string) (string, string, error) {
	canonicalSource, err := filepath.Abs(sourceDir)
	if err != nil {
		return "", "", fmt.Errorf("resolve BDS source: %w", err)
	}
	canonicalSource, err = canonicalExistingPath(canonicalSource)
	if err != nil {
		return "", "", fmt.Errorf("resolve BDS source links: %w", err)
	}
	sourceInfo, err := os.Stat(canonicalSource)
	if err != nil {
		return "", "", fmt.Errorf("stat BDS source: %w", err)
	}
	if !sourceInfo.IsDir() {
		return "", "", fmt.Errorf("BDS source is not a directory: %s", canonicalSource)
	}

	canonicalRuntime, err := canonicalPathThroughExistingParent(runtimeDir)
	if err != nil {
		return "", "", fmt.Errorf("resolve stable runtime: %w", err)
	}
	if pathsOverlap(canonicalSource, canonicalRuntime) {
		return "", "", fmt.Errorf("BDS source and stable runtime overlap: source=%s runtime=%s", canonicalSource, canonicalRuntime)
	}
	return filepath.Clean(canonicalSource), filepath.Clean(canonicalRuntime), nil
}

func canonicalPathThroughExistingParent(path string) (string, error) {
	absolute, err := filepath.Abs(path)
	if err != nil {
		return "", err
	}
	current := filepath.Clean(absolute)
	missing := make([]string, 0, 4)
	for {
		if _, err := os.Lstat(current); err == nil {
			resolved, err := canonicalExistingPath(current)
			if err != nil {
				return "", err
			}
			for index := len(missing) - 1; index >= 0; index-- {
				resolved = filepath.Join(resolved, missing[index])
			}
			return filepath.Clean(resolved), nil
		} else if !errors.Is(err, os.ErrNotExist) {
			return "", err
		}
		parent := filepath.Dir(current)
		if parent == current {
			return "", fmt.Errorf("no existing parent for %s", absolute)
		}
		missing = append(missing, filepath.Base(current))
		current = parent
	}
}

func pathsOverlap(first, second string) bool {
	if runtimePathsCaseInsensitive() {
		first = strings.ToLower(first)
		second = strings.ToLower(second)
	}
	return pathContains(first, second) || pathContains(second, first)
}

func runtimePathsCaseInsensitive() bool {
	return runtime.GOOS == "windows" || runtime.GOOS == "darwin"
}

func pathContains(parent, candidate string) bool {
	relative, err := filepath.Rel(parent, candidate)
	if err != nil || filepath.IsAbs(relative) {
		return false
	}
	return relative == "." || (relative != ".." && !strings.HasPrefix(relative, ".."+string(os.PathSeparator)))
}

func prepareStableRuntime(sourceDir, runtimeDir, executable string) (string, error) {
	canonicalSource, canonicalRuntime, err := validateRuntimeSeparation(sourceDir, runtimeDir)
	if err != nil {
		return "", err
	}
	if err := os.MkdirAll(canonicalRuntime, 0o700); err != nil {
		return "", fmt.Errorf("create stable runtime: %w", err)
	}
	info, err := os.Lstat(canonicalRuntime)
	if err != nil {
		return "", err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return "", fmt.Errorf("stable runtime is not a real directory: %s", canonicalRuntime)
	}
	if err := claimRuntimeOwnership(canonicalRuntime, canonicalSource); err != nil {
		return "", err
	}
	sourceExecutable := filepath.Join(canonicalSource, executable)
	runtimeExecutable := filepath.Join(canonicalRuntime, executable)
	if err := ensureStableExecutable(sourceExecutable, runtimeExecutable); err != nil {
		return "", err
	}
	entries, err := os.ReadDir(canonicalRuntime)
	if err != nil {
		return "", err
	}
	for _, entry := range entries {
		if entry.Name() == executable || entry.Name() == runtimeOwnerMarker {
			continue
		}
		if err := os.RemoveAll(filepath.Join(canonicalRuntime, entry.Name())); err != nil {
			return "", fmt.Errorf("reset runtime entry %q: %w", entry.Name(), err)
		}
	}
	if err := copyTree(canonicalSource, canonicalRuntime, executable); err != nil {
		return "", fmt.Errorf("copy mutable BDS runtime data: %w", err)
	}
	return runtimeExecutable, nil
}

func claimRuntimeOwnership(runtimeDir, canonicalSource string) error {
	markerPath := filepath.Join(runtimeDir, runtimeOwnerMarker)
	want := runtimeOwnershipMarker(canonicalSource)
	entries, err := os.ReadDir(runtimeDir)
	if err != nil {
		return fmt.Errorf("inspect stable runtime ownership: %w", err)
	}
	for _, entry := range entries {
		if entry.Name() == runtimeOwnerMarker {
			return validateRuntimeOwnershipMarker(markerPath, want)
		}
	}
	if len(entries) != 0 {
		return fmt.Errorf("refuse unmarked non-empty stable runtime: %s", runtimeDir)
	}

	marker, err := os.OpenFile(markerPath, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if errors.Is(err, os.ErrExist) {
		return validateRuntimeOwnershipMarker(markerPath, want)
	}
	if err != nil {
		return fmt.Errorf("claim stable runtime ownership: %w", err)
	}
	claimed := false
	defer func() {
		if !claimed {
			_ = os.Remove(markerPath)
		}
	}()
	_, writeErr := io.WriteString(marker, want)
	syncErr := marker.Sync()
	closeErr := marker.Close()
	if err := errors.Join(writeErr, syncErr, closeErr); err != nil {
		return fmt.Errorf("write stable runtime ownership marker: %w", err)
	}
	entries, err = os.ReadDir(runtimeDir)
	if err != nil {
		return fmt.Errorf("verify stable runtime ownership: %w", err)
	}
	if len(entries) != 1 || entries[0].Name() != runtimeOwnerMarker {
		return fmt.Errorf("stable runtime changed while ownership was claimed: %s", runtimeDir)
	}
	claimed = true
	return nil
}

func validateRuntimeOwnershipMarker(path, want string) error {
	info, err := os.Lstat(path)
	if err != nil {
		return fmt.Errorf("inspect stable runtime ownership marker: %w", err)
	}
	if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
		return fmt.Errorf("stable runtime ownership marker is not a regular file: %s", path)
	}
	data, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("read stable runtime ownership marker: %w", err)
	}
	if string(data) != want {
		return fmt.Errorf("stable runtime belongs to a different BDS source: %s", path)
	}
	return nil
}

func runtimeOwnershipMarker(canonicalSource string) string {
	identity := filepath.Clean(canonicalSource)
	if runtime.GOOS == "windows" {
		identity = strings.ToLower(identity)
	}
	return "rust-mcbe-bds-runtime-v1\nsource=" + identity + "\n"
}

func ensureStableExecutable(source, destination string) error {
	equal, err := filesEqual(source, destination)
	if err == nil && equal {
		return nil
	}
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		return fmt.Errorf("compare stable executable: %w", err)
	}
	temp, err := os.CreateTemp(filepath.Dir(destination), "bedrock-server-exe-")
	if err != nil {
		return err
	}
	tempName := temp.Name()
	defer os.Remove(tempName)
	sourceFile, err := os.Open(source)
	if err != nil {
		_ = temp.Close()
		return err
	}
	_, copyErr := io.Copy(temp, sourceFile)
	sourceCloseErr := sourceFile.Close()
	closeErr := temp.Close()
	if err := errors.Join(copyErr, sourceCloseErr, closeErr); err != nil {
		return err
	}
	if sourceInfo, err := os.Stat(source); err == nil {
		if err := os.Chmod(tempName, sourceInfo.Mode().Perm()); err != nil {
			return err
		}
	}
	if err := os.Remove(destination); err != nil && !errors.Is(err, os.ErrNotExist) {
		return err
	}
	if err := os.Rename(tempName, destination); err != nil {
		return err
	}
	return nil
}

func filesEqual(first, second string) (bool, error) {
	firstFile, err := os.Open(first)
	if err != nil {
		return false, err
	}
	defer firstFile.Close()
	secondFile, err := os.Open(second)
	if err != nil {
		return false, err
	}
	defer secondFile.Close()
	firstHash, secondHash := sha256.New(), sha256.New()
	if _, err := io.Copy(firstHash, firstFile); err != nil {
		return false, err
	}
	if _, err := io.Copy(secondHash, secondFile); err != nil {
		return false, err
	}
	return bytes.Equal(firstHash.Sum(nil), secondHash.Sum(nil)), nil
}

func configureServerProperties(path string, port, portV6 int) error {
	data, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	required := []string{"server-port", "server-portv6", "online-mode", "allow-list", "enable-lan-visibility"}
	want := map[string]string{
		"server-port":           strconv.Itoa(port),
		"server-portv6":         strconv.Itoa(portV6),
		"online-mode":           "false",
		"allow-list":            "false",
		"enable-lan-visibility": "false",
	}
	found := make(map[string]int, len(required))
	lines := strings.Split(string(data), "\n")
	for index, line := range lines {
		key, _, ok := strings.Cut(strings.TrimSuffix(line, "\r"), "=")
		value, requiredProperty := want[key]
		if !ok || !requiredProperty {
			continue
		}
		found[key]++
		if found[key] > 1 {
			return fmt.Errorf("duplicate required server property %q", key)
		}
		lines[index] = key + "=" + value
	}
	for _, key := range required {
		if found[key] != 1 {
			return fmt.Errorf("required server property %q is missing", key)
		}
	}
	if err := os.WriteFile(path, []byte(strings.Join(lines, "\n")), 0o600); err != nil {
		return err
	}
	verified, err := os.ReadFile(path)
	if err != nil {
		return fmt.Errorf("verify server properties: %w", err)
	}
	for _, key := range required {
		line := key + "=" + want[key]
		if !containsPropertyLine(string(verified), line) {
			return fmt.Errorf("verify server property %q failed", key)
		}
	}
	return nil
}

func containsPropertyLine(properties, want string) bool {
	for _, line := range strings.Split(properties, "\n") {
		if strings.TrimSuffix(line, "\r") == want {
			return true
		}
	}
	return false
}

func copyTree(source, destination, skippedRootFile string) error {
	return filepath.WalkDir(source, func(path string, entry os.DirEntry, walkErr error) error {
		if walkErr != nil {
			return walkErr
		}
		relative, err := filepath.Rel(source, path)
		if err != nil {
			return err
		}
		target := filepath.Join(destination, relative)
		if relative == skippedRootFile || relative == runtimeOwnerMarker {
			return nil
		}
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
