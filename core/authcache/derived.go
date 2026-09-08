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
	"net/http"
	"net/url"
	"os"
	"path/filepath"
	"sync"
	"time"

	"github.com/df-mc/go-playfab/v2"
	"github.com/df-mc/go-playfab/v2/title"
	"github.com/df-mc/go-xsapi/v2"
	"github.com/df-mc/go-xsapi/v2/xal/nsal"
	"github.com/df-mc/go-xsapi/v2/xal/sisu"
	"github.com/df-mc/go-xsapi/v2/xal/xasd"
	"github.com/df-mc/go-xsapi/v2/xal/xsts"
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

// PersistentSource returns an authenticated source that persists reusable,
// proof-key-bound Xbox and Minecraft service state. The OAuth source remains
// authoritative: replacing its token material makes the derived cache miss.
// Cache failures are optional misses and are reported without paths or secrets.
func PersistentSource(ctx context.Context, path string, oauth oauth2.TokenSource, diagnostics io.Writer) oauth2.TokenSource {
	return persistentSource(ctx, path, oauth, diagnostics, defaultDerivedDeps())
}

type derivedDeps struct {
	discover     func(context.Context) (*service.AuthorizationEnvironment, error)
	serviceToken func(context.Context, *service.AuthorizationEnvironment, xsapi.TokenAndSignaturer) (*service.Token, error)
	mint         func(context.Context, *service.AuthorizationEnvironment, service.TokenSource, *ecdsa.PublicKey) (string, error)
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
		serviceToken: func(ctx context.Context, env *service.AuthorizationEnvironment, signer xsapi.TokenAndSignaturer) (*service.Token, error) {
			client, err := playfab.LoginWithXbox(ctx, env.PlayFabTitleID, signer, playfab.ClientConfig{CreateAccount: true})
			if err != nil {
				return nil, err
			}
			defer client.Close()
			return env.TokenSource(client, service.TokenConfig{}).ServiceToken(ctx)
		},
		mint: func(ctx context.Context, env *service.AuthorizationEnvironment, source service.TokenSource, key *ecdsa.PublicKey) (string, error) {
			return env.MultiplayerToken(ctx, source, key)
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

type persistentAuthSource struct {
	mu          sync.Mutex
	path        string
	diagnostics io.Writer
	oauth       oauth2.TokenSource
	binding     string
	client      string
	device      xasd.TokenSource
	deviceToken *xasd.Token
	session     *sisu.Session
	environment *service.AuthorizationEnvironment
	cachedEnv   *derivedEnvironment
	service     *service.Token
	persisted   string
	deps        derivedDeps
}

var (
	_ oauth2.TokenSource               = (*persistentAuthSource)(nil)
	_ xsapi.TokenSource                = (*persistentAuthSource)(nil)
	_ minecraft.MultiplayerTokenSource = (*persistentAuthSource)(nil)
)

func persistentSource(ctx context.Context, path string, oauth oauth2.TokenSource, diagnostics io.Writer, deps derivedDeps) oauth2.TokenSource {
	if oauth == nil || path == "" {
		return oauth
	}
	if diagnostics == nil {
		diagnostics = io.Discard
	}
	tok, err := oauth.Token()
	if err != nil || tok == nil {
		return oauth
	}
	path, err = filepath.Abs(path)
	if err != nil {
		return oauth
	}
	binding := oauthBinding(tok)
	client := clientBinding()
	source := &persistentAuthSource{
		path:        filepath.Clean(path),
		diagnostics: diagnostics,
		oauth:       oauth,
		binding:     binding,
		client:      client,
		deps:        deps,
	}
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
	if source.session == nil {
		source.device = xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, nil, nil)
		source.session = auth.AndroidConfig.New(oauth, &sisu.SessionConfig{DeviceTokenSource: source.device})
	}
	_ = ctx // Construction deliberately performs no derived network exchange.
	return source
}

func (s *persistentAuthSource) diagnostic(event, layer, reason string) {
	_, _ = fmt.Fprintf(s.diagnostics, "AUTH_ACCEL_CACHE event=%s layer=%s reason=%s\n", event, layer, reason)
}

func (s *persistentAuthSource) Token() (*oauth2.Token, error) {
	tok, err := s.oauth.Token()
	if err != nil {
		return nil, err
	}
	if tok == nil {
		return nil, errors.New("authentication: OAuth source returned nil token")
	}
	next := oauthBinding(tok)
	s.mu.Lock()
	if next != s.binding {
		s.resetLocked(next)
	}
	s.mu.Unlock()
	return tok, nil
}

func (s *persistentAuthSource) DeviceToken(ctx context.Context) (*xasd.Token, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.ensureOAuthLocked(ctx); err != nil {
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
	before := s.deviceToken
	token, err := s.device.DeviceToken(ctx)
	if err != nil {
		return nil, err
	}
	s.deviceToken = token
	s.persistLocked(ctx)
	if before != nil && before.Token == token.Token {
		s.diagnostic("reuse", "device", "valid")
	} else {
		s.diagnostic("refresh", "device", "expired")
	}
	return token, nil
}

func (s *persistentAuthSource) ProofKey() *ecdsa.PrivateKey {
	s.mu.Lock()
	defer s.mu.Unlock()
	return s.device.ProofKey()
}

func (s *persistentAuthSource) XSTSToken(ctx context.Context, relyingParty string) (*xsts.Token, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := s.ensureOAuthLocked(ctx); err != nil {
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
		s.updateOAuthBindingLocked()
	}
	s.persistLocked(ctx)
	if before != nil && before.Token == token.Token {
		s.diagnostic("reuse", "xsts", "valid")
	} else {
		s.diagnostic("refresh", "xsts", "expired")
	}
	return token, nil
}

func (s *persistentAuthSource) MultiplayerToken(ctx context.Context, key *ecdsa.PublicKey) (string, error) {
	if key == nil {
		return "", errors.New("authentication: connection proof key is absent")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return "", err
	}
	if err := s.ensureOAuthLocked(ctx); err != nil {
		return "", err
	}
	lease, err := s.acquireLeaseLocked(ctx)
	if err != nil {
		return "", err
	}
	if lease != nil {
		defer lease.Close()
		s.reloadLocked()
	}
	if s.environment == nil {
		env, err := s.deps.discover(ctx)
		if err != nil {
			if ctx.Err() != nil {
				return "", ctx.Err()
			}
			return "", errors.New("authentication: discover service environment")
		}
		if !validEnvironment(env) {
			return "", errors.New("authentication: invalid service environment")
		}
		fresh := snapshotEnvironment(env)
		if !sameEnvironment(s.cachedEnv, fresh) {
			s.service = nil
			s.diagnostic("miss", "service", "environment")
		}
		s.environment = env
		s.cachedEnv = fresh
	}
	if s.service == nil || !s.service.Valid() {
		s.diagnostic("refresh", "service", "expired")
		if err := s.refreshServiceLocked(ctx); err != nil {
			return "", err
		}
	}
	jwt, err := s.deps.mint(ctx, s.environment, staticServiceToken{s.service}, key)
	if err == nil {
		s.diagnostic("reuse", "service", "valid")
		return jwt, nil
	}
	if ctx.Err() != nil {
		return "", ctx.Err()
	}
	var responseErr *service.ResponseError
	if !errors.As(err, &responseErr) || responseErr.StatusCode != http.StatusUnauthorized {
		return "", errors.New("authentication: mint multiplayer credential")
	}
	// A still-unexpired service token may have been revoked. Invalidate only
	// that layer and retry the refresh/mint sequence once.
	s.service = nil
	s.diagnostic("refresh", "service", "rejected")
	if err := s.refreshServiceLocked(ctx); err != nil {
		return "", err
	}
	jwt, err = s.deps.mint(ctx, s.environment, staticServiceToken{s.service}, key)
	if err != nil {
		if ctx.Err() != nil {
			return "", ctx.Err()
		}
		return "", errors.New("authentication: mint multiplayer credential after refresh")
	}
	s.persistLocked(ctx)
	return jwt, nil
}

func (s *persistentAuthSource) refreshServiceLocked(ctx context.Context) error {
	before := sessionFingerprint(s.session.Snapshot())
	token, err := s.deps.serviceToken(ctx, s.environment, nsal.NewResolver(s.session))
	if err != nil || token == nil || !token.Valid() {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("authentication: refresh service credential")
	}
	s.service = token
	if before != sessionFingerprint(s.session.Snapshot()) {
		s.updateOAuthBindingLocked()
	}
	s.persistLocked(ctx)
	return nil
}

func (s *persistentAuthSource) updateOAuthBindingLocked() {
	token, err := s.oauth.Token()
	if err == nil && token != nil {
		s.binding = oauthBinding(token)
	}
}

func (s *persistentAuthSource) ensureOAuthLocked(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	token, err := s.oauth.Token()
	if err != nil || token == nil {
		if ctx.Err() != nil {
			return ctx.Err()
		}
		return errors.New("authentication: validate OAuth credential")
	}
	binding := oauthBinding(token)
	if binding != s.binding {
		s.resetLocked(binding)
	}
	return nil
}

func (s *persistentAuthSource) acquireLeaseLocked(ctx context.Context) (io.Closer, error) {
	lockPath := s.path + ".lock"
	if err := prepareLeasePath(lockPath); err != nil {
		s.diagnostic("miss", "write", "unsafe")
		return nil, nil
	}
	deadline := time.NewTimer(5 * time.Second)
	defer deadline.Stop()
	for {
		lease, err := lockfile.Acquire(lockPath, 0)
		if err == nil {
			return lease, nil
		}
		if !errors.Is(err, lockfile.ErrBusy) {
			s.diagnostic("miss", "write", "unsafe")
			return nil, nil
		}
		retry := time.NewTimer(20 * time.Millisecond)
		select {
		case <-ctx.Done():
			retry.Stop()
			return nil, ctx.Err()
		case <-deadline.C:
			retry.Stop()
			s.diagnostic("miss", "write", "contended")
			return nil, nil
		case <-retry.C:
		}
	}
}

func prepareLeasePath(path string) error {
	var lastErr error
	for range 5 {
		_, err := os.Lstat(path)
		if errors.Is(err, fs.ErrNotExist) {
			if _, err := createPrivateOnce(path, []byte("join-auth-lease\n")); err != nil {
				lastErr = err
				time.Sleep(20 * time.Millisecond)
				continue
			}
		} else if err != nil {
			return errors.New("inspect authentication lease")
		}
		if err := validateLeasePath(path); err == nil {
			return nil
		} else {
			lastErr = err
		}
		time.Sleep(20 * time.Millisecond)
	}
	return lastErr
}

func validateLeasePath(path string) error {
	canonical, err := canonicalizeCachePath(filepath.Clean(path))
	if err != nil || canonical != filepath.Clean(path) {
		return errors.New("resolve authentication lease path")
	}
	parents, err := snapshotDirectoryChain(filepath.Dir(canonical))
	if err != nil || !parents.complete {
		return errors.New("inspect authentication lease parent")
	}
	if err := parents.revalidate(); err != nil {
		return errors.New("authentication lease parent changed")
	}
	info, err := os.Lstat(canonical)
	if errors.Is(err, fs.ErrNotExist) {
		return nil
	}
	if err != nil {
		return errors.New("inspect authentication lease")
	}
	if err := checkRegular(info); err != nil {
		return err
	}
	if err := checkCacheSecurityByPath(canonical, info); err != nil {
		return err
	}
	return parents.revalidate()
}

func (s *persistentAuthSource) reloadLocked() {
	state, err := loadDerived(s.path)
	if err != nil || state.OAuthBinding != s.binding || state.ClientBinding != s.client {
		return
	}
	if s.restore(state) == nil {
		s.persisted = derivedFingerprint(state)
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

type staticServiceToken struct{ token *service.Token }

func (s staticServiceToken) ServiceToken(context.Context) (*service.Token, error) {
	if s.token == nil || !s.token.Valid() {
		return nil, errors.New("authentication: service credential expired")
	}
	return s.token, nil
}

func (s *persistentAuthSource) persistLocked(ctx context.Context) {
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

func (s *persistentAuthSource) resetLocked(binding string) {
	s.binding = binding
	s.environment = nil
	s.cachedEnv = nil
	s.service = nil
	s.deviceToken = nil
	s.device = xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, nil, nil)
	s.session = auth.AndroidConfig.New(s.oauth, &sisu.SessionConfig{DeviceTokenSource: s.device})
	b, err := json.Marshal(derivedState{
		Version:       derivedCacheVersion,
		OAuthBinding:  binding,
		ClientBinding: s.client,
	})
	if err == nil {
		if err := savePrivate(s.path, append(b, '\n')); err != nil {
			s.diagnostic("miss", "write", "contended")
		} else {
			s.persisted = bytesFingerprint(b)
		}
	}
}

func (s *persistentAuthSource) restore(state *derivedState) error {
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
	var cachedEnv *derivedEnvironment
	if state.Environment != nil {
		if _, err := restoreEnvironment(state.Environment); err != nil {
			return errDerivedCacheMiss
		}
		cachedEnv = state.Environment
	}
	device := xasd.ReuseTokenSource(auth.AndroidConfig.Config.Config, state.DeviceToken, key)
	session := auth.AndroidConfig.New(s.oauth, &sisu.SessionConfig{Snapshot: state.SISU, DeviceTokenSource: device})
	var serviceToken *service.Token
	if state.ServiceToken != nil && state.ServiceToken.Valid() && cachedEnv != nil {
		serviceToken = state.ServiceToken
	}
	s.device = device
	s.deviceToken = state.DeviceToken
	s.session = session
	s.environment = nil
	s.cachedEnv = cachedEnv
	s.service = serviceToken
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
