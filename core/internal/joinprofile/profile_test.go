//go:build joinprofile

package joinprofile

import (
	"bytes"
	"context"
	"io"
	"log/slog"
	"net/http"
	"net/http/httptrace"
	"strings"
	"testing"
)

// profileRoundTripper lets tests inspect trace attachment without making requests.
type profileRoundTripper func(*http.Request) (*http.Response, error)

// RoundTrip invokes the test transport.
func (f profileRoundTripper) RoundTrip(r *http.Request) (*http.Response, error) { return f(r) }

// TestHTTPProfileExcludesRequestData protects the diagnostic privacy boundary.
func TestHTTPProfileExcludesRequestData(t *testing.T) {
	var output bytes.Buffer
	old := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(&output, &slog.HandlerOptions{Level: slog.LevelDebug})))
	t.Cleanup(func() { slog.SetDefault(old) })
	ctx := Start(context.Background(), 42)
	request, _ := http.NewRequestWithContext(ctx, "POST", "https://private.example/sensitive-user?secret=credential", strings.NewReader("body-secret"))
	request.Header.Set("Authorization", "header-secret")
	transport := joinProfileTransport{base: profileRoundTripper(func(r *http.Request) (*http.Response, error) {
		trace := httptrace.ContextClientTrace(r.Context())
		if trace == nil {
			t.Fatal("missing timing hooks")
		}
		trace.GotConn(httptrace.GotConnInfo{Reused: true})
		trace.GotFirstResponseByte()
		return &http.Response{StatusCode: 200, Body: io.NopCloser(strings.NewReader("response-secret"))}, nil
	})}
	response, err := transport.RoundTrip(request)
	if err != nil {
		t.Fatal(err)
	}
	defer response.Body.Close()
	for _, secret := range []string{"private.example", "sensitive-user", "credential", "body-secret", "header-secret", "response-secret"} {
		if strings.Contains(output.String(), secret) {
			t.Fatalf("request data leaked: %s", secret)
		}
	}
	for _, field := range []string{"join_profile_id=42", "connection_reused", "first_response_byte", "response_headers", "elapsed_ms="} {
		if !strings.Contains(output.String(), field) {
			t.Fatalf("missing timing field %s", field)
		}
	}
}

// TestHTTPProfileDisabledAtInfo avoids installing per-request trace hooks when debug logs are off.
func TestHTTPProfileDisabledAtInfo(t *testing.T) {
	old := slog.Default()
	slog.SetDefault(slog.New(slog.NewTextHandler(io.Discard, nil)))
	t.Cleanup(func() { slog.SetDefault(old) })
	transport := joinProfileTransport{base: profileRoundTripper(func(r *http.Request) (*http.Response, error) {
		if httptrace.ContextClientTrace(r.Context()) != nil {
			t.Fatal("debug-disabled request acquired timing hooks")
		}
		return &http.Response{StatusCode: 200, Body: http.NoBody}, nil
	})}
	request, _ := http.NewRequest("GET", "https://example.invalid", nil)
	response, err := transport.RoundTrip(request)
	if err != nil {
		t.Fatal(err)
	}
	response.Body.Close()
}
