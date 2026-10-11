package streamnet

import (
	"context"
	"errors"
	"net"
	"os"
	"path/filepath"
	"runtime"
	"testing"
	"time"
)

func TestResolveMissingEndpoint(t *testing.T) {
	_, _, err := ResolveSession(t.TempDir())
	if err == nil {
		t.Fatal("ResolveSession() error = nil, want missing endpoint error")
	}
}

func TestResolveAndRoundTrip(t *testing.T) {
	dir := t.TempDir()
	listener, err := ListenSession(dir)
	if err != nil {
		t.Fatalf("Listen() error = %v", err)
	}
	defer listener.Close()

	network, address, err := ResolveSession(dir)
	if err != nil {
		t.Fatalf("ResolveSession() error = %v", err)
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
		if network != "unix" || address != unixEndpointPathNamed(dir, sessionUnixEndpointName) {
			t.Fatalf("ResolveSession() = %q, %q", network, address)
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
	client, err := (&net.Dialer{}).DialContext(ctx, network, address)
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
		_, err := NewFramedConn(client).Write([]byte{0xfe, 42})
		writeErr <- err
	}()
	got, err := NewFramedConn(server).ReadPacket()
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

func TestSessionAndControlEndpointsCoexistAndCloseIndependently(t *testing.T) {
	dir := t.TempDir()
	game, err := ListenSession(dir)
	if err != nil {
		t.Fatalf("listen game: %v", err)
	}
	defer game.Close()
	control, err := ListenControl(dir)
	if err != nil {
		t.Fatalf("listen control: %v", err)
	}

	gameNetwork, gameAddress, err := ResolveSession(dir)
	if err != nil {
		t.Fatalf("resolve game: %v", err)
	}
	controlNetwork, controlAddress, err := ResolveControl(dir)
	if err != nil {
		t.Fatalf("resolve control: %v", err)
	}
	if gameAddress == controlAddress {
		t.Fatalf("game and control endpoints collide at %q", gameAddress)
	}
	controlConn, err := net.Dial(controlNetwork, controlAddress)
	if err != nil {
		t.Fatalf("dial control: %v", err)
	}
	_ = controlConn.Close()
	if err := control.Close(); err != nil {
		t.Fatalf("close control: %v", err)
	}
	if _, _, err := ResolveControl(dir); err == nil {
		t.Fatal("closed control endpoint still resolves")
	}
	gameConn, err := net.Dial(gameNetwork, gameAddress)
	if err != nil {
		t.Fatalf("game endpoint broken by control close: %v", err)
	}
	_ = gameConn.Close()
}

func TestControlEndpointLeaseAllowsOneOwnerAndThenSuccessor(t *testing.T) {
	dir := t.TempDir()
	first, err := ListenControl(dir)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := ListenControl(dir); err == nil {
		t.Fatal("second control listener acquired live lease")
	}
	if err := first.Close(); err != nil {
		t.Fatal(err)
	}
	second, err := ListenControl(dir)
	if err != nil {
		t.Fatalf("successor control listener: %v", err)
	}
	if err := second.Close(); err != nil {
		t.Fatal(err)
	}
}

func TestResolveRejectsNonLoopbackAddress(t *testing.T) {
	if runtime.GOOS != "windows" {
		t.Skip("session.addr is Windows-only")
	}
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "session.addr"), []byte("0.0.0.0:19132\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	_, _, err := ResolveSession(dir)
	if err == nil {
		t.Fatal("ResolveSession() accepted a non-loopback address")
	}
}

func TestSessionListenerCleanup(t *testing.T) {
	dir := t.TempDir()
	listener, err := ListenSession(dir)
	if err != nil {
		t.Fatalf("Listen() error = %v", err)
	}
	if err := listener.Close(); err != nil && !errors.Is(err, net.ErrClosed) {
		t.Fatalf("Close() error = %v", err)
	}
	if err := listener.Close(); err != nil && !errors.Is(err, net.ErrClosed) {
		t.Fatalf("second Close() error = %v", err)
	}
	endpoint := unixEndpointPathNamed(dir, sessionUnixEndpointName)
	if runtime.GOOS == "windows" {
		endpoint = filepath.Join(dir, sessionWindowsEndpointName)
	}
	if _, err := os.Lstat(endpoint); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("endpoint remains after Close(): %v", err)
	}
}

func TestEndpointLeaseAllowsExactlyOneSimultaneousOwner(t *testing.T) {
	dir := t.TempDir()
	start := make(chan struct{})
	type result struct {
		listener net.Listener
		err      error
	}
	results := make(chan result, 2)
	for range 2 {
		go func() {
			<-start
			listener, err := ListenSession(dir)
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

	networkName, address, err := ResolveSession(dir)
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
	first, err := ListenSession(dir)
	if err != nil {
		t.Fatalf("first Listen(): %v", err)
	}
	if err := first.Close(); err != nil && !errors.Is(err, net.ErrClosed) {
		t.Fatalf("close first listener: %v", err)
	}
	if _, err := os.Stat(filepath.Join(dir, "session.lock")); err != nil {
		t.Fatalf("stable lease file missing after Close(): %v", err)
	}

	successor, err := ListenSession(dir)
	if err != nil {
		t.Fatalf("successor Listen(): %v", err)
	}
	defer successor.Close()
	_ = first.Close()
	networkName, address, err := ResolveSession(dir)
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
	path := unixEndpointPathNamed(dir, sessionUnixEndpointName)
	contents := []byte("not a socket")
	if runtime.GOOS == "windows" {
		path = filepath.Join(dir, sessionWindowsEndpointName)
		contents = []byte("invalid publication")
	}
	if err := os.WriteFile(path, contents, 0o600); err != nil {
		t.Fatal(err)
	}
	if listener, err := ListenSession(dir); err == nil {
		_ = listener.Close()
		t.Fatal("Listen() accepted an invalid existing endpoint")
	}
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	listener, err := ListenSession(dir)
	if err != nil {
		t.Fatalf("Listen() after constructor failure: %v", err)
	}
	_ = listener.Close()
}
