// Package webrelay serves Cinnabar's browser build: each browser gets an in-memory session whose
// Microsoft sign-in and Bedrock joins run here, with the core's session protocol carried over
// WebSockets. It only dials public RakNet servers.
package webrelay

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/http"
	"net/netip"
	"strings"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authflow"
	"golang.org/x/time/rate"
)

// Config configures a relay. Zero limits take the defaults.
type Config struct {
	// AllowedOrigins are the page origins whose WebSockets are accepted, as host patterns
	// ("play.example.com", "localhost:8080"); empty accepts only same-host pages.
	AllowedOrigins []string
	// AllowPrivateUpstream lets sessions join non-public addresses, for a relay on loopback only.
	AllowPrivateUpstream bool
	// TrustForwardedFor takes the client address from X-Forwarded-For, for a relay behind a
	// reverse proxy on loopback.
	TrustForwardedFor bool
	// Vanilla is the pinned vanilla resource pack the relay streams through; zero disables it.
	Vanilla VanillaSource

	MaxSessions       int           // concurrent web sessions
	MaxSessionsPerIP  int           // concurrent web sessions from one client address
	IdleTTL           time.Duration // how long a session without connections keeps its account
	SignInTimeout     time.Duration // how long a device-code sign-in may wait
	ActionsPerMinute  float64       // session creations, sign-ins and joins per client address
	UploadBytesPerSec int           // sustained browser-to-relay bytes on one play connection

	Logger *slog.Logger
}

// Defaults fill unset limits.
const (
	defaultMaxSessions       = 256
	defaultMaxSessionsPerIP  = 4
	defaultIdleTTL           = 10 * time.Minute
	defaultSignInTimeout     = 15 * time.Minute
	defaultActionsPerMinute  = 12
	defaultUploadBytesPerSec = 256 << 10
)

var (
	errTooManySessions = errors.New("webrelay: too many sessions")
	errRateLimited     = errors.New("webrelay: rate limited")
)

// Relay owns every web session. Serve it with Handler; Close ends every session.
type Relay struct {
	cfg    Config
	logger *slog.Logger
	dialer *net.Dialer
	ctx    context.Context
	cancel context.CancelFunc
	conns  sync.WaitGroup

	mu       sync.Mutex
	sessions map[string]*webSession
	perIP    map[netip.Addr]int
	limiters map[netip.Addr]*clientLimiter
	vanilla  chan struct{} // one slot per concurrent pass-through download

	deviceFlow authflow.DeviceFlow // zero signs in with the Android client
}

type clientLimiter struct {
	limiter  *rate.Limiter
	lastSeen time.Time
}

// New returns a relay; call Run to expire idle sessions.
func New(cfg Config) *Relay {
	if cfg.MaxSessions <= 0 {
		cfg.MaxSessions = defaultMaxSessions
	}
	if cfg.MaxSessionsPerIP <= 0 {
		cfg.MaxSessionsPerIP = defaultMaxSessionsPerIP
	}
	if cfg.IdleTTL <= 0 {
		cfg.IdleTTL = defaultIdleTTL
	}
	if cfg.SignInTimeout <= 0 {
		cfg.SignInTimeout = defaultSignInTimeout
	}
	if cfg.ActionsPerMinute <= 0 {
		cfg.ActionsPerMinute = defaultActionsPerMinute
	}
	if cfg.UploadBytesPerSec <= 0 {
		cfg.UploadBytesPerSec = defaultUploadBytesPerSec
	}
	if cfg.Logger == nil {
		cfg.Logger = slog.Default()
	}
	ctx, cancel := context.WithCancel(context.Background())
	return &Relay{
		cfg:      cfg,
		logger:   cfg.Logger,
		dialer:   newUpstreamDialer(cfg.AllowPrivateUpstream),
		ctx:      ctx,
		cancel:   cancel,
		sessions: make(map[string]*webSession),
		perIP:    make(map[netip.Addr]int),
		limiters: make(map[netip.Addr]*clientLimiter),
		vanilla:  make(chan struct{}, vanillaStreams),
	}
}

// Handler routes the relay's endpoints.
func (relay *Relay) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /v1/control", relay.serveControl)
	mux.HandleFunc("GET /v1/session", relay.servePlay)
	mux.HandleFunc("GET /v1/vanilla-pack", relay.serveVanilla)
	mux.HandleFunc("GET /v1/health", func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Cache-Control", "no-store")
		_, _ = w.Write([]byte("ok\n"))
	})
	return mux
}

// Run expires idle sessions until ctx ends.
func (relay *Relay) Run(ctx context.Context) {
	ticker := time.NewTicker(30 * time.Second)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case now := <-ticker.C:
			relay.expire(now)
		}
	}
}

// expire closes sessions idle past the TTL and forgets quiet rate limiters.
func (relay *Relay) expire(now time.Time) {
	relay.mu.Lock()
	var expired []*webSession
	for id, session := range relay.sessions {
		if session.expired(now, relay.cfg.IdleTTL) {
			expired = append(expired, session)
			relay.removeLocked(id, session)
		}
	}
	for addr, limiter := range relay.limiters {
		if now.Sub(limiter.lastSeen) > 10*time.Minute {
			delete(relay.limiters, addr)
		}
	}
	relay.mu.Unlock()
	for _, session := range expired {
		session.close()
	}
}

func (relay *Relay) removeLocked(id string, session *webSession) {
	delete(relay.sessions, id)
	if relay.perIP[session.client]--; relay.perIP[session.client] <= 0 {
		delete(relay.perIP, session.client)
	}
}

// Close ends every session and waits for their connections.
func (relay *Relay) Close() {
	relay.cancel()
	relay.mu.Lock()
	sessions := make([]*webSession, 0, len(relay.sessions))
	for id, session := range relay.sessions {
		sessions = append(sessions, session)
		relay.removeLocked(id, session)
	}
	relay.mu.Unlock()
	relay.conns.Wait()
	for _, session := range sessions {
		session.close()
	}
}

// allow spends one action from client's budget.
func (relay *Relay) allow(client netip.Addr) bool {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	entry := relay.limiters[client]
	if entry == nil {
		perSecond := rate.Limit(relay.cfg.ActionsPerMinute / 60)
		entry = &clientLimiter{limiter: rate.NewLimiter(perSecond, max(1, int(relay.cfg.ActionsPerMinute/2)))}
		relay.limiters[client] = entry
	}
	entry.lastSeen = time.Now()
	return entry.limiter.Allow()
}

// createSession opens a session for client within the session limits.
func (relay *Relay) createSession(client netip.Addr) (*webSession, error) {
	if !relay.allow(client) {
		return nil, errRateLimited
	}
	session, err := newWebSession(relay, client)
	if err != nil {
		return nil, err
	}
	relay.mu.Lock()
	defer relay.mu.Unlock()
	if relay.ctx.Err() != nil {
		return nil, net.ErrClosed
	}
	if len(relay.sessions) >= relay.cfg.MaxSessions || relay.perIP[client] >= relay.cfg.MaxSessionsPerIP {
		go session.close()
		return nil, errTooManySessions
	}
	relay.sessions[session.id] = session
	relay.perIP[client]++
	return session, nil
}

// lookup returns the session id names when client owns it.
func (relay *Relay) lookup(id string, client netip.Addr) *webSession {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	session := relay.sessions[id]
	if session == nil || session.client != client {
		return nil
	}
	return session
}

// clientAddr is the request's client address, from X-Forwarded-For only when trusted.
func (relay *Relay) clientAddr(r *http.Request) netip.Addr {
	if relay.cfg.TrustForwardedFor {
		forwarded := r.Header.Values("X-Forwarded-For")
		if len(forwarded) > 0 {
			hops := strings.Split(forwarded[len(forwarded)-1], ",")
			if addr, err := netip.ParseAddr(strings.TrimSpace(hops[len(hops)-1])); err == nil {
				return addr.Unmap()
			}
		}
	}
	if addrPort, err := netip.ParseAddrPort(r.RemoteAddr); err == nil {
		return addrPort.Addr().Unmap()
	}
	return netip.Addr{}
}
