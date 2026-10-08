//go:build !windows

package streamnet

import (
	"bytes"
	"errors"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"syscall"
	"testing"
)

func TestUnixLongSocketDirectoryUsesStableLengthSafeEndpoint(t *testing.T) {
	dir := filepath.Join(t.TempDir(), strings.Repeat("macos-runner-segment-", 8))
	listener, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("Listen() with long socket directory: %v", err)
	}
	defer listener.Close()

	network, address, err := Resolve(dir)
	if err != nil {
		t.Fatalf("Resolve() with long socket directory: %v", err)
	}
	if network != "unix" {
		t.Fatalf("network = %q, want unix", network)
	}
	first := unixEndpointPath(dir)
	second := unixEndpointPath(dir)
	if address != first || first != second {
		t.Fatalf("endpoint is not stable: got %q, then %q, want resolved %q", first, second, address)
	}
	if len(address) > maxUnixEndpointPathBytes {
		t.Fatalf("endpoint length = %d, want <= %d: %q", len(address), maxUnixEndpointPathBytes, address)
	}
	if !strings.HasPrefix(address, "/tmp/cinnabar-") {
		t.Fatalf("long endpoint = %q, want bounded /tmp path", address)
	}
}

func TestUnixLongEndpointDerivationMatchesRustBridge(t *testing.T) {
	dir := filepath.Join("/var/folders/zz", strings.Repeat("macos-runner-segment-", 8))
	want := "/tmp/cinnabar-7b260d1b166f7db809ce8c3d8bd42d1a.sock"
	if got := unixEndpointPath(dir); got != want {
		t.Fatalf("unixEndpointPath() = %q, want shared Rust endpoint %q", got, want)
	}
}

func TestUnixEndpointLexicalNormalizationMatchesRustBridge(t *testing.T) {
	repeatedParent := "/tmp/" + strings.Repeat("segment/../", 12)
	invalidBytes := append([]byte("/tmp/\xff/"), bytes.Repeat([]byte{'x'}, 100)...)
	vectors := []struct {
		name      string
		socketDir string
		want      string
	}{
		{
			name:      "duplicate separators and dot components",
			socketDir: "/tmp//alpha/./beta/../gamma",
			want:      "/tmp/alpha/gamma/game.sock",
		},
		{
			name:      "long raw path normalizes below limit",
			socketDir: repeatedParent,
			want:      "/tmp/game.sock",
		},
		{
			name:      "exact direct limit",
			socketDir: "/" + strings.Repeat("a", 92),
			want:      "/" + strings.Repeat("a", 92) + "/game.sock",
		},
		{
			name:      "first hashed length",
			socketDir: "/" + strings.Repeat("a", 93),
			want:      "/tmp/cinnabar-d32a5982698ad8de34829c65f893edf6.sock",
		},
		{
			name:      "unicode bytes",
			socketDir: "/tmp/" + strings.Repeat("路径/", 20),
			want:      "/tmp/cinnabar-08390d1ff13834e20abadae40eff1ce0.sock",
		},
		{
			name:      "non UTF-8 bytes",
			socketDir: string(invalidBytes),
			want:      "/tmp/cinnabar-32ec4a93b88918d1547cfbaf69f63a13.sock",
		},
	}
	for _, vector := range vectors {
		t.Run(vector.name, func(t *testing.T) {
			if got := unixEndpointPath(vector.socketDir); got != vector.want {
				t.Fatalf("unixEndpointPath() = %q, want shared Rust endpoint %q", got, vector.want)
			}
		})
	}
}

func TestUnixActiveListenerCannotBeStolen(t *testing.T) {
	dir := t.TempDir()
	first, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("first Listen(): %v", err)
	}
	defer first.Close()

	second, err := New(dir).Listen("")
	if err == nil {
		_ = second.Close()
		t.Fatal("second Listen() stole an active Unix socket")
	}
}

func TestUnixOldListenerCannotDeleteSuccessorSocket(t *testing.T) {
	dir := t.TempDir()
	old, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("old Listen(): %v", err)
	}
	path := unixEndpointPath(dir)
	moved := path + ".old"
	if err := os.Rename(path, moved); err != nil {
		t.Fatalf("move old socket: %v", err)
	}
	defer os.Remove(moved)

	successor, err := net.Listen("unix", path)
	if err != nil {
		t.Fatalf("successor Listen(): %v", err)
	}
	if unix, ok := successor.(*net.UnixListener); ok {
		unix.SetUnlinkOnClose(false)
	}
	defer func() {
		_ = successor.Close()
		_ = os.Remove(path)
	}()

	closeErr := old.Close()
	if closeErr == nil {
		t.Fatal("old Close() did not report the changed endpoint identity")
	}
	info, err := os.Lstat(path)
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			t.Fatal("old Close() deleted the successor socket")
		}
		t.Fatal(err)
	}
	if info.Mode()&os.ModeSocket == 0 {
		t.Fatalf("successor path mode = %v, want socket", info.Mode())
	}
}

// Vectored header+payload writes must keep frames intact across partial writev calls.
func TestUnixVectoredWriteKeepsLargeFramesIntact(t *testing.T) {
	pair, err := syscallSocketPair()
	if err != nil {
		t.Fatalf("socketpair: %v", err)
	}
	writer, reader := NewFramedConn(pair[0]), NewFramedConn(pair[1])
	defer writer.Close()
	defer reader.Close()
	if !vectoredConn(writer.Conn) {
		t.Fatalf("socketpair conn %T is not vectored", writer.Conn)
	}
	frames := [][]byte{{0xfe}, bytes.Repeat([]byte{0xab}, 4<<20), {0xfe, 2}}
	errC := make(chan error, 1)
	go func() {
		for _, frame := range frames {
			n, err := writer.Write(frame)
			if err == nil && n != len(frame) {
				err = errors.New("short vectored write")
			}
			if err != nil {
				errC <- err
				return
			}
		}
		errC <- nil
	}()
	for i, want := range frames {
		got, err := reader.ReadPacket()
		if err != nil {
			t.Fatalf("ReadPacket(%d) error = %v", i, err)
		}
		if !bytes.Equal(got, want) {
			t.Fatalf("ReadPacket(%d) returned %d bytes, want %d", i, len(got), len(want))
		}
	}
	if err := <-errC; err != nil {
		t.Fatalf("Write() error = %v", err)
	}
}

func syscallSocketPair() ([2]net.Conn, error) {
	var conns [2]net.Conn
	fds, err := syscall.Socketpair(syscall.AF_UNIX, syscall.SOCK_STREAM, 0)
	if err != nil {
		return conns, err
	}
	for i, fd := range fds {
		file := os.NewFile(uintptr(fd), "streamnet-socketpair")
		conn, err := net.FileConn(file)
		_ = file.Close()
		if err != nil {
			return conns, err
		}
		conns[i] = conn
	}
	return conns, nil
}

// Accepted game connections must not keep macOS's 8 KiB Unix socket buffers.
func TestUnixAcceptedGameConnectionUsesLargeSocketBuffers(t *testing.T) {
	dir := t.TempDir()
	listener, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("Listen(): %v", err)
	}
	defer listener.Close()
	network, address, err := Resolve(dir)
	if err != nil {
		t.Fatalf("Resolve(): %v", err)
	}
	client, err := net.Dial(network, address)
	if err != nil {
		t.Fatalf("Dial(): %v", err)
	}
	defer client.Close()
	accepted, err := listener.Accept()
	if err != nil {
		t.Fatalf("Accept(): %v", err)
	}
	defer accepted.Close()

	for _, option := range []int{syscall.SO_SNDBUF, syscall.SO_RCVBUF} {
		tuned := socketOption(t, accepted.(*FramedConn).Conn, syscall.SOL_SOCKET, option)
		untuned := socketOption(t, client, syscall.SOL_SOCKET, option)
		if tuned <= untuned {
			t.Fatalf("option %d = %d on the accepted conn, want above the untuned %d", option, tuned, untuned)
		}
		if runtime.GOOS == "darwin" && tuned < localSocketBufferBytes {
			t.Fatalf("option %d = %d, want at least %d", option, tuned, localSocketBufferBytes)
		}
	}
}

// Loopback TCP (the Windows local leg) must send each frame without waiting for an ACK.
func TestTunedLoopbackTCPConnectionDisablesNagle(t *testing.T) {
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatalf("Listen(): %v", err)
	}
	defer listener.Close()
	client, err := net.Dial("tcp", listener.Addr().String())
	if err != nil {
		t.Fatalf("Dial(): %v", err)
	}
	defer client.Close()
	accepted, err := listener.Accept()
	if err != nil {
		t.Fatalf("Accept(): %v", err)
	}
	defer accepted.Close()
	if err := accepted.(*net.TCPConn).SetNoDelay(false); err != nil {
		t.Fatalf("SetNoDelay(false): %v", err)
	}

	tuneLocalConn(accepted)

	if got := socketOption(t, accepted, syscall.IPPROTO_TCP, syscall.TCP_NODELAY); got == 0 {
		t.Fatal("TCP_NODELAY is off after tuning")
	}
}

func socketOption(t *testing.T, conn net.Conn, level, option int) int {
	t.Helper()
	raw, err := conn.(syscall.Conn).SyscallConn()
	if err != nil {
		t.Fatalf("SyscallConn(): %v", err)
	}
	var value int
	var optErr error
	if err := raw.Control(func(fd uintptr) {
		value, optErr = syscall.GetsockoptInt(int(fd), level, option)
	}); err != nil {
		t.Fatalf("Control(): %v", err)
	}
	if optErr != nil {
		t.Fatalf("getsockopt(%d, %d): %v", level, option, optErr)
	}
	return value
}
