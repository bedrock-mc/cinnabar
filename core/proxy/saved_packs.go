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

// prependSavedPacks loads active saved resource packs from the user's global catalog
// and prepends them to the session handoff and pack list so they have the HIGHEST priority
// over all server packs.
func prependSavedPacks(selected []sessionPack, packs []*resource.Pack, logger *slog.Logger) ([]sessionPack, []*resource.Pack) {
	if strings.HasSuffix(os.Args[0], ".test") || strings.HasSuffix(os.Args[0], ".test.exe") {
		return selected, packs
	}
	localAppData := os.Getenv("LOCALAPPDATA")
	if localAppData == "" {
		return selected, packs
	}
	globalDir := filepath.Join(localAppData, "Cinnabar", "resource-packs", "global")
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
		// Find matching mcpack in globalDir
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
