package ingress

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"time"
)

type audioRecord struct {
	SHA256 string `json:"sha256"`
	File   string `json:"file"`
	Size   int64  `json:"size"`
}
type Audio struct {
	manifest []byte
	files    map[string]audioFile
}
type audioFile struct {
	file *os.File
	size int64
	mime string
}

// LoadAudio pins independently decoded canonical audio assets. Replay files
// retain only sound events; all recordings reuse this verified artwork bundle.
func LoadAudio(directory string) (*Audio, error) {
	if directory == "" {
		return nil, nil
	}
	data, err := os.ReadFile(filepath.Join(directory, "manifest.json"))
	if err != nil {
		return nil, err
	}
	if len(data) > 8<<20 {
		return nil, errors.New("audio manifest exceeds limit")
	}
	var manifest struct {
		Version int                    `json:"version"`
		Files   map[string]audioRecord `json:"files"`
	}
	if json.Unmarshal(data, &manifest) != nil || manifest.Version != 1 || len(manifest.Files) == 0 || len(manifest.Files) > 8192 {
		return nil, errors.New("invalid audio manifest")
	}
	a := &Audio{manifest: data, files: map[string]audioFile{}}
	success := false
	defer func() {
		if !success {
			a.Close()
		}
	}()
	var total int64
	for _, record := range manifest.Files {
		if len(record.SHA256) != 64 || record.Size <= 0 || record.Size > 4<<20 || record.File != record.SHA256+".wav" && record.File != record.SHA256+".ogg" {
			return nil, errors.New("invalid audio record")
		}
		route := record.SHA256 + "/" + record.File
		if _, found := a.files[route]; found {
			continue
		}
		info, err := os.Lstat(filepath.Join(directory, record.File))
		if err != nil || !info.Mode().IsRegular() || info.Size() != record.Size {
			return nil, errors.New("invalid audio file")
		}
		file, err := os.Open(filepath.Join(directory, record.File))
		if err != nil {
			return nil, err
		}
		hash := sha256.New()
		_, err = io.Copy(hash, file)
		if err != nil || hex.EncodeToString(hash.Sum(nil)) != record.SHA256 {
			file.Close()
			return nil, errors.New("audio asset hash mismatch")
		}
		mime := "audio/wav"
		if strings.HasSuffix(record.File, ".ogg") {
			mime = "audio/ogg"
		}
		a.files[route] = audioFile{file: file, size: record.Size, mime: mime}
		total += record.Size
		if total > 640<<20 {
			return nil, errors.New("audio bundle exceeds limit")
		}
	}
	success = true
	return a, nil
}
func (a *Audio) Close() {
	if a != nil {
		for _, file := range a.files {
			_ = file.file.Close()
		}
	}
}
func (h *Handler) SetAudio(audio *Audio) { h.audio = audio }
func (h *Handler) audioRoute(w http.ResponseWriter, r *http.Request) {
	if h.audio == nil {
		failure(w, 503, "Replay audio is unavailable.")
		return
	}
	_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(15 * time.Second))
	if r.URL.Path == "/api/spectator/audio/manifest" {
		w.Header().Set("Content-Type", "application/json")
		if r.Method != http.MethodHead {
			_, _ = w.Write(h.audio.manifest)
		}
		return
	}
	route := strings.TrimPrefix(r.URL.Path, "/api/spectator/audio/")
	file, found := h.audio.files[route]
	if !found {
		failure(w, 404, "Audio asset not found.")
		return
	}
	select {
	case h.downloads <- struct{}{}:
		defer func() { <-h.downloads }()
	default:
		w.Header().Set("Retry-After", "2")
		failure(w, 429, "Audio downloads are busy.")
		return
	}
	w.Header().Set("Content-Type", file.mime)
	w.Header().Set("Content-Length", fmt.Sprint(file.size))
	if r.Method != http.MethodHead {
		_, _ = io.Copy(w, io.NewSectionReader(file.file, 0, file.size))
	}
}
