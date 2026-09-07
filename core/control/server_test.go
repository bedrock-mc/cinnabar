package control

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"io"
	"net"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

const testRequestIOTimeout = 100 * time.Millisecond

func TestBridgeCompatibilityStatusHelper(t *testing.T) {
	if os.Getenv("RUST_MCBE_BRIDGE_STATUS_HELPER") != "1" {
		t.Skip("bridge cross-language helper")
	}
	dir := os.Getenv("RUST_MCBE_BRIDGE_STATUS_SOCKET_DIR")
	if dir == "" {
		t.Fatal("RUST_MCBE_BRIDGE_STATUS_SOCKET_DIR is required")
	}
	store := NewStore()
	store.SetLifecycle(LifecycleRunning)
	latest := snapshot(17, proxy.ResourcePackOfferRequired)
	latest.PackCount, latest.TotalBytes = 1, 512
	latest.Acquisition = proxy.ResourcePackAcquisitionIgnored
	latest.DownstreamOutcome = proxy.ResourcePackDownstreamStrippedIgnored
	store.Observe(latest)
	server, err := Start(dir, store)
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	_, _ = io.Copy(io.Discard, os.Stdin)
}

func TestStatusV1RoundTripAndSecretSafeWireShape(t *testing.T) {
	store := NewStore()
	store.SetLifecycle(LifecycleRunning)
	latest := snapshot(9, proxy.ResourcePackOfferOptional)
	latest.PackCount, latest.TotalBytes = 2, 4096
	latest.Acquisition = proxy.ResourcePackAcquisitionIgnored
	latest.CacheLoads, latest.CacheHits, latest.CacheMisses = 2, 1, 1
	latest.CacheStores = 1
	latest.DownstreamOutcome = proxy.ResourcePackDownstreamStrippedIgnored
	store.Observe(latest)

	dir := t.TempDir()
	server, err := Start(dir, store)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":1,"method":"status.v1"}`))
	for _, want := range []string{`"schema_version":1`, `"lifecycle":"running"`, `"attempt_id":9`, `"pack_count":2`, `"total_bytes":4096`, `"acquisition":"ignored"`, `"downstream_outcome":"stripped_ignored"`, `"application":"unavailable"`} {
		if !strings.Contains(string(payload), want) {
			t.Fatalf("response %s does not contain %s", payload, want)
		}
	}
	for _, forbidden := range []string{`"uuid"`, `"version"`, `"url"`, `"content_key"`, `"key"`, `"digest"`, `"path"`} {
		if strings.Contains(strings.ToLower(string(payload)), forbidden) {
			t.Fatalf("response exposed forbidden field %q: %s", forbidden, payload)
		}
	}
}

func TestServerContinuesAfterMalformedAndUnknownClients(t *testing.T) {
	dir := t.TempDir()
	store := NewStore()
	server, err := Start(dir, store)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })

	malformed := exchange(t, dir, []byte("{"))
	assertRPCError(t, malformed, -32700)
	unknown := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":7,"method":"events.v1"}`))
	assertRPCError(t, unknown, -32601)
	missingID := exchange(t, dir, []byte(`{"jsonrpc":"2.0","method":"status.v1"}`))
	assertRPCError(t, missingID, -32600)
	params := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":7,"method":"status.v1","params":{}}`))
	assertRPCError(t, params, -32602)
	valid := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":8,"method":"status.v1"}`))
	if !strings.Contains(string(valid), `"schema_version":1`) || !strings.Contains(string(valid), `"id":8`) {
		t.Fatalf("valid response after bad clients = %s", valid)
	}
}

func TestOversizedClientCannotTerminateServer(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })

	conn := dialControl(t, dir)
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], MaxFrameLen+1)
	if _, err := conn.Write(header[:]); err != nil {
		t.Fatal(err)
	}
	_ = conn.Close()
	valid := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":1,"method":"status.v1"}`))
	if !strings.Contains(string(valid), `"result"`) {
		t.Fatalf("server did not survive oversized client: %s", valid)
	}
}

func TestSilentClientCannotDenyNextStatusClient(t *testing.T) {
	dir := t.TempDir()
	server, err := startWithRequestIOTimeout(dir, NewStore(), testRequestIOTimeout)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })

	silent := dialControl(t, dir)
	defer silent.Close()
	waitForActiveConnection(t, server)

	started := time.Now()
	valid := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":21,"method":"status.v1"}`))
	if !strings.Contains(string(valid), `"id":21`) {
		t.Fatalf("valid response after silent client = %s", valid)
	}
	if elapsed := time.Since(started); elapsed > 10*testRequestIOTimeout {
		t.Fatalf("next client waited %v after silent client", elapsed)
	}
}

func TestPartialFrameCannotDenyNextStatusClient(t *testing.T) {
	dir := t.TempDir()
	server, err := startWithRequestIOTimeout(dir, NewStore(), testRequestIOTimeout)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })

	partial := dialControl(t, dir)
	defer partial.Close()
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], 128)
	if _, err := partial.Write(append(header[:], []byte(`{"jsonrpc":`)...)); err != nil {
		t.Fatal(err)
	}
	waitForActiveConnection(t, server)

	valid := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":22,"method":"status.v1"}`))
	if !strings.Contains(string(valid), `"id":22`) {
		t.Fatalf("valid response after partial frame = %s", valid)
	}
}

func TestNonReadingClientResponseWriteIsBounded(t *testing.T) {
	serverConn, clientConn := net.Pipe()
	defer serverConn.Close()
	defer clientConn.Close()
	server := &Server{store: NewStore(), requestIOTimeout: testRequestIOTimeout}
	request := []byte(`{"jsonrpc":"2.0","id":23,"method":"status.v1"}`)
	writeDone := make(chan error, 1)
	go func() {
		var header [4]byte
		binary.BigEndian.PutUint32(header[:], uint32(len(request)))
		_, err := clientConn.Write(append(header[:], request...))
		writeDone <- err
	}()

	started := time.Now()
	err := server.serveOne(serverConn)
	var netErr net.Error
	if !errors.As(err, &netErr) || !netErr.Timeout() {
		t.Fatalf("serveOne() error = %v, want write timeout", err)
	}
	if elapsed := time.Since(started); elapsed > 10*testRequestIOTimeout {
		t.Fatalf("blocked response write took %v", elapsed)
	}
	if err := <-writeDone; err != nil {
		t.Fatalf("request write error = %v", err)
	}
}

func TestReadDeadlineFailureRejectsConnectionBeforeReading(t *testing.T) {
	want := errors.New("deadline unavailable")
	conn := &deadlineFailureConn{readDeadlineErr: want}
	server := &Server{store: NewStore(), requestIOTimeout: testRequestIOTimeout}
	if err := server.serveOne(conn); !errors.Is(err, want) {
		t.Fatalf("serveOne() error = %v, want %v", err, want)
	}
	if conn.reads != 0 {
		t.Fatalf("serveOne() performed %d reads after deadline failure", conn.reads)
	}
}

func TestWriteDeadlineFailureRejectsResponseBeforeWriting(t *testing.T) {
	serverConn, clientConn := net.Pipe()
	conn := &writeDeadlineFailureConn{Conn: serverConn, err: errors.New("deadline unavailable")}
	defer conn.Close()
	defer clientConn.Close()
	server := &Server{store: NewStore(), requestIOTimeout: testRequestIOTimeout}
	request := []byte(`{"jsonrpc":"2.0","id":24,"method":"status.v1"}`)
	writeDone := make(chan error, 1)
	go func() {
		var header [4]byte
		binary.BigEndian.PutUint32(header[:], uint32(len(request)))
		_, err := clientConn.Write(append(header[:], request...))
		writeDone <- err
	}()

	if err := server.serveOne(conn); !errors.Is(err, conn.err) {
		t.Fatalf("serveOne() error = %v, want %v", err, conn.err)
	}
	if conn.writes != 0 {
		t.Fatalf("serveOne() performed %d writes after deadline failure", conn.writes)
	}
	if err := <-writeDone; err != nil {
		t.Fatalf("request write error = %v", err)
	}
}

func TestCloseUnblocksSlowActiveClientAndRemovesEndpoint(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	conn := dialControl(t, dir)
	if _, err := conn.Write([]byte{0, 0}); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() { done <- server.Close() }()
	select {
	case err := <-done:
		if err != nil {
			t.Fatalf("Close() = %v", err)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("Close blocked on slow client")
	}
	_ = conn.Close()
	if _, _, err := streamnet.ResolveControl(dir); err == nil {
		t.Fatal("control endpoint remained resolvable after shutdown")
	}
}

func exchange(t *testing.T, dir string, request []byte) []byte {
	t.Helper()
	conn := dialControl(t, dir)
	defer conn.Close()
	if err := conn.SetDeadline(time.Now().Add(time.Second)); err != nil {
		t.Fatal(err)
	}
	var header [4]byte
	binary.BigEndian.PutUint32(header[:], uint32(len(request)))
	if _, err := conn.Write(append(header[:], request...)); err != nil {
		t.Fatal(err)
	}
	if _, err := io.ReadFull(conn, header[:]); err != nil {
		t.Fatal(err)
	}
	length := binary.BigEndian.Uint32(header[:])
	payload := make([]byte, length)
	if _, err := io.ReadFull(conn, payload); err != nil {
		t.Fatal(err)
	}
	return payload
}

func waitForActiveConnection(t *testing.T, server *Server) {
	t.Helper()
	deadline := time.Now().Add(time.Second)
	for time.Now().Before(deadline) {
		server.mu.Lock()
		active := server.active != nil
		server.mu.Unlock()
		if active {
			return
		}
		time.Sleep(time.Millisecond)
	}
	t.Fatal("server did not accept control connection")
}

type deadlineFailureConn struct {
	readDeadlineErr error
	reads           int
}

func (conn *deadlineFailureConn) Read([]byte) (int, error) {
	conn.reads++
	return 0, io.EOF
}

func (*deadlineFailureConn) Write(payload []byte) (int, error) { return len(payload), nil }
func (*deadlineFailureConn) Close() error                      { return nil }
func (*deadlineFailureConn) LocalAddr() net.Addr               { return testControlAddr("local") }
func (*deadlineFailureConn) RemoteAddr() net.Addr              { return testControlAddr("remote") }
func (*deadlineFailureConn) SetDeadline(time.Time) error       { return nil }
func (conn *deadlineFailureConn) SetReadDeadline(time.Time) error {
	return conn.readDeadlineErr
}
func (*deadlineFailureConn) SetWriteDeadline(time.Time) error { return nil }

type writeDeadlineFailureConn struct {
	net.Conn
	err    error
	writes int
}

func (conn *writeDeadlineFailureConn) Write(payload []byte) (int, error) {
	conn.writes++
	return conn.Conn.Write(payload)
}

func (conn *writeDeadlineFailureConn) SetWriteDeadline(time.Time) error { return conn.err }

type testControlAddr string

func (address testControlAddr) Network() string { return "control-test" }
func (address testControlAddr) String() string  { return string(address) }

func dialControl(t *testing.T, dir string) net.Conn {
	t.Helper()
	network, address, err := streamnet.ResolveControl(dir)
	if err != nil {
		t.Fatal(err)
	}
	conn, err := net.DialTimeout(network, address, time.Second)
	if err != nil {
		t.Fatal(err)
	}
	return conn
}

func assertRPCError(t *testing.T, payload []byte, code int) {
	t.Helper()
	var response struct {
		Error *responseError `json:"error"`
	}
	if err := json.Unmarshal(payload, &response); err != nil {
		t.Fatal(err)
	}
	if response.Error == nil || response.Error.Code != code {
		t.Fatalf("RPC error = %s, want %d", payload, code)
	}
}
