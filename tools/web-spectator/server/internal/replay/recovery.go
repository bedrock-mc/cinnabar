package replay

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"io"
	"os"
	"path/filepath"
)

// recover discards unpublished data. Completed manifests and referenced files
// are checked before becoming visible. Unknown files cannot silently consume
// unaccounted quota; an unexpected root entry refuses startup.
func (s *Store) recover() error {
	if err := os.MkdirAll(s.directory, 0700); err != nil {
		return err
	}
	root, err := os.ReadDir(s.directory)
	if err != nil {
		return err
	}
	for _, entry := range root {
		if entry.Name() != ".lock" && entry.Name() != "assets" && entry.Name() != "active" && entry.Name() != "complete" {
			return fmt.Errorf("unexpected replay root entry: %s", entry.Name())
		}
	}
	if err = os.RemoveAll(filepath.Join(s.directory, "active")); err != nil {
		return err
	}
	for _, folder := range []string{"active", "complete", "assets"} {
		if err = os.MkdirAll(filepath.Join(s.directory, folder), 0700); err != nil {
			return err
		}
	}
	assets, err := os.ReadDir(filepath.Join(s.directory, "assets"))
	if err != nil {
		return err
	}
	for _, entry := range assets {
		path := filepath.Join(s.directory, "assets", entry.Name())
		if !validHash.MatchString(entry.Name()) {
			if err = os.RemoveAll(path); err != nil {
				return err
			}
			continue
		}
		info, err := entry.Info()
		if err != nil {
			return err
		}
		if !info.Mode().IsRegular() {
			return fmt.Errorf("invalid asset file")
		}
		data, decodeErr := s.readAsset(path)
		hash := sha256.Sum256(data)
		if decodeErr != nil || hex.EncodeToString(hash[:]) != entry.Name() {
			if err = os.Remove(path); err != nil {
				return err
			}
			continue
		}
		s.assets[entry.Name()] = info.Size()
		s.used += info.Size()
	}
	entries, err := os.ReadDir(filepath.Join(s.directory, "complete"))
	if err != nil {
		return err
	}
	for _, entry := range entries {
		path := filepath.Join(s.directory, "complete", entry.Name())
		manifest, err := s.recoverManifest(path, entry.Name())
		if err != nil {
			if removeErr := os.RemoveAll(path); removeErr != nil {
				return removeErr
			}
			continue
		}
		bytes, err := directoryBytes(path)
		if err != nil {
			return err
		}
		s.used += bytes
		s.completed[entry.Name()] = manifest
	}
	if err = s.collectAssets(nil); err != nil {
		return err
	}
	return s.makeRoom(0, nil)
}

func (s *Store) recoverManifest(path, id string) (Manifest, error) {
	var m Manifest
	if !validID.MatchString(id) {
		return m, fmt.Errorf("invalid replay ID")
	}
	f, err := os.Open(filepath.Join(path, "manifest.json"))
	if err != nil {
		return m, err
	}
	data, err := io.ReadAll(io.LimitReader(f, maxManifestBytes+1))
	f.Close()
	if err != nil || len(data) > maxManifestBytes {
		return m, fmt.Errorf("invalid replay index")
	}
	if err = json.Unmarshal(data, &m); err != nil {
		return m, err
	}
	if m.Version != FormatVersion || m.Metadata.ID != id || len(m.Chunks) == 0 {
		return m, fmt.Errorf("invalid replay manifest")
	}
	for _, hash := range m.Assets {
		if _, ok := s.assets[hash]; !ok {
			return m, fmt.Errorf("missing replay asset")
		}
	}
	previous := int64(-1)
	for index, chunk := range m.Chunks {
		if chunk.Index != index || chunk.StartMS < previous || chunk.EndMS < chunk.StartMS || chunk.Frames < 1 || chunk.Bytes < 1 || chunk.Bytes > maxChunkBytes+1024 {
			return m, fmt.Errorf("invalid replay chunk index")
		}
		info, err := os.Lstat(filepath.Join(path, chunkName(index)))
		if err != nil {
			return m, err
		}
		if !info.Mode().IsRegular() || info.Size() != chunk.Bytes {
			return m, fmt.Errorf("invalid replay chunk")
		}
		previous = chunk.EndMS
	}
	files, err := os.ReadDir(path)
	if err != nil {
		return m, err
	}
	if len(files) != len(m.Chunks)+1 {
		return m, fmt.Errorf("unexpected replay files")
	}
	return m, nil
}
