package authcache

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"path/filepath"

	"github.com/google/uuid"
	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"golang.org/x/oauth2"
)

const maxCacheSize = 64 * 1024

// errAccountChanged prevents an existing runtime from adopting a different sign-in.
var errAccountChanged = errors.New("authentication: sign-in changed; reopen the account")

// cachedToken adds a stable sign-in generation without changing the OAuth JSON fields.
// Refreshes keep this value; an interactive sign-in starts a new generation.
type cachedToken struct {
	oauth2.Token
	Generation string `json:"cinnabar_sign_in_generation,omitempty"`
}

// Config configures the Microsoft token cache and its authentication operations.
type Config struct {
	Path    string
	Writer  io.Writer
	Request func(context.Context, io.Writer) (*oauth2.Token, error)
	Refresh func(*oauth2.Token, io.Writer) oauth2.TokenSource
}

// Source loads or acquires a Microsoft token. Every refresh reloads the latest
// token under a process-wide file lock and persists any rotation before returning.
// ctx governs the returned source's lifetime, including waits for another process.
func Source(ctx context.Context, config Config) (oauth2.TokenSource, error) {
	if config.Path == "" {
		return nil, errors.New("auth cache path is empty")
	}
	path, err := filepath.Abs(config.Path)
	if err != nil {
		return nil, errors.New("resolve auth cache path")
	}
	if config.Writer == nil {
		config.Writer = io.Discard
	}
	if config.Request == nil {
		config.Request = auth.AndroidConfig.RequestLiveTokenContext
	}
	if config.Refresh == nil {
		config.Refresh = func(token *oauth2.Token, _ io.Writer) oauth2.TokenSource {
			return auth.AndroidConfig.TokenSource(ctx, token)
		}
	}
	lease, err := lockfile.AcquireContext(ctx, path+cacheLockSuffix)
	if err != nil {
		return nil, fmt.Errorf("lock Microsoft auth cache: %w", err)
	}
	defer lease.Close()

	s := &persistingSource{gate: make(chan struct{}, 1), ctx: ctx, path: path, writer: config.Writer, refresh: config.Refresh}
	cached, err := load(path)
	if err != nil && !errors.Is(err, fs.ErrNotExist) {
		return nil, fmt.Errorf("load Microsoft auth cache: %w", err)
	}
	if err == nil {
		s.generation, s.lastGeneration = cached.Generation, cached.Generation
		if s.generation == "" {
			s.generation = uuid.NewString()
		}
		s.source = s.refresh(&cached.Token, s.writer)
		s.last = cloneToken(&cached.Token)
		if s.source != nil {
			current, refreshErr := s.source.Token()
			if refreshErr == nil && validToken(current) {
				if err := s.persist(current); err != nil {
					return nil, err
				}
				return s, nil
			}
		}
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	current, err := config.Request(ctx, s.writer)
	if err != nil {
		return nil, fmt.Errorf("request Microsoft token: %w", err)
	}
	s.generation = uuid.NewString()
	if err := s.persist(current); err != nil {
		return nil, err
	}
	s.source = s.refresh(current, s.writer)
	if s.source == nil {
		return nil, errors.New("create Microsoft refresh source: nil token source")
	}
	return s, nil
}

// persistingSource serializes local callers and holds a stable path.lock lease
// across each refresh. Atomic token replacement never replaces the lock file.
type persistingSource struct {
	gate           chan struct{}
	ctx            context.Context
	path           string
	writer         io.Writer
	refresh        func(*oauth2.Token, io.Writer) oauth2.TokenSource
	source         oauth2.TokenSource
	last           *oauth2.Token
	generation     string
	lastGeneration string
	changed        bool
}

// Token reloads another process's rotation before attempting its own refresh.
func (s *persistingSource) Token() (*oauth2.Token, error) {
	return s.token(s.ctx)
}

// token lets each caller interrupt waits for local serialization or the file lease.
func (s *persistingSource) token(ctx context.Context) (*oauth2.Token, error) {
	if err := s.ctx.Err(); err != nil {
		return nil, err
	}
	wait, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(s.ctx, cancel)
	defer stop()
	defer cancel()
	select {
	case s.gate <- struct{}{}:
		defer func() { <-s.gate }()
	case <-wait.Done():
		return nil, wait.Err()
	}
	if s.changed {
		return nil, errAccountChanged
	}
	lease, err := lockfile.AcquireContext(wait, s.path+cacheLockSuffix)
	if err != nil {
		return nil, fmt.Errorf("lock Microsoft auth cache: %w", err)
	}
	defer lease.Close()
	cached, err := load(s.path)
	if errors.Is(err, fs.ErrNotExist) || (err == nil && cached.Generation != s.generation) {
		s.changed = true
		return nil, errAccountChanged
	}
	if err != nil {
		return nil, fmt.Errorf("reload Microsoft auth cache: %w", err)
	}
	if !sameToken(s.last, &cached.Token) {
		s.source = s.refresh(&cached.Token, s.writer)
		s.last = cloneToken(&cached.Token)
	}
	if s.source == nil {
		return nil, errors.New("create Microsoft refresh source: nil token source")
	}
	token, err := s.source.Token()
	if err != nil {
		return nil, err
	}
	if err := s.persist(token); err != nil {
		return nil, err
	}
	return token, nil
}

// persist publishes a changed token while the caller holds the cache lease.
func (s *persistingSource) persist(token *oauth2.Token) error {
	if !validToken(token) {
		return errors.New("Microsoft token has no refresh token")
	}
	if sameToken(s.last, token) && s.lastGeneration == s.generation {
		return nil
	}
	if err := save(s.path, token, s.generation); err != nil {
		return fmt.Errorf("persist Microsoft token: %w", err)
	}
	s.last = cloneToken(token)
	s.lastGeneration = s.generation
	return nil
}

// sameToken compares all persisted OAuth token fields.
func sameToken(left, right *oauth2.Token) bool {
	return left != nil && right != nil && left.AccessToken == right.AccessToken && left.TokenType == right.TokenType &&
		left.RefreshToken == right.RefreshToken && left.Expiry.Equal(right.Expiry)
}

// cloneToken keeps the persisted snapshot independent of a mutable source token.
func cloneToken(token *oauth2.Token) *oauth2.Token {
	if token == nil {
		return nil
	}
	cloned := *token
	return &cloned
}

// load decodes exactly one bounded OAuth token from a private regular file.
func load(path string) (*cachedToken, error) {
	contents, err := loadPrivate(path, maxCacheSize)
	if err != nil {
		return nil, err
	}

	decoder := json.NewDecoder(bytes.NewReader(contents))
	var tok cachedToken
	if err := decoder.Decode(&tok); err != nil {
		return nil, fmt.Errorf("decode auth cache: %w", err)
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		if err == nil {
			return nil, errors.New("decode auth cache: trailing JSON value")
		}
		return nil, fmt.Errorf("decode auth cache trailing data: %w", err)
	}
	if !validToken(&tok.Token) {
		return nil, errors.New("decode auth cache: token has no refresh token")
	}
	return &tok, nil
}

// save atomically publishes the token after checking its serialized size.
func save(path string, token *oauth2.Token, generation string) error {
	serialized, err := serializeToken(token, generation)
	if err != nil {
		return err
	}
	return savePrivate(path, serialized)
}

// serializeToken bounds the credential before allocating its JSON representation.
func serializeToken(tok *oauth2.Token, generation string) ([]byte, error) {
	if !validToken(tok) {
		return nil, errors.New("refusing to persist token without refresh token")
	}
	remaining := maxCacheSize
	for _, field := range []string{tok.AccessToken, tok.TokenType, tok.RefreshToken} {
		if len(field) > remaining {
			return nil, fmt.Errorf("auth cache exceeds %d bytes", maxCacheSize)
		}
		remaining -= len(field)
	}
	serialized, err := json.Marshal(cachedToken{Token: *tok, Generation: generation})
	if err != nil {
		return nil, err
	}
	if len(serialized) >= maxCacheSize {
		return nil, fmt.Errorf("auth cache exceeds %d bytes", maxCacheSize)
	}
	return append(serialized, '\n'), nil
}

// validToken requires the refresh token needed to keep the account signed in.
func validToken(token *oauth2.Token) bool {
	return token != nil && token.RefreshToken != ""
}
