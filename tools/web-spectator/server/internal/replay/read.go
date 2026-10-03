package replay

import (
	"bufio"
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
)

func (s *Store) ReadAsset(id, hash string) ([]byte, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	m, ok := s.completed[id]
	if !ok {
		return nil, ErrNotFound
	}
	for _, asset := range m.Assets {
		if asset == hash {
			return s.readAsset(s.assetPath(hash))
		}
	}
	return nil, ErrNotFound
}

// ReadChunk decompresses at most one bounded chunk, never the whole replay.
func (s *Store) ReadChunk(id string, index int) ([]Frame, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	m, ok := s.completed[id]
	if !ok || index < 0 || index >= len(m.Chunks) {
		return nil, ErrNotFound
	}
	compressed, err := os.ReadFile(filepath.Join(s.recordingPath(id, false), chunkName(index)))
	if err != nil {
		return nil, err
	}
	raw, err := s.decoder.DecodeAll(compressed, make([]byte, 0, maxChunkBytes))
	if err != nil {
		return nil, err
	}
	if len(raw) > maxChunkBytes {
		return nil, fmt.Errorf("replay chunk exceeds decode limit")
	}
	scanner := bufio.NewScanner(bytes.NewReader(raw))
	scanner.Buffer(make([]byte, 4096), maxChunkBytes)
	frames := make([]Frame, 0, m.Chunks[index].Frames)
	for scanner.Scan() {
		var frame Frame
		if err = json.Unmarshal(scanner.Bytes(), &frame); err != nil {
			return nil, err
		}
		frames = append(frames, frame)
	}
	if err = scanner.Err(); err != nil {
		return nil, err
	}
	if len(frames) != m.Chunks[index].Frames {
		return nil, fmt.Errorf("replay frame count mismatch")
	}
	return frames, nil
}
