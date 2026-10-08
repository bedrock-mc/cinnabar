package update

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

// Progress describes a background download or its verified staged result.
type Progress struct {
	State      string `json:"state"`
	Downloaded int64  `json:"downloaded,omitempty"`
	Total      int64  `json:"total,omitempty"`
	Stage      string `json:"stage,omitempty"`
	Latest     string `json:"latest,omitempty"`
	NotesURL   string `json:"notes_url,omitempty"`
}

// Download checks the channel and saves a verified artifact and its signed envelope.
// Unique staging directories make interrupted downloads and retries independent.
func Download(ctx context.Context, cfg Config, cache string, progress func(Progress)) (Progress, error) {
	body, err := fetch(ctx, cfg)
	if err != nil {
		return Progress{}, err
	}
	manifest, artifact, err := selected(body, cfg)
	if err != nil {
		return Progress{}, err
	}
	newer, err := Newer(manifest.Version, cfg.Current)
	if err != nil {
		return Progress{}, err
	}
	if !newer {
		return Progress{State: "current"}, nil
	}
	if err := os.MkdirAll(cache, 0o700); err != nil {
		return Progress{}, err
	}
	stage, err := os.MkdirTemp(cache, "update-")
	if err != nil {
		return Progress{}, err
	}
	stage, err = filepath.Abs(stage)
	if err != nil {
		return Progress{}, err
	}
	ready := false
	defer func() {
		if !ready {
			_ = os.RemoveAll(stage)
		}
	}()
	part := filepath.Join(stage, "artifact.part")
	if err := downloadArtifact(ctx, cfg, artifact, part, progress); err != nil {
		return Progress{}, err
	}
	if err := os.WriteFile(filepath.Join(stage, "manifest.json"), body, 0o600); err != nil {
		return Progress{}, err
	}
	if err := os.Rename(part, filepath.Join(stage, "artifact")); err != nil {
		return Progress{}, err
	}
	ready = true
	return Progress{State: "ready", Stage: stage, Latest: manifest.Version, NotesURL: manifest.NotesURL}, nil
}

// selected rechecks the envelope and chooses only the requested channel and platform.
func selected(body []byte, cfg Config) (Manifest, Artifact, error) {
	now := time.Now
	if cfg.Now != nil {
		now = cfg.Now
	}
	manifest, err := Verify(body, cfg.Keys, now())
	if err != nil {
		return Manifest{}, Artifact{}, err
	}
	if manifest.Channel != cfg.Channel {
		return Manifest{}, Artifact{}, errors.New("manifest channel does not match requested channel")
	}
	artifact, ok := manifest.Artifacts[cfg.Platform]
	if !ok {
		return Manifest{}, Artifact{}, fmt.Errorf("no artifact for %s", cfg.Platform)
	}
	if err := validateArtifact(artifact); err != nil {
		return Manifest{}, Artifact{}, err
	}
	return manifest, artifact, nil
}

// downloadArtifact writes a bounded temporary file and validates it before staging.
func downloadArtifact(ctx context.Context, cfg Config, artifact Artifact, path string, progress func(Progress)) error {
	if progress != nil {
		progress(Progress{State: "downloading", Total: artifact.Size})
	}
	req, err := http.NewRequestWithContext(ctx, http.MethodGet, artifact.URL, nil)
	if err != nil {
		return err
	}
	client := cfg.Client
	if client == nil {
		client = &http.Client{Timeout: 30 * time.Minute}
	}
	clientCopy := *client
	previousRedirect := client.CheckRedirect
	clientCopy.CheckRedirect = func(req *http.Request, via []*http.Request) error {
		if req.URL.Scheme != "https" {
			return errors.New("artifact redirect must use HTTPS")
		}
		if previousRedirect != nil {
			return previousRedirect(req, via)
		}
		if len(via) >= 10 {
			return errors.New("too many artifact redirects")
		}
		return nil
	}
	resp, err := clientCopy.Do(req)
	if err != nil {
		return fmt.Errorf("download artifact: %w", err)
	}
	defer resp.Body.Close()
	if resp.StatusCode != http.StatusOK {
		return fmt.Errorf("download artifact: status %d", resp.StatusCode)
	}
	file, err := os.OpenFile(path, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0o600)
	if err != nil {
		return err
	}
	writer := &progressWriter{writer: file, total: artifact.Size, notify: progress}
	_, copyErr := io.Copy(writer, io.LimitReader(resp.Body, artifact.Size+1))
	syncErr := file.Sync()
	closeErr := file.Close()
	if err := errors.Join(copyErr, syncErr, closeErr); err != nil {
		return err
	}
	return verifyFile(path, artifact)
}

type progressWriter struct {
	writer         io.Writer
	total, written int64
	last           time.Time
	notify         func(Progress)
}

// Write reports bounded-rate progress without buffering the artifact in memory.
func (w *progressWriter) Write(data []byte) (int, error) {
	n, err := w.writer.Write(data)
	w.written += int64(n)
	if w.notify != nil && (time.Since(w.last) >= 200*time.Millisecond || w.written >= w.total) {
		w.notify(Progress{State: "downloading", Downloaded: w.written, Total: w.total})
		w.last = time.Now()
	}
	return n, err
}

// verifyFile checks both signed size and digest before a file may be installed.
func verifyFile(path string, artifact Artifact) error {
	file, err := os.Open(path)
	if err != nil {
		return err
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return err
	}
	if !info.Mode().IsRegular() || info.Size() != artifact.Size {
		return errors.New("artifact size does not match signed manifest")
	}
	hash := sha256.New()
	n, err := io.Copy(hash, io.LimitReader(file, artifact.Size+1))
	if err != nil {
		return err
	}
	if n != artifact.Size {
		return errors.New("artifact size changed during verification")
	}
	if !strings.EqualFold(hex.EncodeToString(hash.Sum(nil)), artifact.SHA256) {
		return errors.New("artifact SHA-256 does not match signed manifest")
	}
	return nil
}
