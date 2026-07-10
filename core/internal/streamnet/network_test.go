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

func TestNetworkPingUnsupported(t *testing.T) {
	_, err := New(t.TempDir()).PingContext(context.Background(), "ignored")
	if !errors.Is(err, ErrPingUnsupported) {
		t.Fatalf("PingContext() error = %v, want ErrPingUnsupported", err)
	}
}
