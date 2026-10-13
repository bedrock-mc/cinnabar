// Package webrelay carries browser control and reliable play streams to the Go core.
package webrelay

import (
	"context"
	"errors"
	"log/slog"
	"net"
	"net/http"
	"net/netip"
	"net/url"
	"sync"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authflow"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"golang.org/x/time/rate"
)

const (
	maxSessions       = 256
	maxPerIP          = 4
	idleTTL           = 10 * time.Minute
	resumeTTL         = 2 * time.Minute
	helloTimeout      = 10 * time.Second
	keepAliveInterval = time.Duration(bridgecontract.RelayKeepAliveSeconds) * time.Second
	playMessageLimit  = bridgecontract.RelayPlayMessageBytes
)

var (
	errAdmission = errors.New("webrelay: admission refused")
	errResume    = errors.New("webrelay: resume refused")
)

// Config supplies allowed page origins and the relay logger. Empty origins require the relay host.
type Config struct {
	AllowedOrigins []string
	Logger         *slog.Logger
}

// Relay owns bounded in-memory accounts and transport connections. Close erases the accounts.
type Relay struct {
	cfg         Config
	logger      *slog.Logger
	dialer      *net.Dialer
	ctx         context.Context
	cancel      context.CancelFunc
	conns       sync.WaitGroup
	mu          sync.Mutex
	sessions    map[*webSession]struct{}
	tokens      map[string]resumeBearer
	perIP       map[netip.Addr]int
	connections map[netip.Addr]int
	active      int
	limiters    map[netip.Addr]*clientLimiter
	deviceFlow  authflow.DeviceFlow
}

type clientLimiter struct {
	limiter  *rate.Limiter
	lastSeen time.Time
}

// New creates a relay with public-only upstream egress and fixed admission bounds.
func New(cfg Config) *Relay {
	if cfg.Logger == nil {
		cfg.Logger = slog.Default()
	}
	ctx, cancel := context.WithCancel(context.Background())
	return &Relay{cfg: cfg, logger: cfg.Logger, dialer: newUpstreamDialer(false), ctx: ctx, cancel: cancel,
		sessions: make(map[*webSession]struct{}), tokens: make(map[string]resumeBearer), perIP: make(map[netip.Addr]int),
		connections: make(map[netip.Addr]int), limiters: make(map[netip.Addr]*clientLimiter)}
}

// admit bounds every transport, including connections that have not sent Hello yet.
func (relay *Relay) admit(client netip.Addr, now time.Time) (func(), error) {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	if !client.IsValid() || relay.ctx.Err() != nil || relay.active >= maxSessions || relay.connections[client] >= maxPerIP || !relay.allowLocked(client, now) {
		return nil, errAdmission
	}
	relay.active++
	relay.connections[client]++
	relay.conns.Add(1)
	return sync.OnceFunc(func() {
		relay.mu.Lock()
		relay.active--
		relay.connections[client]--
		if relay.connections[client] == 0 {
			delete(relay.connections, client)
		}
		relay.mu.Unlock()
		relay.conns.Done()
	}), nil
}

// allowLocked spends an action while bounding the number of remembered client addresses.
func (relay *Relay) allowLocked(client netip.Addr, now time.Time) bool {
	entry := relay.limiters[client]
	if entry == nil {
		if len(relay.limiters) >= 4*maxSessions {
			return false
		}
		entry = &clientLimiter{limiter: rate.NewLimiter(rate.Limit(12.0/60), 6)}
		relay.limiters[client] = entry
	}
	entry.lastSeen = now
	return entry.limiter.AllowN(now, 1)
}

// allow spends one sign-in or play action from the client's admission budget.
func (relay *Relay) allow(client netip.Addr) bool {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	return relay.allowLocked(client, time.Now())
}

// openControl creates or resumes an account, rotates its bearer and claims its sole controller.
func (relay *Relay) openControl(client netip.Addr, token string, now time.Time) (*webSession, func(time.Time), error) {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	if relay.ctx.Err() != nil {
		return nil, nil, net.ErrClosed
	}
	session := relay.resumeLocked(token, now)
	if token != "" && (session == nil || session.controlOpen) {
		return nil, nil, errResume
	}
	if session == nil {
		if len(relay.sessions) >= maxSessions || relay.perIP[client] >= maxPerIP {
			return nil, nil, errAdmission
		}
		var err error
		session, err = newWebSession(relay, client, now)
		if err != nil {
			return nil, nil, err
		}
		relay.sessions[session] = struct{}{}
		relay.perIP[client]++
	} else if session.client != client {
		if relay.perIP[client] >= maxPerIP {
			return nil, nil, errAdmission
		}
		relay.perIP[session.client]--
		if relay.perIP[session.client] == 0 {
			delete(relay.perIP, session.client)
		}
		session.client = client
		relay.perIP[client]++
	}
	detach, err := session.attach()
	if err != nil {
		return nil, nil, err
	}
	session.controlOpen = true
	session.controlCtx, session.controlCancel = context.WithCancel(relay.ctx)
	session.generation++
	relay.rotateLocked(session, token, now)
	// A fresh account has no earlier capability to protect. Its first ping may replace
	// the welcome while retaining it; a resume must first confirm its replacement welcome.
	if token == "" {
		session.tokenPending = false
	}
	var once sync.Once
	return session, func(now time.Time) {
		once.Do(func() {
			relay.mu.Lock()
			session.controlOpen = false
			session.controlCancel()
			detach(now)
			relay.mu.Unlock()
		})
	}, nil
}

// welcome refreshes acknowledged capabilities and repeats outstanding ones at their original expiry.
func (relay *Relay) welcome(session *webSession, refresh bool, now time.Time) controlEvent {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	if refresh && !session.tokenPending {
		relay.rotateLocked(session, session.token, now)
	}
	expires := relay.tokens[session.token].expires.Sub(now)
	return controlEvent{Type: bridgecontract.RelayEventWelcome, Version: bridgecontract.RelayVersion,
		Resume: session.token, Generation: session.generation, ExpiresSeconds: max(0, int((expires+time.Second-1)/time.Second))}
}

// playSession checks a fallback bearer or the authenticated QUIC account and its generation.
func (relay *Relay) playSession(bound *webSession, hello hello, now time.Time) (*webSession, context.Context, error) {
	relay.mu.Lock()
	defer relay.mu.Unlock()
	session := bound
	if session == nil {
		session = relay.resumeLocked(hello.Resume, now)
		if session == nil {
			return nil, nil, errResume
		}
	}
	if !session.controlOpen || hello.Generation != session.generation {
		return nil, nil, errResume
	}
	return session, session.controlCtx, nil
}

// expire forgets detached accounts and old address limiters using caller time.
func (relay *Relay) expire(now time.Time) {
	relay.mu.Lock()
	var expired []*webSession
	for session := range relay.sessions {
		if session.expired(now, idleTTL) {
			delete(relay.sessions, session)
			delete(relay.tokens, session.token)
			delete(relay.tokens, session.previousToken)
			relay.perIP[session.client]--
			if relay.perIP[session.client] == 0 {
				delete(relay.perIP, session.client)
			}
			expired = append(expired, session)
		}
	}
	for client, entry := range relay.limiters {
		if now.Sub(entry.lastSeen) > idleTTL {
			delete(relay.limiters, client)
		}
	}
	relay.mu.Unlock()
	for _, session := range expired {
		session.close()
	}
}

// Run expires idle accounts until the caller or relay closes.
func (relay *Relay) Run(ctx context.Context) {
	ticker := time.NewTicker(keepAliveInterval)
	defer ticker.Stop()
	for {
		select {
		case now := <-ticker.C:
			relay.expire(now)
		case <-ctx.Done():
			return
		case <-relay.ctx.Done():
			return
		}
	}
}

// Close cancels transports, waits for their handlers, then erases all account state.
func (relay *Relay) Close() {
	relay.mu.Lock()
	relay.cancel()
	relay.mu.Unlock()
	relay.conns.Wait()
	relay.mu.Lock()
	sessions := relay.sessions
	relay.sessions = make(map[*webSession]struct{})
	relay.tokens = make(map[string]resumeBearer)
	relay.mu.Unlock()
	for session := range sessions {
		session.close()
	}
}

// checkOrigin applies the same exact origin policy to both transport adapters.
func (relay *Relay) checkOrigin(r *http.Request) bool {
	origin := r.Header.Get("Origin")
	if origin == "" {
		return true
	}
	parsed, err := url.Parse(origin)
	if err != nil || parsed.User != nil || parsed.Host == "" || (parsed.Scheme != "https" && parsed.Scheme != "http") || parsed.Path != "" || parsed.RawQuery != "" || parsed.Fragment != "" {
		return false
	}
	if len(relay.cfg.AllowedOrigins) == 0 {
		return parsed.Host == r.Host
	}
	for _, allowed := range relay.cfg.AllowedOrigins {
		if origin == allowed {
			return true
		}
	}
	return false
}

// clientAddr uses the peer address only; forwarded headers never alter admission identity.
func clientAddr(r *http.Request) netip.Addr {
	addr, err := netip.ParseAddrPort(r.RemoteAddr)
	if err != nil {
		return netip.Addr{}
	}
	return addr.Addr().Unmap()
}
