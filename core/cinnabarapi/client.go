// Package cinnabarapi keeps the optional Cinnabar API connection for release notices and player lookups.
package cinnabarapi

import (
	"context"
	"crypto/ecdsa"
	"encoding/json"
	"errors"
	"log/slog"
	mathrand "math/rand/v2"
	"net/http"
	"strings"
	"sync"
	"time"

	"github.com/coder/websocket"
	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

const (
	connectPath  = "/v1/connect"
	tokenTimeout = 30 * time.Second
	// authTimeout matches the server's deadline for an unauthenticated socket.
	authTimeout  = 10 * time.Second
	pingInterval = time.Duration(bridgecontract.CinnabarAPIPingIntervalMS) * time.Millisecond
	// idleTimeout ends a session that stopped answering keepalives.
	idleTimeout   = 2*pingInterval + 15*time.Second
	writeTimeout  = 10 * time.Second
	lookupTimeout = 10 * time.Second
	maxFrameBytes = 64 << 10
	minBackoff    = time.Second
	maxBackoff    = time.Duration(bridgecontract.CinnabarAPIReconnectMaxMS) * time.Millisecond
	// stableSession is how long a session must last before failures stop compounding.
	stableSession = time.Minute
)

var (
	// ErrUnavailable means no authenticated connection can answer right now.
	ErrUnavailable = errors.New("cinnabarapi: not connected")
	// ErrRateLimited means a lookup exceeded the negotiated identifier budget.
	ErrRateLimited = errors.New("cinnabarapi: lookup rate limited")
	// ErrInvalidLookup means a lookup had no identifiers, too many identifiers or a malformed one.
	ErrInvalidLookup = errors.New("cinnabarapi: invalid lookup")
	errUnauthorized  = errors.New("cinnabarapi: unauthorized")
)

// TokenSource mints a Minecraft multiplayer token bound to key.
type TokenSource func(ctx context.Context, key *ecdsa.PublicKey) (string, error)

// Config is the client identity the hello carries; Visible may change live.
type Config struct {
	ClientVersion string `json:"client_version"`
	ClientBuild   uint64 `json:"client_build"`
	Channel       string `json:"channel"`
	Visible       bool   `json:"visible"`
}

// Release is a release recorded on the connection's channel.
type Release = bridgecontract.CinnabarRelease

// Options configures a Client; a nil Token keeps it offline.
type Options struct {
	// Base is the API origin; empty selects the shipped one.
	Base      string
	Token     TokenSource
	OnRelease func(Release)
	Logger    *slog.Logger
	// delay overrides the reconnect backoff in tests.
	delay  func(failures int) time.Duration
	random func(int64) int64
}

// Client owns at most one connection; nothing connects until the first Configure.
type Client struct {
	ctx        context.Context
	cancel     context.CancelFunc
	done       chan struct{}
	url        string
	token      TokenSource
	onRelease  func(Release)
	logger     *slog.Logger
	delay      func(int) time.Duration
	wake       chan struct{}
	random     func(int64) int64
	credential credential
	parameters parameters   // guarded by mu
	budget     lookupBudget // guarded by mu

	mu              sync.Mutex
	config          Config
	configured      bool
	reconnect       context.CancelFunc // ends the current session; guarded by mu
	loadoutRevision uint64
	ownershipXUID   string
	ownedCosmetics  []string
	live            *session // authenticated session; guarded by mu
}

// session is one authenticated connection.
type session struct {
	ownershipXUID   string
	ownedCosmetics  []string
	loadoutDirty    chan struct{}
	loadoutSent     bool
	loadoutSlots    []bridgecontract.CinnabarCosmeticSlot
	loadoutRevision uint64
	emoteSent       bool        // guarded by Client.mu
	emoteWatchSent  time.Time   // guarded by Client.mu
	emoteSentXUIDs  []string    // guarded by Client.mu
	emoteSentUUIDs  []string    // guarded by Client.mu
	emoteState      PlayerState // guarded by Client.mu
	emoteQueued     time.Time   // guarded by Client.mu
	emoteDirty      chan struct{}
	loadouts        map[string]bridgecontract.CinnabarRemoteLoadout // guarded by Client.mu
	emotes          map[string]receivedEmote                        // guarded by Client.mu
	conn            *websocket.Conn
	out             chan []byte
	dirty           chan struct{} // visibility changed
	visible         bool          // what the server holds; written only by the writer
	nextID          uint32
	pending         map[uint32]chan lookupReply // guarded by Client.mu
	closed          chan struct{}
	parameters      parameters
}

// LookupResult contains matches in each requested identifier namespace.
type LookupResult struct {
	Cinnabar      []string
	CinnabarUUIDs []string
}

type lookupReply struct {
	result LookupResult
	err    error
}

// New starts the client's lifetime under ctx.
func New(ctx context.Context, options Options) *Client {
	ctx, cancel := context.WithCancel(ctx)
	base := options.Base
	if base == "" {
		base = bridgecontract.CinnabarAPIBaseURL
	}
	logger := options.Logger
	if logger == nil {
		logger = slog.New(slog.DiscardHandler)
	}
	delay := options.delay
	random := options.random
	if random == nil {
		random = mathrand.Int64N
	}
	c := &Client{
		ctx: ctx, cancel: cancel, done: make(chan struct{}),
		url: socketURL(base), token: options.Token, onRelease: options.OnRelease,
		logger: logger, delay: delay, random: random, parameters: defaults(), wake: make(chan struct{}, 1),
	}
	go c.run()
	return c
}

// Configure sets the hello identity; a new version or channel reconnects, visibility is sent live.
func (c *Client) Configure(config Config) {
	c.mu.Lock()
	defer c.mu.Unlock()
	if c.configured && c.config == config {
		return
	}
	previous, had := c.config, c.configured
	c.config, c.configured = config, true
	c.logger.Debug("Cinnabar API visibility configured", "visible", config.Visible)
	switch {
	case !had:
		select {
		case c.wake <- struct{}{}:
		default:
		}
	case previous.ClientVersion != config.ClientVersion || previous.ClientBuild != config.ClientBuild || previous.Channel != config.Channel:
		if c.reconnect != nil {
			c.reconnect()
		}
	case c.live != nil:
		select {
		case c.live.dirty <- struct{}{}:
		default:
		}
	}
}

// Lookup returns matches for both namespaces, charging their combined count.
func (c *Client) Lookup(ctx context.Context, xuids, uuids []string) (LookupResult, error) {
	if !ValidLookup(xuids, uuids) {
		return LookupResult{}, ErrInvalidLookup
	}
	count := len(xuids) + len(uuids)
	c.mu.Lock()
	s := c.live
	if s == nil {
		c.mu.Unlock()
		c.logger.Debug("Cinnabar API lookup unavailable", "requested", len(xuids), "requested_uuids", len(uuids))
		return LookupResult{}, ErrUnavailable
	}
	p := s.parameters
	if !c.budget.take(time.Now(), count, p) {
		c.mu.Unlock()
		c.logger.Debug("Cinnabar API lookup rate limited", "requested", len(xuids), "requested_uuids", len(uuids))
		return LookupResult{}, ErrRateLimited
	}
	c.mu.Unlock()
	found := LookupResult{Cinnabar: []string{}, CinnabarUUIDs: []string{}}
	for len(xuids)+len(uuids) > 0 {
		limit := min(p.LookupBatchMax, p.LookupBucketCapacity)
		nx := min(len(xuids), limit)
		nu := min(len(uuids), limit-nx)
		part, err := c.lookup(ctx, s, xuids[:nx], uuids[:nu])
		if err != nil {
			return LookupResult{}, err
		}
		found.Cinnabar = append(found.Cinnabar, part.Cinnabar...)
		found.CinnabarUUIDs = append(found.CinnabarUUIDs, part.CinnabarUUIDs...)
		xuids, uuids = xuids[nx:], uuids[nu:]
	}
	return found, nil
}

// LookupLimit returns the negotiated per-frame size bounded by the account bucket.
func (c *Client) LookupLimit() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return min(c.parameters.LookupBatchMax, c.parameters.LookupBucketCapacity)
}

// CacheTTLMS returns the current server's answer lifetime for the roster cache.
func (c *Client) CacheTTLMS() int64 { c.mu.Lock(); defer c.mu.Unlock(); return c.parameters.CacheTTLMS }

// lookup sends one negotiated batch on the session chosen by its caller.
func (c *Client) lookup(ctx context.Context, s *session, xuids, uuids []string) (LookupResult, error) {
	c.mu.Lock()
	if c.live != s {
		c.mu.Unlock()
		return LookupResult{}, ErrUnavailable
	}
	id := s.nextID
	s.nextID++
	reply := make(chan lookupReply, 1)
	s.pending[id] = reply
	c.mu.Unlock()
	defer func() {
		c.mu.Lock()
		delete(s.pending, id)
		c.mu.Unlock()
	}()
	payload, err := json.Marshal(lookupMessage{Type: typeLookup, ID: id, XUIDs: xuids, UUIDs: uuids})
	if err != nil {
		return LookupResult{}, err
	}
	timer := time.NewTimer(lookupTimeout)
	defer timer.Stop()
	select {
	case s.out <- payload:
		c.logger.Debug("Cinnabar API lookup queued", "requested", len(xuids), "requested_uuids", len(uuids))
	case <-s.closed:
		return LookupResult{}, ErrUnavailable
	case <-ctx.Done():
		return LookupResult{}, ctx.Err()
	case <-timer.C:
		return LookupResult{}, ErrUnavailable
	}
	select {
	case result := <-reply:
		c.logger.Debug("Cinnabar API lookup answered", "found", len(result.result.Cinnabar), "found_uuids", len(result.result.CinnabarUUIDs), "success", result.err == nil)
		return result.result, result.err
	case <-s.closed:
		return LookupResult{}, ErrUnavailable
	case <-ctx.Done():
		return LookupResult{}, ctx.Err()
	case <-timer.C:
		return LookupResult{}, ErrUnavailable
	}
}

// Close ends the connection and waits for its goroutines; repeating it is safe.
func (c *Client) Close() { c.cancel(); <-c.done }

// run connects once configured and reconnects with capped, jittered backoff.
func (c *Client) run() {
	defer close(c.done)
	if c.token == nil {
		return
	}
	select {
	case <-c.ctx.Done():
		return
	case <-c.wake:
	}
	failures := 0
	rejections := 0
	for {
		sessionCtx, cancel := context.WithCancel(c.ctx)
		c.mu.Lock()
		config := c.config
		c.reconnect = cancel
		c.mu.Unlock()
		started := time.Now()
		welcomed, stage, err := c.session(sessionCtx, config)
		superseded := sessionCtx.Err() != nil
		cancel()
		if c.ctx.Err() != nil {
			return
		}
		if welcomed && time.Since(started) >= stableSession {
			failures = 0
		}
		if superseded {
			continue
		}
		failures++
		c.mu.Lock()
		cap := time.Duration(c.parameters.ReconnectMaxMS) * time.Millisecond
		c.mu.Unlock()
		base := backoff(failures, cap, c.random)
		if c.delay != nil {
			base = c.delay(failures)
		}
		delay := retryDelay(err, stage, base, cap, time.Now(), c.random)
		if errors.Is(err, errUnauthorized) || websocket.CloseStatus(err) == 4401 {
			c.credential = credential{}
		}
		if rejectedFrames(err) {
			rejections++
			if rejections == 1 {
				c.logger.Error("Cinnabar API rejected a client message", "close_code", int(websocket.CloseStatus(err)))
			}
			if rejections >= rejectionLimit {
				return
			}
		} else if failures == 1 {
			c.logger.Warn("Cinnabar API unavailable; retrying", "stage", stage, "close_code", int(websocket.CloseStatus(err)))
		}
		timer := time.NewTimer(delay)
		select {
		case <-c.ctx.Done():
			timer.Stop()
			return
		case <-timer.C:
		}
	}
}

// session runs one connection and names the stage and error that ended it.
func (c *Client) session(ctx context.Context, config Config) (bool, string, error) {
	conn, stage, err := c.connect(ctx, config)
	if err != nil {
		return false, stage, err
	}
	defer conn.CloseNow()
	s := &session{
		conn: conn, out: make(chan []byte, 8), dirty: make(chan struct{}, 1),
		loadoutDirty: make(chan struct{}, 1), loadouts: make(map[string]bridgecontract.CinnabarRemoteLoadout),
		emoteDirty: make(chan struct{}, 1), emotes: make(map[string]receivedEmote),
		parameters: c.parameters, visible: config.Visible, pending: make(map[uint32]chan lookupReply), closed: make(chan struct{}),
	}
	c.mu.Lock()
	s.ownershipXUID = c.ownershipXUID
	s.ownedCosmetics = append([]string(nil), c.ownedCosmetics...)
	c.live = s
	if c.config.Visible != config.Visible {
		s.dirty <- struct{}{}
	}
	c.mu.Unlock()
	c.logger.Info("Cinnabar API connected")
	c.logger.Debug("Cinnabar API visibility announced", "visible", config.Visible)
	ctx, cancel := context.WithCancel(ctx)
	written := make(chan struct{})
	go func() {
		defer close(written)
		c.write(ctx, cancel, s)
	}()
	err = c.read(ctx, s)
	cancel()
	<-written
	c.mu.Lock()
	c.live = nil
	close(s.closed)
	for _, reply := range s.pending {
		reply <- lookupReply{err: ErrUnavailable}
	}
	c.mu.Unlock()
	return true, "session", err
}

// connect dials before minting and reuses credentials until they approach expiry.
func (c *Client) connect(ctx context.Context, config Config) (*websocket.Conn, string, error) {
	authCtx, cancel := context.WithTimeout(ctx, authTimeout)
	defer cancel()
	conn, response, err := websocket.Dial(authCtx, c.url, &websocket.DialOptions{
		HTTPHeader: http.Header{"User-Agent": {userAgent(config.ClientVersion)}},
	})
	if err != nil {
		if response != nil && response.StatusCode == http.StatusTooManyRequests {
			err = retryError{error: err, after: retryAfter(response.Header.Get("Retry-After"), time.Now())}
		}
		return nil, "dial", err
	}
	conn.SetReadLimit(maxFrameBytes)
	if err := c.credential.refresh(authCtx, c.token, time.Now()); err != nil {
		_ = conn.CloseNow()
		return nil, "token", err
	}
	if err := c.handshake(authCtx, conn, c.credential.key, c.credential.token, config); err != nil {
		_ = conn.CloseNow()
		return nil, "handshake", err
	}
	return conn, "", nil
}

// handshake authenticates conn, ignoring server messages it does not know.
func (c *Client) handshake(ctx context.Context, conn *websocket.Conn, key *ecdsa.PrivateKey, token string, config Config) error {
	if err := writeJSON(ctx, conn, helloMessage{
		Type: typeHello, Protocol: protocolVersion, ClientVersion: config.ClientVersion, ClientBuild: config.ClientBuild,
		Channel: config.Channel, Visible: config.Visible, Token: token,
	}); err != nil {
		return err
	}
	for {
		kind, data, err := conn.Read(ctx)
		if err != nil {
			return err
		}
		var msg inbound
		if kind != websocket.MessageText || json.Unmarshal(data, &msg) != nil {
			continue
		}
		switch msg.Type {
		case typeChallenge:
			signature, err := sign(key, msg.Nonce)
			if err != nil {
				return err
			}
			if err := writeJSON(ctx, conn, proofMessage{Type: typeProof, Signature: signature}); err != nil {
				return err
			}
		case typeWelcome:
			c.mu.Lock()
			c.loadoutRevision = max(c.loadoutRevision, min(msg.LoadoutRevision, uint64(maxBuildNumber)))
			c.ownershipXUID = ""
			c.ownedCosmetics = nil
			if ValidXUID(msg.XUID) {
				c.ownershipXUID = msg.XUID
				c.ownedCosmetics = admittedOwnership(msg.OwnedCosmetics)
			}
			c.mu.Unlock()
			c.mu.Lock()
			c.parameters = msg.parameters.normalized()
			c.mu.Unlock()
			return nil
		case typeError:
			return errUnauthorized
		case typeRelease:
			c.release(msg)
		}
	}
}

// read dispatches server messages until the connection fails or goes idle.
func (c *Client) read(ctx context.Context, s *session) error {
	for {
		readCtx, cancel := context.WithTimeout(ctx, max(idleTimeout, 3*time.Duration(s.parameters.PingIntervalMS)*time.Millisecond))
		kind, data, err := s.conn.Read(readCtx)
		cancel()
		if err != nil {
			return err
		}
		var msg inbound
		// The keepalive answer "pong" is not JSON.
		if kind != websocket.MessageText || json.Unmarshal(data, &msg) != nil {
			continue
		}
		switch msg.Type {
		case typeRelease:
			c.release(msg)
		case typeCosmeticOwnership:
			c.mu.Lock()
			s.ownedCosmetics = admittedOwnership(msg.OwnedCosmetics)
			c.mu.Unlock()
		case typeLoadout:
			c.receiveLoadout(s, msg)
		case typeEmote:
			c.receiveEmote(s, msg)
		case typeLookupResult:
			c.logger.Debug("Cinnabar API lookup result received", "found", len(msg.Cinnabar), "found_uuids", len(msg.CinnabarUUIDs))
			c.answer(s, msg.ID, lookupReply{result: LookupResult{Cinnabar: msg.Cinnabar, CinnabarUUIDs: msg.CinnabarUUIDs}})
		case typeError:
			err := ErrUnavailable
			if msg.Code == codeRateLimited {
				err = ErrRateLimited
			}
			c.answer(s, msg.ID, lookupReply{err: err})
		}
	}
}

// write serializes lookups, coalesced visibility changes and keepalives on one goroutine.
func (c *Client) write(ctx context.Context, stop context.CancelFunc, s *session) {
	ticker := time.NewTicker(time.Duration(s.parameters.PingIntervalMS) * time.Millisecond)
	defer ticker.Stop()
	for {
		var payload []byte
		visibilityChanged := false
		select {
		case <-ctx.Done():
			return
		case payload = <-s.out:
		case <-s.loadoutDirty:
			payload = c.loadoutPayload(s)
		case <-s.emoteDirty:
			payload = c.emotePayload(s)
		case <-s.dirty:
			c.mu.Lock()
			visible := c.config.Visible
			c.mu.Unlock()
			if visible == s.visible {
				continue
			}
			s.visible = visible
			c.mu.Lock()
			s.loadoutSent = false
			c.mu.Unlock()
			visibilityChanged = true
			payload, _ = json.Marshal(visibilityMessage{Type: typeVisibility, Visible: visible})
		case <-ticker.C:
			payload = []byte(keepalivePing)
		}
		writeCtx, cancel := context.WithTimeout(ctx, writeTimeout)
		err := s.conn.Write(writeCtx, websocket.MessageText, payload)
		cancel()
		if err != nil {
			stop()
			return
		}
		if visibilityChanged {
			c.logger.Debug("Cinnabar API visibility sent", "visible", s.visible)
		}
	}
}

// release reports a well-formed release for the configured channel.
func (c *Client) release(msg inbound) {
	c.mu.Lock()
	channel := c.config.Channel
	c.mu.Unlock()
	if msg.Channel == channel && msg.Version != "" && c.onRelease != nil {
		c.onRelease(Release{Channel: msg.Channel, Version: msg.Version, Build: msg.Build, PublishedAt: msg.PublishedAt})
	}
}

// answer completes the pending lookup id; unknown ids were abandoned by their caller.
func (c *Client) answer(s *session, id *uint32, reply lookupReply) {
	if id == nil {
		return
	}
	c.mu.Lock()
	pending := s.pending[*id]
	delete(s.pending, *id)
	c.mu.Unlock()
	if pending != nil {
		pending <- reply
	}
}

// rejectedFrames reports a close for a malformed, schema-invalid or oversized client frame.
// Unauthorized (4401) and issuer outages (1011) retry with the normal backoff instead.
func rejectedFrames(err error) bool {
	switch websocket.CloseStatus(err) {
	case websocket.StatusInvalidFramePayloadData, websocket.StatusPolicyViolation, websocket.StatusMessageTooBig:
		return true
	}
	return false
}

func writeJSON(ctx context.Context, conn *websocket.Conn, value any) error {
	payload, err := json.Marshal(value)
	if err != nil {
		return err
	}
	return conn.Write(ctx, websocket.MessageText, payload)
}

// userAgent names the client to the API's edge, which refuses some library defaults.
func userAgent(version string) string { return "cinnabar/" + version }

// socketURL is the WebSocket form of the API origin's connect route.
func socketURL(base string) string {
	base = strings.TrimSuffix(base, "/")
	switch {
	case strings.HasPrefix(base, "https://"):
		base = "wss://" + strings.TrimPrefix(base, "https://")
	case strings.HasPrefix(base, "http://"):
		base = "ws://" + strings.TrimPrefix(base, "http://")
	}
	return base + connectPath
}

// backoff applies equal jitter under this server's advertised cap.
func backoff(failures int, cap time.Duration, random func(int64) int64) time.Duration {
	ceiling := cap
	if failures >= 1 && failures <= 32 {
		if step := minBackoff << (failures - 1); step > 0 && step < cap {
			ceiling = step
		}
	}
	half := ceiling / 2
	return half + time.Duration(random(int64(ceiling-half)+1))
}
