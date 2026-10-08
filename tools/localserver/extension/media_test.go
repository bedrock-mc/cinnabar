package extension

import (
	"crypto/tls"
	"crypto/x509"
	"io"
	"log/slog"
	"net/http"
	"os"
	"path/filepath"
	"testing"
)

func mediaClient(t *testing.T, caPath string) *http.Client {
	t.Helper()
	pemBytes, err := os.ReadFile(caPath)
	if err != nil {
		t.Fatal(err)
	}
	pool := x509.NewCertPool()
	if !pool.AppendCertsFromPEM(pemBytes) {
		t.Fatal("CA file holds no certificate")
	}
	return &http.Client{Transport: &http.Transport{
		TLSClientConfig:    &tls.Config{RootCAs: pool},
		DisableCompression: true,
	}}
}

// The media server verifies against its written CA for 127.0.0.1 and answers ranges with an
// exact 206 and Content-Range, unencoded; directories and missing files are not found.
func TestServeMediaRanges(t *testing.T) {
	dir := t.TempDir()
	body := make([]byte, 1000)
	for i := range body {
		body[i] = byte(i)
	}
	if err := os.WriteFile(filepath.Join(dir, "intro.webm"), body, 0o644); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(filepath.Join(dir, "sub"), 0o755); err != nil {
		t.Fatal(err)
	}
	caPath := filepath.Join(t.TempDir(), MediaCAFile)
	m, err := ServeMedia(dir, "127.0.0.1:0", caPath, slog.New(slog.DiscardHandler))
	if err != nil {
		t.Fatal(err)
	}
	defer m.Close()
	client := mediaClient(t, caPath)
	base := "https://" + m.Addr().String()

	req, _ := http.NewRequest(http.MethodGet, base+"/intro.webm", nil)
	req.Header.Set("Range", "bytes=100-299")
	req.Header.Set("Accept-Encoding", "identity")
	resp, err := client.Do(req)
	if err != nil {
		t.Fatal(err)
	}
	got, _ := io.ReadAll(resp.Body)
	resp.Body.Close()
	if resp.StatusCode != http.StatusPartialContent || resp.Header.Get("Content-Range") != "bytes 100-299/1000" ||
		resp.Header.Get("Content-Encoding") != "" || string(got) != string(body[100:300]) {
		t.Fatalf("status %d, range %q, encoding %q, %d bytes", resp.StatusCode, resp.Header.Get("Content-Range"), resp.Header.Get("Content-Encoding"), len(got))
	}
	for _, path := range []string{"/sub", "/", "/missing.webm", "/../intro.webm"} {
		resp, err := client.Get(base + path)
		if err != nil {
			t.Fatal(err)
		}
		resp.Body.Close()
		if resp.StatusCode != http.StatusNotFound {
			t.Errorf("%s: status %d, want 404", path, resp.StatusCode)
		}
	}
	// A directory handle left open past Close blocks removing the directory on Windows.
	if err := m.Close(); err != nil {
		t.Fatal(err)
	}
	if _, err := m.root.Stat("intro.webm"); err == nil {
		t.Error("media directory still open after Close")
	}
}

// Only IPv4 loopback addresses with a port make a media origin; the default port is omitted as
// the client's URL serialization does.
func TestMediaOrigin(t *testing.T) {
	if o, err := MediaOrigin(DefaultMediaAddr); err != nil || o != "https://"+DefaultMediaAddr {
		t.Fatalf("%q, %v", o, err)
	}
	if o, err := MediaOrigin("127.0.0.1:443"); err != nil || o != "https://127.0.0.1" {
		t.Fatalf("%q, %v", o, err)
	}
	for _, bad := range []string{"0.0.0.0:19443", "192.168.1.2:19443", "[::1]:19443", "localhost:19443", "127.0.0.1:0", "127.0.0.1"} {
		if _, err := MediaOrigin(bad); err == nil {
			t.Errorf("%s accepted", bad)
		}
	}
}
