package streamnet

import (
	"context"
	"errors"
	"io"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"sync"
	"testing"
	"time"
)

func TestResolveMissingEndpoint(t *testing.T) {
	_, _, err := Resolve(t.TempDir())
	if err == nil {
		t.Fatal("Resolve() error = nil, want missing endpoint error")
	}
}

func TestResolveAndRoundTrip(t *testing.T) {
	dir := t.TempDir()
	n := New(dir)
	listener, err := n.Listen("")
	if err != nil {
		t.Fatalf("Listen() error = %v", err)
	}
	defer listener.Close()

	network, address, err := Resolve(dir)
	if err != nil {
		t.Fatalf("Resolve() error = %v", err)
	}
	if runtime.GOOS == "windows" {
		if network != "tcp" {
			t.Fatalf("network = %q, want tcp", network)
		}
		host, _, err := net.SplitHostPort(address)
		if err != nil {
			t.Fatalf("SplitHostPort(%q) error = %v", address, err)
		}
		if host != "127.0.0.1" {
			t.Fatalf("host = %q, want 127.0.0.1", host)
		}
	} else {
		if network != "unix" || address != filepath.Join(dir, "game.sock") {
			t.Fatalf("Resolve() = %q, %q", network, address)
		}
	}

	accepted := make(chan net.Conn, 1)
	errC := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			errC <- err
			return
		}
		accepted <- conn
	}()

	ctx, cancel := context.WithTimeout(context.Background(), time.Second)
	defer cancel()
	client, err := n.DialContext(ctx, address)
	if err != nil {
		t.Fatalf("DialContext() error = %v", err)
	}
	defer client.Close()

	var server net.Conn
	select {
	case server = <-accepted:
	case err := <-errC:
		t.Fatalf("Accept() error = %v", err)
	case <-ctx.Done():
		t.Fatal("Accept() timed out")
	}
	defer server.Close()

	writeErr := make(chan error, 1)
	go func() {
		_, err := client.Write([]byte{0xfe, 42})
		writeErr <- err
	}()
	got, err := server.(interface{ ReadPacket() ([]byte, error) }).ReadPacket()
	if err != nil {
		t.Fatalf("ReadPacket() error = %v", err)
	}
	if string(got) != string([]byte{0xfe, 42}) {
		t.Fatalf("ReadPacket() = %x", got)
	}
	if err := <-writeErr; err != nil {
		t.Fatalf("Write() error = %v", err)
	}
}

func TestResolveRejectsNonLoopbackAddress(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("game.addr is Windows-only")
	}
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "game.addr"), []byte("0.0.0.0:19132\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	_, _, err := Resolve(dir)
	if err == nil {
		t.Fatal("Resolve() accepted a non-loopback address")
	}
}

func TestNetworkListenerStableIDAndCleanup(t *testing.T) {
	dir := t.TempDir()
	listener, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("Listen() error = %v", err)
	}
	id1, id2 := listener.ID(), listener.ID()
	if id1 == 0 || id1 != id2 {
		t.Fatalf("ID() = %d, then %d", id1, id2)
	}
	listener.PongData([]byte("ignored"))
	if err := listener.Close(); err != nil && !errors.Is(err, net.ErrClosed) {
		t.Fatalf("Close() error = %v", err)
	}
	if err := listener.Close(); err != nil && !errors.Is(err, net.ErrClosed) {
		t.Fatalf("second Close() error = %v", err)
	}
	name := "game.sock"
	if runtime.GOOS == "windows" {
		name = "game.addr"
	}
	if _, err := os.Lstat(filepath.Join(dir, name)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("endpoint remains after Close(): %v", err)
	}
}

func TestListenerCloseClosesAcceptedRawTransport(t *testing.T) {
	dir := t.TempDir()
	listener, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("Listen() error = %v", err)
	}
	networkName, address, err := Resolve(dir)
	if err != nil {
		t.Fatalf("Resolve() error = %v", err)
	}

	accepted := make(chan net.Conn, 1)
	acceptErr := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err != nil {
			acceptErr <- err
			return
		}
		accepted <- conn
	}()
	client, err := net.DialTimeout(networkName, address, time.Second)
	if err != nil {
		t.Fatalf("dial raw endpoint: %v", err)
	}
	defer client.Close()
	select {
	case <-accepted:
	case err := <-acceptErr:
		t.Fatalf("Accept() error = %v", err)
	case <-time.After(time.Second):
		t.Fatal("Accept() timed out")
	}

	if err := client.SetReadDeadline(time.Now().Add(2 * time.Second)); err != nil {
		t.Fatal(err)
	}
	readErr := make(chan error, 1)
	go func() {
		_, err := client.Read(make([]byte, 1))
		readErr <- err
	}()
	if err := listener.Close(); err != nil && !errors.Is(err, net.ErrClosed) {
		t.Fatalf("Close() error = %v", err)
	}
	select {
	case err := <-readErr:
		if err == nil {
			t.Fatal("raw pre-login transport returned data after listener Close()")
		}
		var netErr net.Error
		if errors.As(err, &netErr) && netErr.Timeout() {
			t.Fatalf("raw pre-login transport closed only by deadline: %v", err)
		}
	case <-time.After(500 * time.Millisecond):
		t.Fatal("raw pre-login transport remained open after listener Close()")
	}
}

func TestAcceptReturningDuringListenerCloseCannotEscapeTracking(t *testing.T) {
	server, client := net.Pipe()
	defer client.Close()
	raw := &closeRaceListener{
		conn:    server,
		started: make(chan struct{}),
		release: make(chan struct{}),
	}
	listener := &listener{
		Listener:    raw,
		cleanup:     func() error { return nil },
		lease:       noopCloser{},
		connections: make(map[*FramedConn]struct{}),
	}
	accepted := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if conn != nil {
			_ = conn.Close()
		}
		accepted <- err
	}()
	<-raw.started
	if err := listener.Close(); err != nil {
		t.Fatalf("Close() error = %v", err)
	}
	select {
	case err := <-accepted:
		if !errors.Is(err, net.ErrClosed) {
			t.Fatalf("Accept() error = %v, want net.ErrClosed", err)
		}
	case <-time.After(time.Second):
		t.Fatal("Accept() remained blocked during Close()")
	}
	readErr := make(chan error, 1)
	go func() {
		_, err := client.Read(make([]byte, 1))
		readErr <- err
	}()
	select {
	case err := <-readErr:
		if err == nil {
			t.Fatal("transport returned during Close() remained open")
		}
	case <-time.After(time.Second):
		t.Fatal("transport returned during Close() remained open")
	}
}

func TestEndpointLeaseAllowsExactlyOneSimultaneousOwner(t *testing.T) {
	dir := t.TempDir()
	start := make(chan struct{})
	type result struct {
		listener interface {
			net.Listener
			ID() int64
			PongData([]byte)
		}
		err error
	}
	results := make(chan result, 2)
	for range 2 {
		go func() {
			<-start
			listener, err := New(dir).Listen("")
			results <- result{listener: listener, err: err}
		}()
	}
	close(start)
	first, second := <-results, <-results
	var winner result
	if first.err == nil && second.err != nil {
		winner = first
	} else if second.err == nil && first.err != nil {
		winner = second
	} else {
		if first.listener != nil {
			_ = first.listener.Close()
		}
		if second.listener != nil {
			_ = second.listener.Close()
		}
		t.Fatalf("simultaneous Listen() results = (%v, %v), want exactly one success", first.err, second.err)
	}
	defer winner.listener.Close()

	networkName, address, err := Resolve(dir)
	if err != nil {
		t.Fatalf("loser changed winner publication: %v", err)
	}
	client, err := net.DialTimeout(networkName, address, time.Second)
	if err != nil {
		t.Fatalf("winner endpoint is unreachable: %v", err)
	}
	_ = client.Close()
}

func TestEndpointLeaseSurvivesCloseAndSuccessor(t *testing.T) {
	dir := t.TempDir()
	first, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("first Listen(): %v", err)
	}
	if err := first.Close(); err != nil && !errors.Is(err, net.ErrClosed) {
		t.Fatalf("close first listener: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dir, "game.lock")); err != nil {
		t.Fatalf("stable lease file missing after Close(): %v", err)
	}

	successor, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("successor Listen(): %v", err)
	}
	defer successor.Close()
	_ = first.Close()
	networkName, address, err := Resolve(dir)
	if err != nil {
		t.Fatalf("old Close() changed successor publication: %v", err)
	}
	client, err := net.DialTimeout(networkName, address, time.Second)
	if err != nil {
		t.Fatalf("successor endpoint is unreachable: %v", err)
	}
	_ = client.Close()
}

func TestEndpointConstructorFailureReleasesLease(t *testing.T) {
	dir := t.TempDir()
	name := unixEndpointName
	contents := []byte("not a socket")
	if runtime.GOOS == "windows" {
		name = windowsEndpointName
		contents = []byte("invalid publication")
	}
	path := filepath.Join(dir, name)
	if err := os.WriteFile(path, contents, 0o600); err != nil {
		t.Fatal(err)
	}
	if listener, err := New(dir).Listen(""); err == nil {
		_ = listener.Close()
		t.Fatal("Listen() accepted an invalid existing endpoint")
	}
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	listener, err := New(dir).Listen("")
	if err != nil {
		t.Fatalf("Listen() after constructor failure: %v", err)
	}
	_ = listener.Close()
}

func TestNetworkPingUnsupported(t *testing.T) {
	_, err := New(t.TempDir()).PingContext(context.Background(), "ignored")
	if !errors.Is(err, ErrPingUnsupported) {
		t.Fatalf("PingContext() error = %v, want ErrPingUnsupported", err)
	}
}

type closeRaceListener struct {
	conn    net.Conn
	started chan struct{}
	release chan struct{}
	once    sync.Once
}

func (listener *closeRaceListener) Accept() (net.Conn, error) {
	close(listener.started)
	<-listener.release
	return listener.conn, nil
}

func (listener *closeRaceListener) Close() error {
	listener.once.Do(func() { close(listener.release) })
	return nil
}

func (listener *closeRaceListener) Addr() net.Addr { return testNetworkAddr("close-race") }

type noopCloser struct{}

func (noopCloser) Close() error { return nil }

type testNetworkAddr string

func (address testNetworkAddr) Network() string { return "test" }
func (address testNetworkAddr) String() string  { return string(address) }

var _ io.Closer = noopCloser{}
