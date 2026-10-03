package replay

import (
	"fmt"
	"os"
	"path/filepath"
	"sort"
)

// makeRoom includes every completed and active byte. The caller holds the lock
// until its write completes, so simultaneous writers cannot oversubscribe it.
// Retained asset references and all active recordings are protected.
func (s *Store) makeRoom(extra int64, protect map[string]bool) error {
	if extra < 0 || extra > s.limit {
		return ErrQuota
	}
	// Reject an impossible active recording before evicting any history.
	// Active bytes and their referenced/staged assets cannot be reclaimed.
	protectedAssets := map[string]bool{}
	var activeBytes int64
	for _, recording := range s.active {
		activeBytes += recording.bytes
		for _, hash := range recording.manifest.Assets {
			protectedAssets[hash] = true
		}
	}
	for hash, count := range s.pending {
		if count > 0 {
			protectedAssets[hash] = true
		}
	}
	for hash := range protect {
		protectedAssets[hash] = true
	}
	for hash := range protectedAssets {
		activeBytes += s.assets[hash]
	}
	if activeBytes+extra > s.limit {
		return ErrQuota
	}
	if s.used+extra <= s.limit {
		return nil
	}
	if err := s.collectAssets(protect); err != nil {
		return err
	}
	if s.used+extra <= s.limit {
		return nil
	}
	oldest := make([]Manifest, 0, len(s.completed))
	for _, manifest := range s.completed {
		oldest = append(oldest, manifest)
	}
	sort.Slice(oldest, func(i, j int) bool {
		if oldest[i].Metadata.StartedAt.Equal(oldest[j].Metadata.StartedAt) {
			return oldest[i].Metadata.ID < oldest[j].Metadata.ID
		}
		return oldest[i].Metadata.StartedAt.Before(oldest[j].Metadata.StartedAt)
	})
	for _, manifest := range oldest {
		path := s.recordingPath(manifest.Metadata.ID, false)
		bytes, err := directoryBytes(path)
		if err != nil {
			return err
		}
		if err = os.RemoveAll(path); err != nil {
			return err
		}
		s.used -= bytes
		delete(s.completed, manifest.Metadata.ID)
		if err = s.collectAssets(protect); err != nil {
			return err
		}
		if s.used+extra <= s.limit {
			return nil
		}
	}
	return ErrQuota
}

func (s *Store) collectAssets(protect map[string]bool) error {
	references := map[string]bool{}
	for hash := range protect {
		references[hash] = true
	}
	for hash, count := range s.pending {
		if count > 0 {
			references[hash] = true
		}
	}
	for _, manifest := range s.completed {
		for _, hash := range manifest.Assets {
			references[hash] = true
		}
	}
	for _, recording := range s.active {
		for _, hash := range recording.manifest.Assets {
			references[hash] = true
		}
	}
	for hash, size := range s.assets {
		if references[hash] {
			continue
		}
		if err := os.Remove(s.assetPath(hash)); err != nil && !os.IsNotExist(err) {
			return err
		}
		delete(s.assets, hash)
		s.used -= size
	}
	return nil
}

func directoryBytes(path string) (int64, error) {
	var total int64
	err := filepath.Walk(path, func(_ string, info os.FileInfo, err error) error {
		if err != nil {
			return err
		}
		if info.Mode().IsRegular() {
			total += info.Size()
		} else if !info.IsDir() {
			return fmt.Errorf("unexpected replay file type")
		}
		return nil
	})
	return total, err
}
