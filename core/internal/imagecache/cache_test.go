package imagecache

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"fmt"
	"net"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"runtime"
	"sync/atomic"
	"testing"
	"time"
)

const pngHeader = "\x89PNG\r\n\x1a\n"

// newTestCache trusts the test server and routes every dial to it.
func newTestCache(t *testing.T, handler http.HandlerFunc) (*Cache, *httptest.Server) {
	t.Helper()
	server := httptest.NewTLSServer(handler)
	t.Cleanup(server.Close)
	cfg := testConfig()
	cfg.Transport = &http.Transport{DialContext: func(ctx context.Context, network, _ string) (net.Conn, error) {
		return (&net.Dialer{}).DialContext(ctx, network, server.Listener.Addr().String())
	}}
	cache := New(t.TempDir(), cfg)
	t.Cleanup(cache.http.CloseIdleConnections)
	pool := x509.NewCertPool()
	pool.AddCert(server.Certificate())
	cache.http.Transport.(*http.Transport).TLSClientConfig = &tls.Config{RootCAs: pool}
	return cache, server
}

func TestCacheStoresAndReusesAnImage(t *testing.T) {
	var hits atomic.Int32
	cache, _ := newTestCache(t, func(w http.ResponseWriter, r *http.Request) {
		hits.Add(1)
		_, _ = w.Write([]byte(pngHeader + "payload"))
	})
	first, err := cache.Fetch(context.Background(), "https://example.com/a.png")
	if err != nil || first.ContentType != "image/png" || filepath.Ext(first.Path) != ".png" {
		t.Fatalf("first = %+v err=%v", first, err)
	}
	info, err := os.Stat(first.Path)
	if err != nil || !info.Mode().IsRegular() {
		t.Fatalf("stat = %v err=%v", info, err)
	}
	// Windows exposes a read-only attribute rather than Unix permission bits.
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("cache permissions = %o, want 600", info.Mode().Perm())
	}
	second, err := cache.Fetch(context.Background(), "https://example.com/a.png")
	if err != nil || second != first || hits.Load() != 1 {
		t.Fatalf("second = %+v err=%v hits=%d", second, err, hits.Load())
	}
}

func TestCacheRejectsUnsafeInputs(t *testing.T) {
	cache, _ := newTestCache(t, func(w http.ResponseWriter, r *http.Request) {
		switch r.URL.Path {
		case "/text":
			_, _ = w.Write([]byte("<html>not an image</html>"))
		case "/big":
			_, _ = w.Write(append([]byte(pngHeader), make([]byte, int(testConfig().MaxBytes))...))
		case "/missing":
			http.NotFound(w, r)
		case "/redirect":
			http.Redirect(w, r, "http://example.com/plain.png", http.StatusFound)
		}
	})
	for _, raw := range []string{
		"http://example.com/a.png", "ftp://example.com/a.png", "https://user@example.com/a.png", "https:///a.png",
		"::", "https://example.com/text", "https://example.com/big", "https://example.com/missing", "https://example.com/redirect",
	} {
		if _, err := cache.Fetch(context.Background(), raw); err == nil {
			t.Errorf("%q was accepted", raw)
		}
	}
	entries, _ := os.ReadDir(cache.dir)
	for _, e := range entries {
		t.Errorf("rejected input left %s behind", e.Name())
	}
}

func TestPublicDialerRefusesNonPublicAddresses(t *testing.T) {
	dial := publicDialer(net.DefaultResolver.LookupIPAddr, (&net.Dialer{}).DialContext)
	for _, addr := range []string{"127.0.0.1:443", "10.0.0.1:443", "192.168.1.1:443", "169.254.169.254:80", "[::1]:443", "0.0.0.0:80"} {
		ctx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		conn, err := dial(ctx, "tcp", addr)
		cancel()
		if conn != nil {
			_ = conn.Close()
		}
		if !errors.Is(err, ErrRejected) {
			t.Errorf("%s: err = %v", addr, err)
		}
	}
}

func TestCacheEvictsLeastRecentlyUsedBeyondTheFileBound(t *testing.T) {
	cache := New(t.TempDir(), testConfig())
	base := time.Now().Add(-time.Hour)
	for i := 0; i < cache.cfg.MaxFiles+3; i++ {
		path := filepath.Join(cache.dir, fmt.Sprintf("f%04d.png", i))
		if err := os.WriteFile(path, []byte("x"), 0o600); err != nil {
			t.Fatal(err)
		}
		mod := base.Add(time.Duration(i) * time.Second)
		if err := os.Chtimes(path, mod, mod); err != nil {
			t.Fatal(err)
		}
	}
	cache.evictLocked()
	entries, _ := os.ReadDir(cache.dir)
	if len(entries) != cache.cfg.MaxFiles {
		t.Fatalf("files = %d", len(entries))
	}
	if _, err := os.Stat(filepath.Join(cache.dir, "f0000.png")); !os.IsNotExist(err) {
		t.Fatalf("oldest file survived: %v", err)
	}
}

// testConfig uses small limits so boundary tests stay fast.
func testConfig() Config {
	return Config{MaxBytes: 1024, MaxFiles: 4, MaxDirBytes: 4096, Timeout: time.Second, MaxRedirects: 3, MaxURLBytes: 1024}
}

func TestCacheRedownloadsAnInvalidExistingFile(t *testing.T) {
	var hits atomic.Int32
	cache, _ := newTestCache(t, func(w http.ResponseWriter, _ *http.Request) {
		hits.Add(1)
		_, _ = w.Write([]byte(pngHeader + "payload"))
	})
	first, err := cache.Fetch(context.Background(), "https://example.com/image")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(first.Path, []byte("not an image"), 0o600); err != nil {
		t.Fatal(err)
	}
	second, err := cache.Fetch(context.Background(), "https://example.com/image")
	if err != nil || first != second || hits.Load() != 2 {
		t.Fatalf("redownload = %+v, %v, hits = %d", second, err, hits.Load())
	}
}

func TestCacheEvictsOldFilesToMeetTheByteLimit(t *testing.T) {
	cache := New(t.TempDir(), testConfig())
	for i := 0; i < 3; i++ {
		path := filepath.Join(cache.dir, fmt.Sprintf("%d.png", i))
		if err := os.WriteFile(path, make([]byte, cache.cfg.MaxDirBytes/2), 0o600); err != nil {
			t.Fatal(err)
		}
		stamp := time.Unix(int64(i), 0)
		if err := os.Chtimes(path, stamp, stamp); err != nil {
			t.Fatal(err)
		}
	}
	cache.Prune()
	entries, err := os.ReadDir(cache.dir)
	if err != nil || len(entries) != 2 {
		t.Fatalf("cache = %v, %v", entries, err)
	}
	if _, err := os.Stat(filepath.Join(cache.dir, "0.png")); !os.IsNotExist(err) {
		t.Fatalf("oldest remains: %v", err)
	}
}
