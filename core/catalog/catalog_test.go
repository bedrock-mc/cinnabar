package catalog

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
)

type roundTripFunc func(*http.Request) (*http.Response, error)

func (f roundTripFunc) RoundTrip(request *http.Request) (*http.Response, error) {
	return f(request)
}

func tlsTransport(t *testing.T, servers ...*httptest.Server) *http.Transport {
	t.Helper()
	roots := x509.NewCertPool()
	for _, server := range servers {
		roots.AddCert(server.Certificate())
	}
	transport := &http.Transport{TLSClientConfig: &tls.Config{RootCAs: roots}}
	t.Cleanup(transport.CloseIdleConnections)
	return transport
}

func assertEmptyDirectory(t *testing.T, directory string) {
	t.Helper()
	entries, err := os.ReadDir(directory)
	if err != nil {
		t.Fatal(err)
	}
	if len(entries) != 0 {
		t.Fatalf("download failure published files: %v", entries)
	}
}

func TestCacheArtworkRejectsHTTPSRedirectToHTTPBeforeDestination(t *testing.T) {
	var destinationRequests atomic.Int32
	destination := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		destinationRequests.Add(1)
		_, _ = io.WriteString(w, "must not be reached")
	}))
	defer destination.Close()

	source := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, destination.URL, http.StatusFound)
	}))
	defer source.Close()
	directory := t.TempDir()

	_, err := cacheArtworkFileWithTransport(
		context.Background(), directory, source.URL, tlsTransport(t, source),
	)
	if err == nil {
		t.Fatal("HTTPS to HTTP redirect unexpectedly succeeded")
	}
	if got := destinationRequests.Load(); got != 0 {
		t.Fatalf("HTTP destination received %d requests", got)
	}
	assertEmptyDirectory(t, directory)
}

func TestCacheArtworkAllowsCrossHostHTTPSRedirectChain(t *testing.T) {
	var hosts []string
	transport := roundTripFunc(func(request *http.Request) (*http.Response, error) {
		hosts = append(hosts, request.URL.Host)
		if got := request.Header.Get("User-Agent"); got != "Cinnabar/1.0" {
			t.Fatalf("User-Agent = %q", got)
		}
		response := &http.Response{
			StatusCode: http.StatusOK,
			Status:     "200 OK",
			Header:     make(http.Header),
			Body:       io.NopCloser(strings.NewReader("artwork")),
			Request:    request,
		}
		switch request.URL.Host {
		case "origin.example":
			response.StatusCode = http.StatusFound
			response.Status = "302 Found"
			response.Header.Set("Location", "https://edge.example/image")
			response.Body = http.NoBody
		case "edge.example":
			response.StatusCode = http.StatusTemporaryRedirect
			response.Status = "307 Temporary Redirect"
			response.Header.Set("Location", "https://cdn.example/image")
			response.Body = http.NoBody
		case "cdn.example":
		default:
			t.Fatalf("unexpected redirect host %q", request.URL.Host)
		}
		return response, nil
	})
	directory := t.TempDir()

	path, err := cacheArtworkFileWithTransport(
		context.Background(), directory, "https://origin.example/image", transport,
	)
	if err != nil {
		t.Fatal(err)
	}
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(contents) != "artwork" {
		t.Fatalf("cached payload = %q", contents)
	}
	if want := []string{"origin.example", "edge.example", "cdn.example"}; !slices.Equal(hosts, want) {
		t.Fatalf("redirect hosts = %v, want %v", hosts, want)
	}
}

func TestCacheArtworkRetainsTenRequestRedirectBound(t *testing.T) {
	var requests atomic.Int32
	var server *httptest.Server
	server = httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		requests.Add(1)
		http.Redirect(w, r, server.URL, http.StatusFound)
	}))
	defer server.Close()
	directory := t.TempDir()

	_, err := cacheArtworkFileWithTransport(
		context.Background(), directory, server.URL, tlsTransport(t, server),
	)
	if err == nil {
		t.Fatal("redirect loop unexpectedly succeeded")
	}
	if got := requests.Load(); got != 10 {
		t.Fatalf("redirect loop issued %d requests, want 10", got)
	}
	assertEmptyDirectory(t, directory)
}

func TestCacheArtworkFailuresNeverPublishPartialFiles(t *testing.T) {
	tests := []struct {
		name   string
		status int
		body   string
	}{
		{name: "status", status: http.StatusNotFound, body: "missing"},
		{name: "empty", status: http.StatusOK},
		{name: "oversized", status: http.StatusOK, body: strings.Repeat("x", maxArtworkBytes+1)},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
				w.WriteHeader(test.status)
				_, _ = io.WriteString(w, test.body)
			}))
			defer server.Close()
			directory := t.TempDir()

			_, err := cacheArtworkFileWithTransport(
				context.Background(), directory, server.URL, tlsTransport(t, server),
			)
			if err == nil {
				t.Fatal("invalid artwork response unexpectedly succeeded")
			}
			assertEmptyDirectory(t, directory)
		})
	}
}

func TestCacheArtworkRejectsInvalidInitialURLBeforeRequest(t *testing.T) {
	var requests atomic.Int32
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		requests.Add(1)
		_, _ = io.WriteString(w, "artwork")
	}))
	defer server.Close()
	directory := t.TempDir()

	_, err := cacheArtworkFileWithTransport(
		context.Background(), directory, server.URL, http.DefaultTransport,
	)
	if err == nil {
		t.Fatal("invalid initial URL unexpectedly succeeded")
	}
	if got := requests.Load(); got != 0 {
		t.Fatalf("invalid initial URL issued %d requests", got)
	}
	assertEmptyDirectory(t, directory)
}

func TestCacheArtworkHonorsCancelledContext(t *testing.T) {
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		_, _ = io.WriteString(w, "artwork")
	}))
	defer server.Close()
	directory := t.TempDir()
	ctx, cancel := context.WithCancel(context.Background())
	cancel()

	_, err := cacheArtworkFileWithTransport(ctx, directory, server.URL, tlsTransport(t, server))
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled download error = %v", err)
	}
	assertEmptyDirectory(t, directory)
}
