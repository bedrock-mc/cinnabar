package control

import (
	"encoding/binary"
	"encoding/json"
	"io"
	"net"
	"os"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/streamnet"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

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
