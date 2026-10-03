package ingress

import (
	"bytes"
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func TestVerifiedAssetsCacheCompressionAndReplacement(t *testing.T) {
	dir := t.TempDir()
	manifest := AssetManifest{Version: 1, Protocol: 1, Source: "test"}
	data := []byte("original carrier bytes")
	for _, name := range append(append([]string(nil), requiredAssetNames...), optionalAssetNames...) {
		hash := sha256.Sum256(append([]byte(name), data...))
		content := append([]byte(name), data...)
		record := AssetRecord{Name: name, File: name + ".bin", SHA256: hex.EncodeToString(hash[:]), Size: int64(len(content))}
		manifest.Files = append(manifest.Files, record)
		if err := os.WriteFile(filepath.Join(dir, record.File), content, 0600); err != nil {
			t.Fatal(err)
		}
		var compressed bytes.Buffer
		writer := gzip.NewWriter(&compressed)
		_, _ = writer.Write(content)
		_ = writer.Close()
		if err := os.WriteFile(filepath.Join(dir, record.File+".gz"), compressed.Bytes(), 0600); err != nil {
			t.Fatal(err)
		}
	}
	encoded, _ := json.Marshal(manifest)
	if err := os.WriteFile(filepath.Join(dir, "manifest.json"), encoded, 0600); err != nil {
		t.Fatal(err)
	}
	assets, err := LoadAssets(dir)
	if err != nil {
		t.Fatal(err)
	}
	defer assets.Close()
	h, _ := testHandler(t)
	h.assets = assets
	for path, count := range map[string]int{"/api/spectator/assets": len(requiredAssetNames), "/api/spectator/assets/manifest-v10": len(manifest.Files)} {
		response := httptest.NewRecorder()
		h.ServeHTTP(response, httptest.NewRequest(http.MethodGet, path, nil))
		var served AssetManifest
		if response.Code != http.StatusOK || json.Unmarshal(response.Body.Bytes(), &served) != nil || len(served.Files) != count {
			t.Fatalf("renderer manifest %s: status=%d files=%d; want %d", path, response.Code, len(served.Files), count)
		}
	}
	record := manifest.Files[0]
	route := "/api/spectator/assets/" + record.SHA256 + "/" + record.File
	request := httptest.NewRequest(http.MethodGet, route, nil)
	request.Header.Set("Accept-Encoding", "gzip")
	response := httptest.NewRecorder()
	h.ServeHTTP(response, request)
	if response.Code != 200 || response.Header().Get("Content-Encoding") != "gzip" {
		t.Fatalf("gzip asset unavailable: %d %v", response.Code, response.Header())
	}
	reader, err := gzip.NewReader(response.Body)
	if err != nil {
		t.Fatal(err)
	}
	decoded, _ := io.ReadAll(reader)
	_ = reader.Close()
	if !bytes.Equal(decoded, append([]byte("world"), data...)) {
		t.Fatal("asset changed")
	}
	conditional := httptest.NewRequest(http.MethodGet, route, nil)
	conditional.Header.Set("Accept-Encoding", "gzip")
	conditional.Header.Set("If-None-Match", response.Header().Get("ETag"))
	cached := httptest.NewRecorder()
	h.ServeHTTP(cached, conditional)
	if cached.Code != 304 || cached.Body.Len() != 0 {
		t.Fatal("immutable conditional request failed")
	}
	// Atomic deployment replacement must not change the verified open inode.
	replacement := filepath.Join(dir, "replacement")
	_ = os.WriteFile(replacement, []byte("invalid replacement"), 0600)
	if err := os.Rename(replacement, filepath.Join(dir, record.File)); err != nil {
		t.Fatal(err)
	}
	plain := httptest.NewRecorder()
	h.ServeHTTP(plain, httptest.NewRequest(http.MethodGet, route, nil))
	if !bytes.Equal(plain.Body.Bytes(), append([]byte("world"), data...)) {
		t.Fatal("unverified deployment bytes served")
	}
	if invalid, err := LoadAssets(dir); err == nil {
		invalid.Close()
		t.Fatal("hash or size mismatch accepted")
	}
	// Download saturation is bounded and immediate, rather than a waiting queue.
	h.downloads <- struct{}{}
	h.downloads <- struct{}{}
	busy := httptest.NewRecorder()
	h.ServeHTTP(busy, httptest.NewRequest(http.MethodGet, route, nil))
	if busy.Code != 429 {
		t.Fatal("saturated download did not return429")
	}
	<-h.downloads
	<-h.downloads
}
