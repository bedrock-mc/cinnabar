package ingress

import (
	"compress/gzip"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"time"
)

type AssetRecord struct {
	Name   string `json:"name"`
	File   string `json:"file"`
	SHA256 string `json:"sha256"`
	Size   int64  `json:"size"`
}
type AssetManifest struct {
	Version  int           `json:"version"`
	Protocol int           `json:"protocol"`
	Source   string        `json:"source"`
	Files    []AssetRecord `json:"files"`
}

var requiredAssetNames = []string{"world", "registry", "entities", "equipment", "hud", "icons", "ui", "font"}
var optionalAssetNames = []string{"actors", "particles"}

type assetFile struct {
	record         AssetRecord
	file           *os.File
	compressed     *os.File
	compressedSize int64
}

// Assets holds verified, immutable carriers outside the repository and image.
// Opening once pins the bytes served even if a deployment changes a symlink.
type Assets struct {
	manifest AssetManifest
	files    map[string]assetFile
}

func LoadAssets(directory string) (*Assets, error) {
	if directory == "" {
		return nil, nil
	}
	data, err := os.ReadFile(filepath.Join(directory, "manifest.json"))
	if err != nil {
		return nil, err
	}
	if len(data) > 16<<10 {
		return nil, errors.New("asset manifest too large")
	}
	var manifest AssetManifest
	if err = json.Unmarshal(data, &manifest); err != nil {
		return nil, err
	}
	if manifest.Version != 1 || manifest.Protocol <= 0 || manifest.Source == "" || len(manifest.Files) < 8 || len(manifest.Files) > 10 {
		return nil, errors.New("invalid asset manifest")
	}
	assets := &Assets{manifest: manifest, files: make(map[string]assetFile)}
	ok := false
	defer func() {
		if !ok {
			assets.Close()
		}
	}()
	names := make(map[string]bool)
	for _, name := range append(append([]string(nil), requiredAssetNames...), optionalAssetNames...) {
		names[name] = false
	}
	var total int64
	for _, record := range manifest.Files {
		seen, known := names[record.Name]
		if !known || seen || record.File == "." || record.File == "" || filepath.Base(record.File) != record.File || strings.ContainsAny(record.File, "\\\x00") || len(record.SHA256) != 64 || record.Size <= 0 || record.Size > 128<<20 {
			return nil, errors.New("invalid asset record")
		}
		raw, err := hex.DecodeString(record.SHA256)
		if err != nil || len(raw) != 32 || strings.ToLower(record.SHA256) != record.SHA256 {
			return nil, errors.New("invalid asset hash")
		}
		path := filepath.Join(directory, record.File)
		info, err := os.Lstat(path)
		if err != nil || !info.Mode().IsRegular() || info.Size() != record.Size {
			return nil, fmt.Errorf("invalid asset file: %s", record.Name)
		}
		file, err := os.Open(path)
		if err != nil {
			return nil, err
		}
		digest := sha256.New()
		_, err = io.Copy(digest, io.NewSectionReader(file, 0, record.Size))
		if err != nil || hex.EncodeToString(digest.Sum(nil)) != record.SHA256 {
			file.Close()
			return nil, fmt.Errorf("asset hash mismatch: %s", record.Name)
		}
		route := record.SHA256 + "/" + record.File
		if _, duplicate := assets.files[route]; duplicate {
			file.Close()
			return nil, errors.New("duplicate asset file")
		}
		asset := assetFile{record: record, file: file}
		assets.files[route] = asset
		if compressedInfo, err := os.Lstat(path + ".gz"); err == nil {
			if !compressedInfo.Mode().IsRegular() || compressedInfo.Size() > record.Size+1<<20 {
				return nil, errors.New("invalid compressed asset")
			}
			compressed, err := os.Open(path + ".gz")
			if err != nil {
				return nil, err
			}
			asset.compressed, asset.compressedSize = compressed, compressedInfo.Size()
			assets.files[route] = asset
			reader, err := gzip.NewReader(compressed)
			if err != nil {
				return nil, err
			}
			digest := sha256.New()
			count, err := io.Copy(digest, io.LimitReader(reader, record.Size+1))
			closeErr := reader.Close()
			if err != nil || closeErr != nil || count != record.Size || hex.EncodeToString(digest.Sum(nil)) != record.SHA256 {
				return nil, errors.New("compressed asset hash mismatch")
			}
		} else if !os.IsNotExist(err) {
			return nil, err
		}
		names[record.Name] = true
		total += record.Size
		if total > 256<<20 {
			return nil, errors.New("asset bundle too large")
		}
	}
	for _, name := range requiredAssetNames {
		if !names[name] {
			return nil, errors.New("required asset missing")
		}
	}
	ok = true
	return assets, nil
}
func (a *Assets) Close() {
	if a != nil {
		for _, asset := range a.files {
			_ = asset.file.Close()
			if asset.compressed != nil {
				_ = asset.compressed.Close()
			}
		}
	}
}
func (h *Handler) assetsRoute(w http.ResponseWriter, r *http.Request) {
	if h.assets == nil {
		failure(w, http.StatusServiceUnavailable, "The renderer assets are not available.")
		return
	}
	if r.URL.Path == "/api/spectator/assets" || r.URL.Path == "/api/spectator/assets/manifest-v10" {
		w.Header().Set("Cache-Control", "public, max-age=300")
		manifest := h.assets.manifest
		if r.URL.Path == "/api/spectator/assets" {
			// Preserve the shipped renderer's strict eight-carrier contract while
			// the new renderer opts into its actor and particle carriers.
			manifest.Files = make([]AssetRecord, 0, len(requiredAssetNames))
			for _, record := range h.assets.manifest.Files {
				for _, name := range requiredAssetNames {
					if record.Name == name {
						manifest.Files = append(manifest.Files, record)
						break
					}
				}
			}
		}
		writeJSON(w, r, manifest)
		return
	}
	route, ok := strings.CutPrefix(r.URL.Path, "/api/spectator/assets/")
	asset, found := h.assets.files[route]
	if !ok || !found {
		failure(w, http.StatusNotFound, "Renderer asset not found.")
		return
	}
	if r.Method == http.MethodGet {
		select {
		case h.downloads <- struct{}{}:
			defer func() { <-h.downloads }()
		default:
			w.Header().Set("Retry-After", "2")
			failure(w, http.StatusTooManyRequests, "Renderer downloads are busy. Try again shortly.")
			return
		}
	}
	_ = http.NewResponseController(w).SetWriteDeadline(time.Now().Add(60 * time.Second))
	w.Header().Set("Content-Type", "application/octet-stream")
	w.Header().Set("Cache-Control", "public, max-age=31536000, immutable")
	w.Header().Set("ETag", `"`+asset.record.SHA256+`"`)
	w.Header().Set("Vary", "Accept-Encoding")
	if asset.compressed != nil && r.Header.Get("Range") == "" && acceptsGzip(r.Header.Get("Accept-Encoding")) {
		w.Header().Set("Content-Encoding", "gzip")
		w.Header().Set("ETag", `"`+asset.record.SHA256+`-gzip"`)
		http.ServeContent(w, r, asset.record.File, time.Time{}, io.NewSectionReader(asset.compressed, 0, asset.compressedSize))
		return
	}
	http.ServeContent(w, r, asset.record.File, time.Time{}, io.NewSectionReader(asset.file, 0, asset.record.Size))
}

func acceptsGzip(value string) bool {
	for _, part := range strings.Split(value, ",") {
		fields := strings.Split(strings.TrimSpace(part), ";")
		if strings.TrimSpace(fields[0]) != "gzip" {
			continue
		}
		quality := 1.0
		for _, field := range fields[1:] {
			if raw, ok := strings.CutPrefix(strings.TrimSpace(field), "q="); ok {
				parsed, err := strconv.ParseFloat(raw, 64)
				if err != nil {
					return false
				}
				quality = parsed
			}
		}
		return quality > 0 && quality <= 1
	}
	return false
}
