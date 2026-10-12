package webrelay

import (
	"encoding/json"
	"io/fs"
	"net/http"
	"os"
	"path/filepath"
	"sort"
)

// DevAssetsPath is where DevAssets serves compiled carriers; index.json lists them.
const DevAssetsPath = "/v1/dev-assets/"

// devAsset is one served carrier, by its path below the compiled-assets directory.
type devAsset struct {
	Path string `json:"path"`
	Size int64  `json:"size"`
}

// maxWarmedDevAsset bounds the carriers compressed ahead of their first request. Every carrier a
// page downloads whole is far smaller; the sound bank, read only by range, is larger.
const maxWarmedDevAsset = 64 << 20

// DevAssets serves a locally compiled carrier directory so a development page needs no import.
// The carriers derive from Mojang's pack, so only a private relay may serve them.
func DevAssets(dir string) http.Handler {
	static := StaticFiles(dir)
	go static.Warm(maxWarmedDevAsset)
	files := http.StripPrefix(DevAssetsPath, static)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path != DevAssetsPath+"index.json" {
			files.ServeHTTP(w, r)
			return
		}
		assets, err := listDevAssets(dir)
		if err != nil {
			http.Error(w, "unavailable", http.StatusServiceUnavailable)
			return
		}
		w.Header().Set("Content-Type", "application/json")
		w.Header().Set("Cache-Control", "no-store")
		_ = json.NewEncoder(w).Encode(map[string]any{"files": assets})
	})
}

// listDevAssets lists the regular files below dir, in path order.
func listDevAssets(dir string) ([]devAsset, error) {
	var assets []devAsset
	err := filepath.WalkDir(dir, func(path string, entry fs.DirEntry, err error) error {
		if err != nil || !entry.Type().IsRegular() {
			return err
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		relative, err := filepath.Rel(dir, path)
		if err != nil {
			return err
		}
		assets = append(assets, devAsset{Path: filepath.ToSlash(relative), Size: info.Size()})
		return nil
	})
	if err == nil && len(assets) == 0 {
		_, err = os.Stat(dir)
	}
	sort.Slice(assets, func(i, j int) bool { return assets[i].Path < assets[j].Path })
	return assets, err
}
