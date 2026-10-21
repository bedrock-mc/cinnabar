//go:build joinprofile

package joinprofile

import (
	"context"
	"crypto/tls"
	"log/slog"
	"net/http"
	"net/http/httptrace"
	"sync/atomic"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/redact"
)

// joinProfileTransport records request timing without request data or credentials.
type joinProfileTransport struct {
	base  http.RoundTripper
	owner attempt
}

// httpProfileKey prevents nested diagnostic transports from timing a request twice.
type httpProfileKey struct{}

var joinProfileRequestID atomic.Uint64

// init installs HTTP timings in tagged developer cores; normal builds have no wrapper.
func init() { http.DefaultTransport = joinProfileTransport{base: http.DefaultTransport} }

// HTTPClient binds an attempt's identity without retaining its cancellation context.
// The shared client stays unchanged; requests keep their own values and lifetime.
func HTTPClient(ctx context.Context, client *http.Client) *http.Client {
	owner, ok := ctx.Value(contextKey{}).(attempt)
	if !ok {
		return client
	}
	if client == nil {
		client = http.DefaultClient
	}
	bound := *client
	base := client.Transport
	if base == nil {
		base = http.DefaultTransport
	}
	bound.Transport = joinProfileTransport{base: base, owner: owner}
	return &bound
}

// RoundTrip times DNS, TCP, TLS and response headers without consuming the response.
func (p joinProfileTransport) RoundTrip(r *http.Request) (*http.Response, error) {
	ctx := r.Context()
	if !slog.Default().Enabled(ctx, slog.LevelDebug) || ctx.Value(httpProfileKey{}) != nil {
		return p.base.RoundTrip(r)
	}
	if _, ok := ctx.Value(contextKey{}).(attempt); !ok && p.owner.id != 0 {
		ctx = context.WithValue(ctx, contextKey{}, p.owner)
	}
	logger := Logger(ctx, slog.Default())
	id, start := joinProfileRequestID.Add(1), time.Now()
	mark := func(stage string) {
		logger.Debug("join HTTP timing", "http_request_id", id, "host", redact.Private(r.URL.Hostname()), "stage", stage, "elapsed_ms", float64(time.Since(start).Microseconds())/1000)
	}
	trace := &httptrace.ClientTrace{
		DNSStart:          func(httptrace.DNSStartInfo) { mark("dns_start") },
		DNSDone:           func(httptrace.DNSDoneInfo) { mark("dns_done") },
		ConnectStart:      func(string, string) { mark("tcp_start") },
		ConnectDone:       func(string, string, error) { mark("tcp_done") },
		TLSHandshakeStart: func() { mark("tls_start") },
		TLSHandshakeDone:  func(tls.ConnectionState, error) { mark("tls_done") },
		GotConn: func(info httptrace.GotConnInfo) {
			if info.Reused {
				mark("connection_reused")
			} else {
				mark("connection_ready")
			}
		},
		WroteRequest:         func(httptrace.WroteRequestInfo) { mark("request_written") },
		GotFirstResponseByte: func() { mark("first_response_byte") },
	}
	mark("start")
	ctx = context.WithValue(ctx, httpProfileKey{}, true)
	response, err := p.base.RoundTrip(r.WithContext(httptrace.WithClientTrace(ctx, trace)))
	status := 0
	if response != nil {
		status = response.StatusCode
	}
	logger.Debug("join HTTP timing", "http_request_id", id, "host", redact.Private(r.URL.Hostname()), "stage", "response_headers", "status", status, "elapsed_ms", float64(time.Since(start).Microseconds())/1000, "success", err == nil)
	return response, err
}

// CloseIdleConnections preserves the default transport's idle-connection cleanup.
func (p joinProfileTransport) CloseIdleConnections() {
	if closer, ok := p.base.(interface{ CloseIdleConnections() }); ok {
		closer.CloseIdleConnections()
	}
}
