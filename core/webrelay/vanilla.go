package webrelay

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"net/url"
	"os"
	"regexp"
	"strconv"
	"time"
)

// VanillaSource is the pinned vanilla resource pack from assets/vanilla-source.json. The relay
// streams it to browsers, which cannot fetch the release directly, and stores none of it; the
// browser checks SHA256 before using a byte.
type VanillaSource struct {
	URL    string `json:"url"`
	SHA256 string `json:"sha256"`
}

var sha256Hex = regexp.MustCompile(`^[0-9a-f]{64}$`)

// LoadVanillaSource reads the manifest at path and accepts only an HTTPS URL and a SHA-256.
func LoadVanillaSource(path string) (VanillaSource, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return VanillaSource{}, err
	}
	var manifest struct {
		Schema int `json:"schema"`
		VanillaSource
	}
	if err := json.Unmarshal(data, &manifest); err != nil {
		return VanillaSource{}, fmt.Errorf("vanilla source: %w", err)
	}
	parsed, err := url.Parse(manifest.URL)
	if manifest.Schema != 1 || err != nil || parsed.Scheme != "https" || !sha256Hex.MatchString(manifest.SHA256) {
		return VanillaSource{}, errors.New("vanilla source: want schema 1, an https url and a sha256")
	}
	return manifest.VanillaSource, nil
}

// vanillaStreams bounds concurrent pass-through downloads.
const vanillaStreams = 4

var vanillaClient = &http.Client{Timeout: 30 * time.Minute}

// serveVanilla streams the pinned archive without caching it.
func (relay *Relay) serveVanilla(w http.ResponseWriter, r *http.Request) {
	if relay.cfg.Vanilla.URL == "" {
		http.NotFound(w, r)
		return
	}
	if !relay.allow(relay.clientAddr(r)) {
		http.Error(w, "rate limited", http.StatusTooManyRequests)
		return
	}
	select {
	case relay.vanilla <- struct{}{}:
		defer func() { <-relay.vanilla }()
	default:
		http.Error(w, "busy", http.StatusServiceUnavailable)
		return
	}
	ctx, cancel := context.WithCancel(r.Context())
	defer cancel()
	request, err := http.NewRequestWithContext(ctx, http.MethodGet, relay.cfg.Vanilla.URL, nil)
	if err != nil {
		http.Error(w, "unavailable", http.StatusBadGateway)
		return
	}
	response, err := vanillaClient.Do(request)
	if err != nil {
		http.Error(w, "unavailable", http.StatusBadGateway)
		return
	}
	defer response.Body.Close()
	if response.StatusCode != http.StatusOK {
		http.Error(w, "unavailable", http.StatusBadGateway)
		return
	}
	header := w.Header()
	header.Set("Content-Type", "application/zip")
	header.Set("Cache-Control", "no-store")
	header.Set("X-Content-SHA256", relay.cfg.Vanilla.SHA256)
	if response.ContentLength > 0 {
		header.Set("Content-Length", strconv.FormatInt(response.ContentLength, 10))
	}
	_, _ = io.Copy(w, response.Body)
}
