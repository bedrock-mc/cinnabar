package localworld

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
)

const prefsFile = "prefs.json"

// Prefs are per-user local-world preferences kept beside the worlds.
type Prefs struct {
	// DockerPromptDismissed suppresses the "vanilla worlds need Docker" modal.
	DockerPromptDismissed bool   `json:"docker_prompt_dismissed"`
	CreationBackend       string `json:"creation_backend,omitempty"`
	CreationGenerator     string `json:"creation_generator,omitempty"`
}

// PrefsUpdate changes preferences; nil fields are left alone.
type PrefsUpdate struct {
	DockerPromptDismissed *bool
	CreationBackend       *string
	CreationGenerator     *string
	// Redetect re-probes for Docker before returning (the modal's Retry).
	Redetect bool
}

// Prefs returns the saved preferences; a missing or unreadable file yields defaults.
func (store *Store) Prefs() Prefs {
	store.mu.Lock()
	defer store.mu.Unlock()
	return store.readPrefs()
}

func (store *Store) readPrefs() Prefs {
	var prefs Prefs
	if raw, err := os.ReadFile(filepath.Join(store.root, prefsFile)); err == nil {
		_ = json.Unmarshal(raw, &prefs)
	}
	return prefs
}

// UpdatePrefs applies update's fields atomically and returns the result.
func (store *Store) UpdatePrefs(update PrefsUpdate) (Prefs, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	prefs := store.readPrefs()
	if update.CreationBackend != nil {
		backend, err := oneOf(*update.CreationBackend, BackendDragonfly, BackendDragonfly, BackendBDS)
		if err != nil {
			return prefs, err
		}
		prefs.CreationBackend = backend
	}
	if update.CreationGenerator != nil {
		generator, err := oneOf(*update.CreationGenerator, GeneratorNormal, GeneratorNormal, GeneratorFlat)
		if err != nil {
			return prefs, err
		}
		prefs.CreationGenerator = generator
	}
	if update.DockerPromptDismissed != nil {
		prefs.DockerPromptDismissed = *update.DockerPromptDismissed
	}
	raw, err := json.Marshal(prefs)
	if err != nil {
		return prefs, err
	}
	tmp, err := os.CreateTemp(store.root, prefsFile+".*.tmp")
	if err != nil {
		return prefs, fmt.Errorf("localworld: write preferences: %w", err)
	}
	_, writeErr := tmp.Write(raw)
	if err := errors.Join(writeErr, tmp.Close()); err != nil {
		_ = os.Remove(tmp.Name())
		return prefs, fmt.Errorf("localworld: write preferences: %w", err)
	}
	if err := os.Rename(tmp.Name(), filepath.Join(store.root, prefsFile)); err != nil {
		_ = os.Remove(tmp.Name())
		return prefs, fmt.Errorf("localworld: write preferences: %w", err)
	}
	return prefs, nil
}
