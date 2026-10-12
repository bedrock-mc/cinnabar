package webrelay

import (
	"bytes"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strconv"
	"testing"

	"github.com/klauspost/compress/gzip"
	"github.com/klauspost/compress/zstd"
)

func TestStaticFilesCompressRevalidateAndKeepRanges(t *testing.T) {
	dir := t.TempDir()
	body := bytes.Repeat([]byte("carrier pixels "), 4096)
	if err := os.WriteFile(filepath.Join(dir, "vanilla-v1.mcbeui"), body, 0o644); err != nil {
		t.Fatal(err)
	}
	server := httptest.NewServer(StaticFiles(dir))
	defer server.Close()
	get := func(header http.Header) (*http.Response, []byte) {
		t.Helper()
		request, _ := http.NewRequest(http.MethodGet, server.URL+"/vanilla-v1.mcbeui", nil)
		request.Header = header
		// The transport must not negotiate or decode on its own.
		response, err := (&http.Transport{DisableCompression: true}).RoundTrip(request)
		if err != nil {
			t.Fatal(err)
		}
		defer response.Body.Close()
		payload, err := io.ReadAll(response.Body)
		if err != nil {
			t.Fatal(err)
		}
		return response, payload
	}

	zstdResponse, compressed := get(http.Header{"Accept-Encoding": {"gzip, deflate, br, zstd"}})
	decoder, _ := zstd.NewReader(nil)
	decoded, err := decoder.DecodeAll(compressed, nil)
	if err != nil || !bytes.Equal(decoded, body) || zstdResponse.Header.Get("Content-Encoding") != "zstd" {
		t.Fatalf("zstd: encoding %q, err %v", zstdResponse.Header.Get("Content-Encoding"), err)
	}
	if len(compressed) >= len(body)/10 || zstdResponse.Header.Get(DecodedLengthHeader) != strconv.Itoa(len(body)) {
		t.Fatalf("zstd body %d bytes, decoded length %q", len(compressed), zstdResponse.Header.Get(DecodedLengthHeader))
	}

	gzipResponse, gzipped := get(http.Header{"Accept-Encoding": {"gzip, zstd;q=0"}})
	reader, err := gzip.NewReader(bytes.NewReader(gzipped))
	if err != nil || gzipResponse.Header.Get("Content-Encoding") != "gzip" {
		t.Fatalf("gzip: encoding %q, err %v", gzipResponse.Header.Get("Content-Encoding"), err)
	}
	if unzipped, _ := io.ReadAll(reader); !bytes.Equal(unzipped, body) {
		t.Fatal("gzip body does not decode to the file")
	}

	revalidated, _ := get(http.Header{"Accept-Encoding": {"zstd"}, "If-None-Match": {zstdResponse.Header.Get("ETag")}})
	if revalidated.StatusCode != http.StatusNotModified || zstdResponse.Header.Get("Cache-Control") != "no-cache" {
		t.Fatalf("revalidation status %d, cache control %q", revalidated.StatusCode, zstdResponse.Header.Get("Cache-Control"))
	}

	ranged, part := get(http.Header{"Accept-Encoding": {"zstd"}, "Range": {"bytes=0-6"}})
	if ranged.StatusCode != http.StatusPartialContent || ranged.Header.Get("Content-Encoding") != "" || string(part) != "carrier" {
		t.Fatalf("range: status %d, encoding %q, body %q", ranged.StatusCode, ranged.Header.Get("Content-Encoding"), part)
	}
}
