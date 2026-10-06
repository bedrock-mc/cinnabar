package catalog

import (
	"context"
	"crypto/tls"
	"crypto/x509"
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"slices"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/hashimthearab/rust-mcbe/core/internal/imagecache"
	"github.com/sandertv/gophertunnel/minecraft/realms"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
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
			Body:       io.NopCloser(strings.NewReader("\x89PNG\r\n\x1a\nartwork")),
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
	if string(contents) != "\x89PNG\r\n\x1a\nartwork" {
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

// cacheArtworkFileWithTransport tests the catalog policy without reaching the public network.
func cacheArtworkFileWithTransport(ctx context.Context, directory, rawURL string, transport http.RoundTripper) (string, error) {
	cfg := artworkPolicy
	cfg.Transport = transport
	image, err := imagecache.New(directory, cfg).Fetch(ctx, rawURL)
	return image.Path, err
}

type fixedRealmsXSTS struct{}

func (fixedRealmsXSTS) Token() (*oauth2.Token, error) { return nil, io.EOF }

func (fixedRealmsXSTS) XSTSToken(context.Context, string) (*xsts.Token, error) {
	token := new(xsts.Token)
	err := json.Unmarshal([]byte(`{"Token":"synthetic","NotAfter":"`+time.Now().Add(time.Hour).Format(time.RFC3339)+
		`","DisplayClaims":{"xui":[{"uhs":"hash"}]}}`), token)
	return token, err
}

// Listing Realms never joins one; the game joins only when the player picks a Realm.
func TestRealmsListingDoesNotJoin(t *testing.T) {
	var joins []string
	client := realms.NewClient(fixedRealmsXSTS{}, &http.Client{Transport: roundTripFunc(func(req *http.Request) (*http.Response, error) {
		body := `{"servers":[{"id":7,"name":"R","state":"OPEN"}]}`
		if req.Method != http.MethodGet {
			joins = append(joins, req.Method+" "+req.URL.Path)
			body = `{"address":"1.2.3.4:19132","networkProtocol":"DEFAULT"}`
		}
		return &http.Response{StatusCode: http.StatusOK, Header: make(http.Header), Body: io.NopCloser(strings.NewReader(body))}, nil
	})})
	list, err := listRealms(context.Background(), client)
	if err != nil || len(list) != 1 || list[0].Target != "realm_id/7" {
		t.Fatalf("realms = %+v, err = %v", list, err)
	}
	if len(joins) != 0 {
		t.Fatalf("listing sent %v", joins)
	}
}

// The account's Realms client is built once, on the discovered endpoint, so its version
// negotiation and token cache last across calls.
func TestRealmsClientIsSharedOnTheDiscoveredEndpoint(t *testing.T) {
	var paths []string
	server := httptest.NewTLSServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		paths = append(paths, r.Host+r.URL.Path)
		_, _ = io.WriteString(w, `{"servers":[]}`)
	}))
	defer server.Close()
	discovery := &service.Discovery{ServiceEnvironments: map[string]map[string]json.RawMessage{
		"realms_frontend_bedrock_legacy": {"prod": json.RawMessage(`{"serviceUri":"` + server.URL + `"}`)},
	}}
	var cache realmsClients
	first, err := cache.get(discovery, fixedRealmsXSTS{}, server.Client())
	if err != nil {
		t.Fatal(err)
	}
	if second, _ := cache.get(discovery, fixedRealmsXSTS{}, server.Client()); second != first {
		t.Fatal("a second call built another Realms client")
	}
	if _, err := listRealms(context.Background(), first); err != nil || len(paths) != 1 || paths[0] != strings.TrimPrefix(server.URL, "https://")+"/worlds" {
		t.Fatalf("paths = %v err = %v", paths, err)
	}
}

// A discovered Realms endpoint that is not absolute https is refused rather than used.
func TestRealmsClientRefusesAnInsecureEndpoint(t *testing.T) {
	discovery := &service.Discovery{ServiceEnvironments: map[string]map[string]json.RawMessage{
		"realms_frontend_bedrock_legacy": {"prod": json.RawMessage(`{"serviceUri":"http://realms.example.test"}`)},
	}}
	var cache realmsClients
	if client, err := cache.get(discovery, fixedRealmsXSTS{}, nil); err == nil || client != nil {
		t.Fatalf("client = %v err = %v, want a refusal", client, err)
	}
}
