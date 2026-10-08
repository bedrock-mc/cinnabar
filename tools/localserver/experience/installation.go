package experience

import (
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"slices"
)

// installedRecord keeps required Experience and block IDs together in one atomic file.
type installedRecord struct {
	Schema int                 `json:"schema"`
	IDs    []string            `json:"ids"`
	Blocks map[string][]string `json:"blocks"`
}

// readInstalled reads an installation manifest without creating or changing any files.
func readInstalled(path string) (installedRecord, error) {
	raw, err := os.ReadFile(path)
	if err != nil {
		return installedRecord{}, err
	}
	var rec installedRecord
	if err := json.Unmarshal(raw, &rec); err != nil {
		return rec, err
	}
	if rec.Schema != storeSchema {
		return rec, fmt.Errorf("schema %d, want %d", rec.Schema, storeSchema)
	}
	rec.IDs = sortedUnique(rec.IDs)
	return rec, nil
}

// loadInstalled loads the required IDs, preserving absent block lists in legacy manifests.
func (s *Store) loadInstalled(path string) error {
	rec, err := readInstalled(path)
	if err != nil {
		return err
	}
	s.installed, s.installedBlocks = rec.IDs, rec.Blocks
	return nil
}

// CheckDisabled refuses to open an installed world without its Experiences. It never creates files.
func CheckDisabled(dir string) error {
	path := filepath.Join(dir, installedFile)
	rec, err := readInstalled(path)
	if errors.Is(err, os.ErrNotExist) {
		return nil
	}
	if err != nil {
		return fmt.Errorf("load %s: %w", path, err)
	}
	if len(rec.IDs) != 0 {
		return fmt.Errorf("world requires installed experiences %q; supply -experiences and -experience-runtime", rec.IDs)
	}
	return nil
}

// Installed returns the recorded installed Experience IDs in sorted order.
func (s *Store) Installed() []string {
	s.mu.Lock()
	defer s.mu.Unlock()
	return slices.Clone(s.installed)
}

// ValidateInstalled rejects missing Experiences or block definitions before the world opens.
// An old ID-only manifest cannot prove that blocks were retained, so it requires explicit migration.
func (s *Store) ValidateInstalled(loaded []Loaded) error {
	available := make(map[string]map[string]bool, len(loaded))
	for _, l := range loaded {
		blocks := make(map[string]bool, len(l.Blocks))
		for _, b := range l.Blocks {
			blocks[b.ID] = true
		}
		available[l.ID] = blocks
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, id := range s.installed {
		now, ok := available[id]
		if !ok {
			return fmt.Errorf("installed experience %q is missing", id)
		}
		previous, ok := s.installedBlocks[id]
		if !ok {
			return fmt.Errorf("installation manifest %s has no block list for %q; migrate it using the original installed block definitions before opening this world", filepath.Join(s.dir, installedFile), id)
		}
		for _, block := range previous {
			if !now[block] {
				return fmt.Errorf("installed experience %q is missing required block %q", id, block)
			}
		}
	}
	return nil
}

// SetInstalled validates and atomically records all installed Experience and block IDs.
func (s *Store) SetInstalled(loaded []Loaded) error {
	s.flushMu.Lock()
	defer s.flushMu.Unlock()
	if err := s.ValidateInstalled(loaded); err != nil {
		return err
	}
	rec := installedRecord{Schema: storeSchema, Blocks: make(map[string][]string, len(loaded))}
	for _, l := range loaded {
		rec.IDs = append(rec.IDs, l.ID)
		ids := make([]string, 0, len(l.Blocks))
		for _, b := range l.Blocks {
			ids = append(ids, b.ID)
		}
		rec.Blocks[l.ID] = sortedUnique(ids)
	}
	rec.IDs = sortedUnique(rec.IDs)
	if err := s.writeJSON(installedFile, rec); err != nil {
		return err
	}
	s.mu.Lock()
	s.installed, s.installedBlocks = rec.IDs, rec.Blocks
	s.mu.Unlock()
	return nil
}
