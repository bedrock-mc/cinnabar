//go:build joinprofile

package joinprofile

import (
	"crypto/tls"
	"github.com/hashimthearab/rust-mcbe/core/internal/redact"
	"log/slog"
	"net/http"
	"net/http/httptrace"
	"sync/atomic"
	"time"
)

// joinProfileTransport records request timing without request data or credentials.
type joinProfileTransport struct{ base http.RoundTripper }

var joinProfileRequestID atomic.Uint64

// init installs HTTP timings in tagged developer cores; normal builds have no wrapper.
func init() { http.DefaultTransport = joinProfileTransport{base: http.DefaultTransport} }

// RoundTrip times DNS, TCP, TLS and response headers without consuming the response.
func (p joinProfileTransport) RoundTrip(r *http.Request) (*http.Response, error) {
	if !slog.Default().Enabled(r.Context(), slog.LevelDebug) {
		return p.base.RoundTrip(r)
	}
	id, start := joinProfileRequestID.Add(1), time.Now()
	mark := func(stage string) {
		Logger(r.Context(), slog.Default()).Debug("join HTTP timing", "http_request_id", id, "host", redact.Private(r.URL.Hostname()), "stage", stage, "elapsed_ms", float64(time.Since(start).Microseconds())/1000)
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
	response, err := p.base.RoundTrip(r.WithContext(httptrace.WithClientTrace(r.Context(), trace)))
	status := 0
	if response != nil {
		status = response.StatusCode
	}
	Logger(r.Context(), slog.Default()).Debug("join HTTP timing", "http_request_id", id, "host", redact.Private(r.URL.Hostname()), "stage", "response_headers", "status", status, "elapsed_ms", float64(time.Since(start).Microseconds())/1000, "success", err == nil)
	return response, err
}

// CloseIdleConnections preserves the default transport's idle-connection cleanup.
func (p joinProfileTransport) CloseIdleConnections() {
	if closer, ok := p.base.(interface{ CloseIdleConnections() }); ok {
		closer.CloseIdleConnections()
	}
}
