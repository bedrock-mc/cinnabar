// Package update verifies signed release manifests and reports whether a newer build exists.
package update

import (
	"context"
	"crypto/ed25519"
	"encoding/base64"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"
)

const maxEnvelopeBytes = 1 << 20

// Envelope is the served document: a base64 manifest plus a detached Ed25519 signature over its bytes.
type Envelope struct {
	KeyID     string `json:"key_id"`
	Signature string `json:"signature"`
	Manifest  string `json:"manifest"`
}

// Artifact is one downloadable build for a platform key such as "macos-arm64".
type Artifact struct {
	URL    string `json:"url"`
	SHA256 string `json:"sha256"`
	Size   int64  `json:"size"`
}

// Manifest describes the newest build on a channel.
type Manifest struct {
	Schema    int                 `json:"schema"`
	Channel   string              `json:"channel"`
	Version   string              `json:"version"`
	Expires   time.Time           `json:"expires"`
	NotesURL  string              `json:"notes_url,omitempty"`
	Artifacts map[string]Artifact `json:"artifacts"`
}

// Result is the outcome of a check; Artifact is set only when Available.
type Result struct {
	Available bool      `json:"available"`
	Current   string    `json:"current"`
	Latest    string    `json:"latest"`
	NotesURL  string    `json:"notes_url,omitempty"`
	Artifact  *Artifact `json:"artifact,omitempty"`
}

// Config selects what to check. Keys maps key IDs to trusted public keys.
type Config struct {
	ManifestURL string
	Channel     string
	Platform    string
	Current     string
	Keys        map[string]ed25519.PublicKey
	Client      *http.Client
	Now         func() time.Time
	AllowHTTP   bool // loopback tests only
}

// Check fetches and verifies the manifest, then compares it with the running version.
func Check(ctx context.Context, cfg Config) (Result, error) {
	body, err := fetch(ctx, cfg)
	if err != nil {
		return Result{}, err
	}
	now := time.Now
	if cfg.Now != nil {
		now = cfg.Now
	}
	manifest, err := Verify(body, cfg.Keys, now())
	if err != nil {
		return Result{}, err
	}
	if manifest.Channel != cfg.Channel {
		return Result{}, fmt.Errorf("manifest channel %q does not match %q", manifest.Channel, cfg.Channel)
	}
	result := Result{Current: cfg.Current, Latest: manifest.Version, NotesURL: manifest.NotesURL}
	newer, err := Newer(manifest.Version, cfg.Current)
	if err != nil {
		return Result{}, err
	}
	if !newer {
		return result, nil
	}
	artifact, ok := manifest.Artifacts[cfg.Platform]
	if !ok {
		return result, nil
	}
	if err := validateArtifact(artifact); err != nil {
		return Result{}, err
	}
	result.Available, result.Artifact = true, &artifact
	return result, nil
}

// Verify checks the signature against a trusted key and returns the parsed, unexpired manifest.
func Verify(envelopeJSON []byte, keys map[string]ed25519.PublicKey, now time.Time) (Manifest, error) {
	var envelope Envelope
	if err := json.Unmarshal(envelopeJSON, &envelope); err != nil {
		return Manifest{}, fmt.Errorf("decode envelope: %w", err)
	}
	key, ok := keys[envelope.KeyID]
	if !ok || len(key) != ed25519.PublicKeySize {
		return Manifest{}, fmt.Errorf("unknown signing key %q", envelope.KeyID)
	}
	payload, err := base64.StdEncoding.DecodeString(envelope.Manifest)
	if err != nil {
		return Manifest{}, fmt.Errorf("decode manifest: %w", err)
	}
	signature, err := base64.StdEncoding.DecodeString(envelope.Signature)
	if err != nil || !ed25519.Verify(key, payload, signature) {
		return Manifest{}, errors.New("manifest signature is invalid")
	}
	var manifest Manifest
	if err := json.Unmarshal(payload, &manifest); err != nil {
		return Manifest{}, fmt.Errorf("decode signed manifest: %w", err)
	}
	if manifest.Schema != 1 {
		return Manifest{}, fmt.Errorf("unsupported manifest schema %d", manifest.Schema)
	}
	if !manifest.Expires.After(now) {
		return Manifest{}, errors.New("manifest has expired")
	}
	return manifest, nil
}

// Newer reports whether candidate is a higher dotted-numeric version than current; a pre-release suffix sorts lower.
func Newer(candidate, current string) (bool, error) {
	c, err := parseVersion(candidate)
	if err != nil {
		return false, err
	}
	r, err := parseVersion(current)
	if err != nil {
		return false, err
	}
	for i := range c.parts {
		if c.parts[i] != r.parts[i] {
			return c.parts[i] > r.parts[i], nil
		}
	}
	return r.pre != "" && c.pre == "", nil
}

type version struct {
	parts [3]uint64
	pre   string
}

func parseVersion(raw string) (version, error) {
	core, pre, _ := strings.Cut(strings.TrimPrefix(raw, "v"), "-")
	fields := strings.Split(core, ".")
	if len(fields) != 3 {
		return version{}, fmt.Errorf("invalid version %q", raw)
	}
	v := version{pre: pre}
	for i, field := range fields {
		n, err := strconv.ParseUint(field, 10, 32)
		if err != nil {
			return version{}, fmt.Errorf("invalid version %q", raw)
		}
		v.parts[i] = n
	}
	return v, nil
}

func validateArtifact(a Artifact) error {
	u, err := url.Parse(a.URL)
	if err != nil || u.Scheme != "https" || u.Host == "" {
		return fmt.Errorf("artifact URL %q is not HTTPS", a.URL)
	}
	if len(a.SHA256) != 64 || strings.Trim(strings.ToLower(a.SHA256), "0123456789abcdef") != "" {
		return errors.New("artifact sha256 is malformed")
	}
	if a.Size <= 0 {
		return errors.New("artifact size must be positive")
	}
	return nil
}

func fetch(ctx context.Context, cfg Config) ([]byte, error) {
	u, err := url.Parse(cfg.ManifestURL)
	if err != nil || (u.Scheme != "https" && !(cfg.AllowHTTP && u.Scheme == "http")) {
		return nil, errors.New("manifest URL must be HTTPS")
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, u.String(), nil)
	if err != nil {
		return nil, err
	}
	client := cfg.Client
	if client == nil {
		client = &http.Client{Timeout: 15 * time.Second}
	}
	resp, err := client.Do(req)
	if err != nil {
		return nil, fmt.Errorf("fetch manifest: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return nil, fmt.Errorf("fetch manifest: status %d", resp.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(resp.Body, maxEnvelopeBytes+1))
	if err != nil {
		return nil, fmt.Errorf("read manifest: %w", err)
	}
	if len(body) > maxEnvelopeBytes {
		return nil, errors.New("manifest exceeds size limit")
	}
	return body, nil
}

// ParseKeys decodes comma-separated "id:base64" trusted-key lists.
func ParseKeys(list string) (map[string]ed25519.PublicKey, error) {
	keys := map[string]ed25519.PublicKey{}
	for _, entry := range strings.Split(list, ",") {
		if entry = strings.TrimSpace(entry); entry == "" {
			continue
		}
		id, encoded, ok := strings.Cut(entry, ":")
		raw, err := base64.StdEncoding.DecodeString(encoded)
		if !ok || id == "" || err != nil || len(raw) != ed25519.PublicKeySize {
			return nil, fmt.Errorf("invalid trusted key entry %q", id)
		}
		keys[id] = ed25519.PublicKey(raw)
	}
	return keys, nil
}

// Sign marshals the manifest and wraps it in a signed envelope for publication.
func Sign(manifest Manifest, keyID string, key ed25519.PrivateKey) ([]byte, error) {
	if len(key) != ed25519.PrivateKeySize {
		return nil, errors.New("invalid signing key")
	}
	payload, err := json.Marshal(manifest)
	if err != nil {
		return nil, err
	}
	return json.Marshal(Envelope{
		KeyID:     keyID,
		Signature: base64.StdEncoding.EncodeToString(ed25519.Sign(key, payload)),
		Manifest:  base64.StdEncoding.EncodeToString(payload),
	})
}
