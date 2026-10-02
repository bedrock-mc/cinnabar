package authcache

import (
	"bytes"
	"context"
	"crypto/ecdsa"
	"crypto/sha256"
	"crypto/x509"
	"encoding/base64"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"net/url"
	"path/filepath"
	"sync/atomic"
	"time"

	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-playfab/v2/title"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/xal/nsal"
	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
	"github.com/hashimthearab/rust-mcbe/core/clientplatform"
	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/service"
	"golang.org/x/oauth2"
)

const (
	derivedCacheVersion = 1
	derivedCacheSuffix  = ".join-auth-v1"
)

// DerivedCachePath returns the private cache path used for authentication
// state derived from the Microsoft token at oauthPath.
func DerivedCachePath(oauthPath string) string {
	return oauthPath + derivedCacheSuffix
}

// NewAccount returns the signed-in account's runtime: the Xbox, PlayFab and Minecraft service
// credentials every consumer shares, with proof-key-bound state persisted at path (empty disables
// persistence). The OAuth source stays authoritative: new token material makes the cache miss.
// Cache failures are optional misses reported without paths or secrets. Close ends the runtime.
func NewAccount(ctx context.Context, path string, oauth oauth2.TokenSource, diagnostics io.Writer) *Account {
	return newAccount(ctx, path, oauth, diagnostics, defaultDerivedDeps())
}

type derivedDeps struct {
	discover func(context.Context) (*service.AuthorizationEnvironment, error)
	login    func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*playfab.Client, error)
	services func(*service.AuthorizationEnvironment, service.SessionTicketSource, *service.Token) service.TokenSource
	mint     func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error)
}

func defaultDerivedDeps() derivedDeps {
	return derivedDeps{
		discover: func(ctx context.Context) (*service.AuthorizationEnvironment, error) {
			discovery, err := service.Default(ctx)
			if err != nil {
				return nil, err
			}
			env := new(service.AuthorizationEnvironment)
			if err := discovery.Environment(env); err != nil {
				return nil, err
			}
			return env, nil
		},
		login: func(ctx context.Context, env *service.AuthorizationEnvironment, signer xsapi.TokenAndSignaturer) (*playfab.Client, error) {
			return playfab.LoginWithXbox(ctx, env.PlayFabTitleID, signer, playfab.ClientConfig{CreateAccount: true})
		},
		services: func(env *service.AuthorizationEnvironment, tickets service.SessionTicketSource, token *service.Token) service.TokenSource {
			return env.ResumeTokenSource(tickets, clientplatform.TokenConfig(), token)
		},
		mint: func(ctx context.Context, env *service.AuthorizationEnvironment, source service.TokenSource, key *ecdsa.PublicKey) (string, error) {
			return minecraft.NewMultiplayerTokenSource(env, source).MultiplayerToken(ctx, key)
		},
	}
}

type derivedEnvironment struct {
	ServiceURI     string      `json:"service_uri"`
	Issuer         string      `json:"issuer"`
	PlayFabTitleID title.Title `json:"playfab_title_id"`
}

type derivedState struct {
	Version       int                 `json:"version"`
	OAuthBinding  string              `json:"oauth_binding"`
	ClientBinding string              `json:"client_binding"`
	Environment   *derivedEnvironment `json:"environment,omitempty"`
	DeviceToken   *xasd.Token         `json:"device_token,omitempty"`
	ProofKey      string              `json:"proof_key,omitempty"`
	SISU          *sisu.Snapshot      `json:"sisu,omitempty"`
	ServiceToken  *service.Token      `json:"service_token,omitempty"`
}

// Account is the per-account runtime; every method is safe for concurrent use.
type Account struct {
	ctx         context.Context
	cancel      context.CancelFunc
	gate        chan struct{}
	path        string
	diagnostics io.Writer
	oauth       oauth2.TokenSource
	binding     string
	client      string
	device      xasd.TokenSource
	deviceToken *xasd.Token
	session     *accountSession
	environment *service.AuthorizationEnvironment
	cachedEnv   *derivedEnvironment
	service     *service.Token
	services    service.TokenSource // native source seeded with service; rebuilt after every restore
	playfab     *playfab.Client     // logged in on first need; closed only by Close
	closed      atomic.Bool
	persisted   string
	rejected    map[string]*xsts.Token // XSTS tokens a relying party refused; re-evicted after every reload
	deps        derivedDeps
}

var (
	_ oauth2.TokenSource               = (*Account)(nil)
	_ xsapi.TokenSource                = (*Account)(nil)
	_ minecraft.MultiplayerTokenSource = (*Account)(nil)
	_ nsal.TokenInvalidator            = (*Account)(nil)
	_ service.TokenSource              = (*Account)(nil)
	_ service.TokenInvalidator         = (*Account)(nil)
)

// ErrAccountClosed is returned once the account has been signed out or shut down.
var ErrAccountClosed = errors.New("authentication: account is closed")

func newAccount(ctx context.Context, path string, oauth oauth2.TokenSource, diagnostics io.Writer, deps derivedDeps) *Account {
	if oauth == nil {
		return nil
	}
	if diagnostics == nil {
		diagnostics = io.Discard
	}
	ctx, cancel := context.WithCancel(ctx)
	source := &Account{ctx: ctx, cancel: cancel, gate: make(chan struct{}, 1), diagnostics: diagnostics, oauth: oauth, client: clientBinding(), deps: deps}
	defer func() {
		if source.session == nil {
			source.device = xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, nil, nil)
			source.session = newAccountSession(source, &sisu.SessionConfig{DeviceTokenSource: source.device})
		}
	}()
	tok, err := source.oauthToken(ctx)
	if err != nil || tok == nil {
		return source
	}
	source.binding = oauthBinding(tok)
	if path == "" {
		return source
	}
	path, err = filepath.Abs(path)
	if err != nil {
		return source
	}
	source.path = path
	binding, client := source.binding, source.client
	state, err := loadDerived(source.path)
	switch {
	case err == nil && state.OAuthBinding == binding && state.ClientBinding == client:
		if err := source.restore(state); err != nil {
			source.diagnostic("miss", "bundle", "invalid")
		} else {
			source.persisted = derivedFingerprint(state)
			source.diagnostic("hit", "bundle", "bound")
		}
	case err == nil:
		source.diagnostic("miss", "bundle", "binding")
		source.resetLocked(binding)
	case errors.Is(err, fs.ErrNotExist):
		source.diagnostic("miss", "bundle", "missing")
	case errors.Is(err, errDerivedCacheMiss):
		source.diagnostic("miss", "bundle", "invalid")
	default:
		source.diagnostic("miss", "bundle", "unsafe")
	}
	return source
}

func (s *Account) diagnostic(event, layer, reason string) {
	_, _ = fmt.Fprintf(s.diagnostics, "AUTH_ACCEL_CACHE event=%s layer=%s reason=%s\n", event, layer, reason)
}

// Token returns the OAuth credential only while this account is open.
func (s *Account) Token() (*oauth2.Token, error) {
	if err := s.lock(s.ctx); err != nil {
		return nil, err
	}
	defer s.unlock()
	return s.tokenLocked(s.ctx)
}

func (s *Account) DeviceToken(ctx context.Context) (*xasd.Token, error) {
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	defer s.unlock()
	if _, err := s.tokenLocked(ctx); err != nil {
		return nil, err
	}
	lease, err := s.acquireLeaseLocked(ctx)
	if err != nil {
		return nil, err
	}
	if lease != nil {
		defer lease.Close()
		s.reloadLocked()
	}
	publish := lease != nil
	before := s.deviceToken
	token, err := s.device.DeviceToken(ctx)
	if err != nil {
		return nil, err
	}
	s.deviceToken = token
	s.persistLocked(ctx, publish)
	if before != nil && before.Token == token.Token {
		s.diagnostic("reuse", "device", "valid")
	} else {
		s.diagnostic("refresh", "device", "expired")
	}
	return token, nil
}

func (s *Account) ProofKey() *ecdsa.PrivateKey {
	if err := s.lock(s.ctx); err != nil {
		return nil
	}
	defer s.unlock()
	if s.closed.Load() {
		return nil
	}
	return s.device.ProofKey()
}

func (s *Account) XSTSToken(ctx context.Context, relyingParty string) (*xsts.Token, error) {
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	defer s.unlock()
	if _, err := s.tokenLocked(ctx); err != nil {
		return nil, err
	}
	lease, err := s.acquireLeaseLocked(ctx)
	if err != nil {
		return nil, err
	}
	if lease != nil {
		defer lease.Close()
		s.reloadLocked()
	}
	publish := lease != nil
	before := s.session.Snapshot().XSTSTokens[relyingParty]
	token, err := s.session.XSTSToken(ctx, relyingParty)
	if err != nil {
		return nil, err
	}
	device, deviceErr := s.device.DeviceToken(ctx)
	if deviceErr == nil {
		s.deviceToken = device
	}
	if before == nil || before.Token != token.Token {
		s.updateOAuthBindingLocked(ctx)
	}
	s.persistLocked(ctx, publish)
	if before != nil && before.Token == token.Token {
		s.diagnostic("reuse", "xsts", "valid")
	} else {
		s.diagnostic("refresh", "xsts", "expired")
	}
	return token, nil
}

// InvalidateXSTSToken evicts rejected through SISU and persists the eviction so no reload, in this
// process or another, can resurrect it.
func (s *Account) InvalidateXSTSToken(relyingParty string, rejected *xsts.Token) {
	if rejected == nil || rejected.Token == "" {
		return
	}
	ctx, cancel := context.WithTimeout(s.ctx, 10*time.Second)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return
	}
	defer s.unlock()
	if s.closed.Load() {
		return
	}
	if s.rejected == nil {
		s.rejected = make(map[string]*xsts.Token)
	}
	s.rejected[relyingParty] = rejected
	lease, _ := s.acquireLeaseLocked(ctx)
	if lease != nil {
		defer lease.Close()
		s.reloadLocked()
	}
	s.session.InvalidateXSTSToken(relyingParty, rejected)
	s.diagnostic("invalidate", "xsts", "rejected")
	s.persistLocked(ctx, lease != nil)
}

// MultiplayerToken mints a key-bound multiplayer token from the shared service token.
func (s *Account) MultiplayerToken(ctx context.Context, key *ecdsa.PublicKey) (string, error) {
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if key == nil {
		return "", errors.New("authentication: connection proof key is absent")
	}
	env, err := s.Environment(ctx)
	if err != nil {
		return "", err
	}
	jwt, err := s.deps.mint(ctx, env, s, key)
	if err != nil {
		if ctx.Err() != nil {
			return "", ctx.Err()
		}
		return "", errors.New("authentication: mint multiplayer credential")
	}
	return jwt, nil
}

// ServiceToken returns the account's Minecraft service token from the shared native source,
// persisting it so other processes reuse it.
func (s *Account) ServiceToken(ctx context.Context) (*service.Token, error) {
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	defer s.unlock()
	if _, err := s.tokenLocked(ctx); err != nil {
		return nil, err
	}
	lease, err := s.acquireLeaseLocked(ctx)
	if err != nil {
		return nil, err
	}
	if lease != nil {
		defer lease.Close()
		s.reloadLocked()
	}
	if err := s.ensureEnvironmentLocked(ctx); err != nil {
		return nil, err
	}
	if s.services == nil {
		s.services = s.deps.services(s.environment, sessionTickets{s}, s.service)
	}
	before, session := s.service, sessionFingerprint(s.session.Snapshot())
	token, err := s.services.ServiceToken(ctx)
	if err != nil || token == nil || !token.Valid() {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, errors.New("authentication: refresh service credential")
	}
	if token == before {
		s.diagnostic("reuse", "service", "valid")
		return token, nil
	}
	s.diagnostic("refresh", "service", "expired")
	s.service = token
	if session != sessionFingerprint(s.session.Snapshot()) {
		s.updateOAuthBindingLocked(ctx)
	}
	s.persistLocked(ctx, lease != nil)
	return token, nil
}

// InvalidateServiceToken drops a service token a service refused and persists the eviction.
func (s *Account) InvalidateServiceToken(rejected *service.Token) {
	if rejected == nil {
		return
	}
	ctx, cancel := context.WithTimeout(s.ctx, 10*time.Second)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return
	}
	defer s.unlock()
	if s.closed.Load() {
		return
	}
	lease, _ := s.acquireLeaseLocked(ctx)
	if lease != nil {
		defer lease.Close()
		s.reloadLocked()
	}
	if invalidator, ok := s.services.(service.TokenInvalidator); ok {
		invalidator.InvalidateServiceToken(rejected)
	}
	if s.service != nil && s.service.AuthorizationHeader == rejected.AuthorizationHeader {
		s.service = nil
		s.services = nil
	}
	s.diagnostic("invalidate", "service", "rejected")
	s.persistLocked(ctx, lease != nil)
}

// Environment returns the discovered authorization environment.
func (s *Account) Environment(ctx context.Context) (*service.AuthorizationEnvironment, error) {
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	defer s.unlock()
	if _, err := s.tokenLocked(ctx); err != nil {
		return nil, err
	}
	if err := s.ensureEnvironmentLocked(ctx); err != nil {
		return nil, err
	}
	return s.environment, nil
}

// PlayFab returns the account's shared PlayFab client, logging in on first use; the account owns it.
func (s *Account) PlayFab(ctx context.Context) (*playfab.Client, error) {
	ctx, cancel := s.operationContext(ctx)
	defer cancel()
	if err := s.lock(ctx); err != nil {
		return nil, err
	}
	defer s.unlock()
	if _, err := s.tokenLocked(ctx); err != nil {
		return nil, err
	}
	if err := s.ensureEnvironmentLocked(ctx); err != nil {
		return nil, err
	}
	return s.playFabLocked(ctx)
}

// Closed reports whether shutdown, cancellation or a replaced sign-in ended this runtime.
func (s *Account) Closed() bool { return s.ctx.Err() != nil }

// Close cancels account operations, ends the PlayFab session and refuses further calls.
func (s *Account) Close() error {
	s.cancel() // Wake credential and lease waits before waiting for the account lock.
	s.gate <- struct{}{}
	defer s.unlock()
	s.closed.Store(true)
	s.services = nil
	return s.closePlayFabLocked()
}

func (s *Account) ensureEnvironmentLocked(ctx context.Context) error {
	if s.environment != nil {
		return nil
	}
	env, err := s.deps.discover(ctx)
	if err != nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("authentication: discover service environment")
	}
	if !validEnvironment(env) {
		return errors.New("authentication: invalid service environment")
	}
	fresh := snapshotEnvironment(env)
	if !sameEnvironment(s.cachedEnv, fresh) {
		s.service = nil
		s.services = nil
		s.diagnostic("miss", "service", "environment")
	}
	s.environment = env
	s.cachedEnv = fresh
	return nil
}

func (s *Account) playFabLocked(ctx context.Context) (*playfab.Client, error) {
	if s.playfab != nil {
		return s.playfab, nil
	}
	client, err := s.deps.login(ctx, s.environment, nsal.NewResolver(s.session))
	if err != nil {
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, errors.New("authentication: PlayFab login")
	}
	if ctx.Err() != nil {
		_ = client.Close()
		return nil, ctx.Err()
	}
	// A source can detect account replacement outside the account gate during SISU refresh.
	context.AfterFunc(s.ctx, func() { _ = client.Close() })
	s.playfab = client
	return client, nil
}

func (s *Account) closePlayFabLocked() error {
	if s.playfab == nil {
		return nil
	}
	err := s.playfab.Close()
	s.playfab = nil
	return err
}

// sessionTickets hands the native service-token source the shared PlayFab session; it is only
// called from ServiceToken, which holds the account lock.
type sessionTickets struct{ account *Account }

func (t sessionTickets) SessionTicket(ctx context.Context) (string, error) {
	client, err := t.account.playFabLocked(ctx)
	if err != nil {
		return "", err
	}
	return client.SessionTicket(ctx)
}

// updateOAuthBindingLocked records any rotation without outliving the caller's lease wait.
func (s *Account) updateOAuthBindingLocked(ctx context.Context) {
	token, err := s.oauthToken(ctx)
	if err == nil && token != nil {
		s.binding = oauthBinding(token)
	}
}

// lock serializes account state while allowing queued callers to cancel their wait.
func (s *Account) lock(ctx context.Context) error {
	if s.closed.Load() {
		return ErrAccountClosed
	}
	select {
	case s.gate <- struct{}{}:
		if s.closed.Load() {
			s.unlock()
			return ErrAccountClosed
		}
		return nil
	case <-ctx.Done():
		if s.closed.Load() {
			return ErrAccountClosed
		}
		return ctx.Err()
	}
}

// unlock lets the next queued account operation access the protected state.
func (s *Account) unlock() { <-s.gate }

// operationContext ends a caller's work when either it or the account closes.
func (s *Account) operationContext(ctx context.Context) (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithCancel(ctx)
	stop := context.AfterFunc(s.ctx, cancel)
	if s.ctx.Err() != nil {
		cancel()
	}
	return ctx, func() { stop(); cancel() }
}

// oauthToken propagates cancellation into cache leases. An already-running
// refresh may still finish and persist its rotation before cancellation is returned.
func (s *Account) oauthToken(ctx context.Context) (*oauth2.Token, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	var token *oauth2.Token
	var err error
	if cached, ok := s.oauth.(*persistingSource); ok {
		token, err = cached.token(ctx)
	} else {
		token, err = s.oauth.Token()
	}
	if errors.Is(err, errAccountChanged) {
		s.closed.Store(true)
		s.cancel()
		return nil, err
	}
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	return token, err
}

// tokenLocked applies the account lifecycle and OAuth binding rules at one boundary.
func (s *Account) tokenLocked(ctx context.Context) (*oauth2.Token, error) {
	if s.closed.Load() {
		return nil, ErrAccountClosed
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	token, err := s.oauthToken(ctx)
	if err != nil || token == nil {
		if errors.Is(err, errAccountChanged) {
			return nil, err
		}
		if ctx.Err() != nil {
			return nil, ctx.Err()
		}
		return nil, errors.New("authentication: validate OAuth credential")
	}
	binding := oauthBinding(token)
	if binding != s.binding {
		s.resetLocked(binding)
	}
	return token, nil
}

// acquireLeaseLocked bounds waits for the optional derived cache. A miss keeps
// the account usable in memory without publishing over another process's state.
func (s *Account) acquireLeaseLocked(ctx context.Context) (io.Closer, error) {
	if s.path == "" {
		return nil, nil
	}
	wait, cancel := context.WithTimeout(ctx, 5*time.Second)
	defer cancel()
	lease, err := lockfile.AcquireContext(wait, s.path+cacheLockSuffix)
	if err == nil {
		return lease, nil
	}
	if ctx.Err() != nil {
		return nil, ctx.Err()
	}
	s.diagnostic("miss", "write", "unavailable")
	return nil, nil
}

func (s *Account) reloadLocked() {
	if s.path == "" {
		return
	}
	state, err := loadDerived(s.path)
	if err != nil || state.OAuthBinding != s.binding || state.ClientBinding != s.client {
		return
	}
	if s.restore(state) == nil {
		s.persisted = derivedFingerprint(state)
		for relyingParty, token := range s.rejected {
			s.session.InvalidateXSTSToken(relyingParty, token)
		}
	}
}

func sessionFingerprint(snapshot *sisu.Snapshot) string {
	if snapshot == nil {
		return ""
	}
	b, err := json.Marshal(snapshot)
	if err != nil {
		return ""
	}
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func (s *Account) persistLocked(ctx context.Context, publish bool) {
	if !publish {
		return
	}
	device := s.deviceToken
	if device == nil {
		var err error
		device, err = s.device.DeviceToken(ctx)
		if err != nil {
			return
		}
		s.deviceToken = device
	}
	if device == nil || s.device.ProofKey() == nil {
		return
	}
	key, err := x509.MarshalECPrivateKey(s.device.ProofKey())
	if err != nil {
		return
	}
	environment := snapshotEnvironment(s.environment)
	if environment == nil {
		environment = s.cachedEnv
	}
	state := derivedState{
		Version:       derivedCacheVersion,
		OAuthBinding:  s.binding,
		ClientBinding: s.client,
		Environment:   environment,
		DeviceToken:   device,
		ProofKey:      base64.RawStdEncoding.EncodeToString(key),
		SISU:          s.session.Snapshot(),
		ServiceToken:  s.service,
	}
	b, err := json.Marshal(state)
	if err != nil || len(b) >= maxCacheSize {
		return
	}
	fingerprint := bytesFingerprint(b)
	if fingerprint == s.persisted {
		return
	}
	b = append(b, '\n')
	if err := savePrivate(s.path, b); err != nil {
		s.diagnostic("miss", "write", "contended")
		return
	}
	s.persisted = fingerprint
}

func (s *Account) resetLocked(binding string) {
	var proofKey *ecdsa.PrivateKey
	if s.device != nil {
		proofKey = s.device.ProofKey()
	}
	s.binding = binding
	s.environment = nil
	s.cachedEnv = nil
	s.service = nil
	s.services = nil // the PlayFab session is kept: a reset is an OAuth rotation of this process's account
	s.deviceToken = nil
	s.rejected = nil
	s.device = xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, nil, proofKey)
	s.session = newAccountSession(s, &sisu.SessionConfig{DeviceTokenSource: s.device})
	s.persisted = ""
}

func (s *Account) restore(state *derivedState) error {
	if state == nil || state.DeviceToken == nil || state.ProofKey == "" || state.SISU == nil {
		return errDerivedCacheMiss
	}
	der, err := base64.RawStdEncoding.DecodeString(state.ProofKey)
	if err != nil {
		return errDerivedCacheMiss
	}
	key, err := x509.ParseECPrivateKey(der)
	if err != nil || key.Curve == nil || key.Curve.Params().Name != "P-256" {
		return errDerivedCacheMiss
	}
	if s.device != nil && s.device.ProofKey() != nil {
		current := s.device.ProofKey()
		if current.Curve == nil || current.Curve.Params().Name != key.Curve.Params().Name || current.D.Cmp(key.D) != 0 {
			return errDerivedCacheMiss
		}
		key = current
	}
	var cachedEnv *derivedEnvironment
	if state.Environment != nil {
		if _, err := restoreEnvironment(state.Environment); err != nil {
			return errDerivedCacheMiss
		}
		cachedEnv = state.Environment
	}
	device := xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, state.DeviceToken, key)
	session := newAccountSession(s, &sisu.SessionConfig{Snapshot: state.SISU, DeviceTokenSource: device})
	var serviceToken *service.Token
	if state.ServiceToken != nil && state.ServiceToken.Valid() && cachedEnv != nil {
		serviceToken = state.ServiceToken
	}
	s.device = device
	s.deviceToken = state.DeviceToken
	s.session = session
	if !sameEnvironment(snapshotEnvironment(s.environment), cachedEnv) {
		s.environment = nil // a restored environment is checked against discovery once more
	}
	s.cachedEnv = cachedEnv
	s.service = serviceToken
	s.services = nil
	return nil
}

var errDerivedCacheMiss = errors.New("derived authentication cache miss")

func loadDerived(path string) (*derivedState, error) {
	b, err := loadPrivate(path, maxCacheSize)
	if err != nil {
		return nil, err
	}
	decoder := json.NewDecoder(bytes.NewReader(b))
	decoder.DisallowUnknownFields()
	var state derivedState
	if err := decoder.Decode(&state); err != nil {
		return nil, errDerivedCacheMiss
	}
	var trailing any
	if err := decoder.Decode(&trailing); !errors.Is(err, io.EOF) {
		return nil, errDerivedCacheMiss
	}
	if state.Version != derivedCacheVersion || state.OAuthBinding == "" || state.ClientBinding == "" {
		return nil, errDerivedCacheMiss
	}
	return &state, nil
}

func oauthBinding(token *oauth2.Token) string {
	h := sha256.New()
	for _, value := range []string{token.AccessToken, token.RefreshToken, token.TokenType} {
		var length [8]byte
		binary.BigEndian.PutUint64(length[:], uint64(len(value)))
		_, _ = h.Write(length[:])
		_, _ = h.Write([]byte(value))
	}
	return hex.EncodeToString(h.Sum(nil))
}

func derivedFingerprint(state *derivedState) string {
	b, err := json.Marshal(state)
	if err != nil {
		return ""
	}
	return bytesFingerprint(b)
}

func bytesFingerprint(b []byte) string {
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func clientBinding() string {
	b, _ := json.Marshal(struct {
		Config   auth.Config `json:"xal"`
		Protocol string      `json:"protocol"`
		App      string      `json:"application"`
	}{auth.AndroidConfig, protocol.CurrentVersion, service.ApplicationTypeMinecraftPE})
	sum := sha256.Sum256(b)
	return hex.EncodeToString(sum[:])
}

func snapshotEnvironment(env *service.AuthorizationEnvironment) *derivedEnvironment {
	if !validEnvironment(env) {
		return nil
	}
	return &derivedEnvironment{
		ServiceURI:     env.ServiceURI.String(),
		Issuer:         env.Issuer.String(),
		PlayFabTitleID: env.PlayFabTitleID,
	}
}

func restoreEnvironment(cached *derivedEnvironment) (*service.AuthorizationEnvironment, error) {
	serviceURI, err := url.Parse(cached.ServiceURI)
	if err != nil {
		return nil, err
	}
	issuer, err := url.Parse(cached.Issuer)
	if err != nil {
		return nil, err
	}
	env := &service.AuthorizationEnvironment{ServiceURI: serviceURI, Issuer: issuer, PlayFabTitleID: cached.PlayFabTitleID}
	if !validEnvironment(env) {
		return nil, fmt.Errorf("invalid environment")
	}
	return env, nil
}

func validEnvironment(env *service.AuthorizationEnvironment) bool {
	return env != nil && validHTTPSURL(env.ServiceURI) && validHTTPSURL(env.Issuer) && env.PlayFabTitleID != ""
}

func sameEnvironment(left, right *derivedEnvironment) bool {
	return left != nil && right != nil && left.ServiceURI == right.ServiceURI && left.Issuer == right.Issuer && left.PlayFabTitleID == right.PlayFabTitleID
}

func validHTTPSURL(value *url.URL) bool {
	return value != nil && value.Scheme == "https" && value.Host != "" && value.User == nil
}
