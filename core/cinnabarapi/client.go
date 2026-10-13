// Package cinnabarapi keeps the optional Cinnabar API connection for release notices and player lookups.
package cinnabarapi

import (
	"context"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"encoding/json"
	"errors"
	"log/slog"
	mathrand "math/rand/v2"
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
	pingInterval = 30 * time.Second
	// idleTimeout ends a session that stopped answering keepalives.
	idleTimeout   = 2*pingInterval + 15*time.Second
	writeTimeout  = 10 * time.Second
	lookupTimeout = 10 * time.Second
	maxFrameBytes = 64 << 10
	minBackoff    = time.Second
	maxBackoff    = 5 * time.Minute
	// stableSession is how long a session must last before failures stop compounding.
	stableSession = time.Minute
)

var (
	// ErrUnavailable means no authenticated connection can answer right now.
	ErrUnavailable = errors.New("cinnabarapi: not connected")
	// ErrRateLimited means the server refused a lookup over its per-minute budget.
	ErrRateLimited = errors.New("cinnabarapi: lookup rate limited")
	// ErrInvalidLookup means a lookup had too many XUIDs or a malformed one.
	ErrInvalidLookup = errors.New("cinnabarapi: invalid lookup")
	errUnauthorized  = errors.New("cinnabarapi: unauthorized")
)

// TokenSource mints a Minecraft multiplayer token bound to key.
type TokenSource func(ctx context.Context, key *ecdsa.PublicKey) (string, error)

// Config is the client identity the hello carries; Visible may change live.
type Config struct {
	ClientVersion string `json:"client_version"`
	Channel       string `json:"channel"`
	Visible       bool   `json:"visible"`
}

// Release is a release recorded on the connection's channel.
type Release struct {
	Channel string `json:"channel"`
	Version string `json:"version"`
}

// Options configures a Client; a nil Token keeps it offline.
type Options struct {
	// Base is the API origin; empty selects the shipped one.
	Base      string
	Token     TokenSource
	OnRelease func(Release)
	Logger    *slog.Logger
	// delay overrides the reconnect backoff in tests.
	delay func(failures int) time.Duration
}

// Client owns at most one connection; nothing connects until the first Configure.
type Client struct {
	ctx       context.Context
	cancel    context.CancelFunc
	done      chan struct{}
	url       string
	token     TokenSource
	onRelease func(Release)
	logger    *slog.Logger
	delay     func(int) time.Duration
	wake      chan struct{}

	mu         sync.Mutex
	config     Config
	configured bool
	reconnect  context.CancelFunc // ends the current session; guarded by mu
	live       *session           // authenticated session; guarded by mu
}

// session is one authenticated connection.
type session struct {
	conn    *websocket.Conn
	out     chan []byte
	dirty   chan struct{} // visibility changed
	visible bool          // what the server holds; written only by the writer
	nextID  uint32
	pending map[uint32]chan lookupReply // guarded by Client.mu
	closed  chan struct{}
}

type lookupReply struct {
	cinnabar []string
	err      error
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
	if delay == nil {
		delay = func(failures int) time.Duration { return backoff(failures, mathrand.Int64N) }
	}
	c := &Client{
		ctx: ctx, cancel: cancel, done: make(chan struct{}),
		url: socketURL(base), token: options.Token, onRelease: options.OnRelease,
		logger: logger, delay: delay, wake: make(chan struct{}, 1),
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
	switch {
	case !had:
		select {
		case c.wake <- struct{}{}:
		default:
		}
	case previous.ClientVersion != config.ClientVersion || previous.Channel != config.Channel:
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

// Lookup returns the subset of xuids the API reports as visible Cinnabar players.
func (c *Client) Lookup(ctx context.Context, xuids []string) ([]string, error) {
	if len(xuids) > bridgecontract.CinnabarAPIMaxLookupXUIDs {
		return nil, ErrInvalidLookup
	}
	for _, xuid := range xuids {
		if !ValidXUID(xuid) {
			return nil, ErrInvalidLookup
		}
	}
	if len(xuids) == 0 {
		return []string{}, nil
	}
	c.mu.Lock()
	s := c.live
	if s == nil {
		c.mu.Unlock()
		return nil, ErrUnavailable
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
	payload, err := json.Marshal(lookupMessage{Type: typeLookup, ID: id, XUIDs: xuids})
	if err != nil {
		return nil, err
	}
	timer := time.NewTimer(lookupTimeout)
	defer timer.Stop()
	select {
	case s.out <- payload:
	case <-s.closed:
		return nil, ErrUnavailable
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-timer.C:
		return nil, ErrUnavailable
	}
	select {
	case result := <-reply:
		return result.cinnabar, result.err
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-timer.C:
		return nil, ErrUnavailable
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
	rejectionLogged := false
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
		delay := c.delay(failures)
		if rejectedFrames(err) {
			// The server refused a frame this client built; retrying soon would only repeat it.
			delay = maxBackoff
			if !rejectionLogged {
				rejectionLogged = true
				c.logger.Error("Cinnabar API rejected a client message", "close_code", int(websocket.CloseStatus(err)))
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
		visible: config.Visible, pending: make(map[uint32]chan lookupReply), closed: make(chan struct{}),
	}
	c.mu.Lock()
	c.live = s
	if c.config.Visible != config.Visible {
		s.dirty <- struct{}{}
	}
	c.mu.Unlock()
	c.logger.Info("Cinnabar API connected")
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

// connect performs hello → challenge → proof → welcome with a fresh ephemeral key and token.
func (c *Client) connect(ctx context.Context, config Config) (*websocket.Conn, string, error) {
	key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if err != nil {
		return nil, "key", err
	}
	tokenCtx, cancel := context.WithTimeout(ctx, tokenTimeout)
	token, err := c.token(tokenCtx, &key.PublicKey)
	cancel()
	if err != nil {
		return nil, "token", err
	}
	authCtx, cancel := context.WithTimeout(ctx, authTimeout)
	defer cancel()
	conn, _, err := websocket.Dial(authCtx, c.url, nil)
	if err != nil {
		return nil, "dial", err
	}
	conn.SetReadLimit(maxFrameBytes)
	if err := c.handshake(authCtx, conn, key, token, config); err != nil {
		_ = conn.CloseNow()
		return nil, "handshake", err
	}
	return conn, "", nil
}

// handshake authenticates conn, ignoring server messages it does not know.
func (c *Client) handshake(ctx context.Context, conn *websocket.Conn, key *ecdsa.PrivateKey, token string, config Config) error {
	if err := writeJSON(ctx, conn, helloMessage{
		Type: typeHello, Protocol: protocolVersion, ClientVersion: config.ClientVersion,
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
		readCtx, cancel := context.WithTimeout(ctx, idleTimeout)
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
		case typeLookupResult:
			c.answer(s, msg.ID, lookupReply{cinnabar: msg.Cinnabar})
		case typeError:
			if msg.Code == codeRateLimited {
				c.answer(s, msg.ID, lookupReply{err: ErrRateLimited})
			}
		}
	}
}

// write serializes lookups, coalesced visibility changes and keepalives on one goroutine.
func (c *Client) write(ctx context.Context, stop context.CancelFunc, s *session) {
	ticker := time.NewTicker(pingInterval)
	defer ticker.Stop()
	for {
		var payload []byte
		select {
		case <-ctx.Done():
			return
		case payload = <-s.out:
		case <-s.dirty:
			c.mu.Lock()
			visible := c.config.Visible
			c.mu.Unlock()
			if visible == s.visible {
				continue
			}
			s.visible = visible
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
	}
}

// release reports a well-formed release for the configured channel.
func (c *Client) release(msg inbound) {
	c.mu.Lock()
	channel := c.config.Channel
	c.mu.Unlock()
	if msg.Channel == channel && msg.Version != "" && c.onRelease != nil {
		c.onRelease(Release{Channel: msg.Channel, Version: msg.Version})
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

// backoff returns an equal-jitter delay: half the capped exponential step plus a random share of the rest.
func backoff(failures int, random func(int64) int64) time.Duration {
	ceiling := maxBackoff
	if failures >= 1 && failures <= 32 {
		if step := minBackoff << (failures - 1); step > 0 && step < maxBackoff {
			ceiling = step
		}
	}
	half := ceiling / 2
	return half + time.Duration(random(int64(ceiling-half)+1))
}
