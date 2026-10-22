package proxy

import (
	"encoding/json"
	"log/slog"
	"os"
	"path/filepath"
	"strings"

	"github.com/sandertv/gophertunnel/minecraft/resource"
)

type globalPacksConfig struct {
	Active []struct {
		ID       string `json:"id"`
		SubPack  string `json:"subpack"`
		Revision uint64 `json:"revision"`
	} `json:"active"`
}

// globalPacksDirOverride allows unit tests to point to a temporary test fixture directory.
var globalPacksDirOverride string

func getGlobalPacksDir() string {
	if globalPacksDirOverride != "" {
		return globalPacksDirOverride
	}
	if custom := os.Getenv("CINNABAR_USER_ROOT"); custom != "" {
		return filepath.Join(custom, "resource-packs", "global")
	}
	if localAppData := os.Getenv("LOCALAPPDATA"); localAppData != "" {
		return filepath.Join(localAppData, "Cinnabar", "resource-packs", "global")
	}
	if xdgData := os.Getenv("XDG_DATA_HOME"); xdgData != "" {
		return filepath.Join(xdgData, "cinnabar", "resource-packs", "global")
	}
	if home := os.Getenv("HOME"); home != "" {
		macDir := filepath.Join(home, "Library", "Application Support", "Cinnabar", "resource-packs", "global")
		if _, err := os.Stat(macDir); err == nil {
			return macDir
		}
		return filepath.Join(home, ".local", "share", "cinnabar", "resource-packs", "global")
	}
	return ""
}

// prependSavedPacks loads active saved resource packs from the user's global catalog
// and prepends them to the session handoff and pack list so they have the highest priority
// over all server packs.
func prependSavedPacks(selected []sessionPack, packs []*resource.Pack, logger *slog.Logger) ([]sessionPack, []*resource.Pack) {
	if globalPacksDirOverride == "" && (strings.HasSuffix(os.Args[0], ".test") || strings.HasSuffix(os.Args[0], ".test.exe")) {
		return selected, packs
	}
	globalDir := getGlobalPacksDir()
	if globalDir == "" {
		return selected, packs
	}
	catPath := filepath.Join(globalDir, "global_packs.json")
	data, err := os.ReadFile(catPath)
	if err != nil {
		return selected, packs
	}

	var cat globalPacksConfig
	if err := json.Unmarshal(data, &cat); err != nil {
		return selected, packs
	}

	var savedSelected []sessionPack
	var savedPacks []*resource.Pack
	seenSaved := make(map[string]bool)

	for _, active := range cat.Active {
		if active.ID == "" {
			continue
		}
		pattern := filepath.Join(globalDir, active.ID+"*.mcpack")
		matches, err := filepath.Glob(pattern)
		if err != nil || len(matches) == 0 {
			continue
		}
		packPath := matches[0]
		archiveData, err := os.ReadFile(packPath)
		if err != nil {
			continue
		}
		pack, err := resource.ReadBytes(archiveData)
		if err != nil {
			if logger != nil {
				logger.Warn("failed to parse saved resource pack", "id", active.ID, "error", err)
			}
			continue
		}

		sp := sessionPack{
			UUID:    pack.UUID().String(),
			Version: pack.Version(),
			SubPack: active.SubPack,
			Size:    uint64(len(archiveData)),
		}
		savedSelected = append(savedSelected, sp)
		savedPacks = append(savedPacks, pack)
		seenSaved[strings.ToLower(sp.UUID)] = true
		if logger != nil {
			logger.Info("prioritizing saved resource pack over server packs", "uuid", sp.UUID, "version", sp.Version)
		}
	}

	if len(savedSelected) == 0 {
		return selected, packs
	}

	// Filter out from server selected/packs any entry with a matching UUID
	var filteredSelected []sessionPack
	var filteredPacks []*resource.Pack
	for i, sp := range selected {
		if !seenSaved[strings.ToLower(sp.UUID)] {
			filteredSelected = append(filteredSelected, sp)
			if i < len(packs) {
				filteredPacks = append(filteredPacks, packs[i])
			}
		}
	}

	return append(savedSelected, filteredSelected...), append(savedPacks, filteredPacks...)
}
