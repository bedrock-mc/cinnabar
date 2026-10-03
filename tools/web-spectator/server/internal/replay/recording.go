package replay

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"time"
)

type Recording struct {
	store           *Store
	manifest        Manifest
	buffer          bytes.Buffer
	frames          int
	startMS, lastMS int64
	bytes           int64
	closed          bool
}

// Append copies one complete frame. A monotonic timestamp and bounded frame
// size prevent malformed input from causing an unbounded recording buffer.
// Compression and IO happen here; invoke it on a worker, outside game threads.
func (r *Recording) Append(timeMS int64, payload json.RawMessage) error {
	if timeMS < 0 || !json.Valid(payload) {
		return fmt.Errorf("invalid replay frame")
	}
	if len(payload) > maxChunkBytes/2 {
		return fmt.Errorf("replay frame exceeds size limit")
	}
	frame, err := json.Marshal(Frame{TimeMS: timeMS, Payload: payload})
	if err != nil {
		return err
	}
	s := r.store
	s.mu.Lock()
	defer s.mu.Unlock()
	if r.closed {
		return ErrClosed
	}
	if timeMS < r.lastMS {
		return fmt.Errorf("replay timestamps must be monotonic")
	}
	if r.frames > 0 && (timeMS-r.startMS >= 1000 || r.buffer.Len()+len(frame)+1 > maxChunkBytes) {
		if err := r.flush(); err != nil {
			s.abort(r)
			return err
		}
	}
	// Pressure flushes another pending chunk rather than letting independent
	// active matches multiply their buffers beyond the global memory ceiling.
	if s.buffered+len(frame)+1 > maxBufferedBytes {
		for _, other := range s.active {
			if other.frames == 0 {
				continue
			}
			if err := other.flush(); err != nil {
				s.abort(other)
				if other == r {
					return err
				}
			}
			if s.buffered+len(frame)+1 <= maxBufferedBytes {
				break
			}
		}
	}
	if r.closed {
		return ErrClosed
	}
	if r.frames == 0 {
		r.startMS = timeMS
	}
	s.buffered += len(frame) + 1
	r.buffer.Write(frame)
	r.buffer.WriteByte('\n')
	r.frames++
	r.lastMS = timeMS
	return nil
}

func (r *Recording) Flush() error {
	s := r.store
	s.mu.Lock()
	defer s.mu.Unlock()
	if r.closed {
		return ErrClosed
	}
	if err := r.flush(); err != nil {
		s.abort(r)
		return err
	}
	return nil
}

func (r *Recording) flush() error {
	if r.frames == 0 {
		return nil
	}
	if len(r.manifest.Chunks) >= 8192 {
		return fmt.Errorf("replay index exceeds size limit")
	}
	// A single shared encoder uses one compression worker and limits peak memory.
	compressed := r.store.encoder.EncodeAll(r.buffer.Bytes(), nil)
	if err := r.store.makeRoom(int64(len(compressed)), nil); err != nil {
		return err
	}
	chunk := Chunk{Index: len(r.manifest.Chunks), StartMS: r.startMS, EndMS: r.lastMS, Frames: r.frames, Bytes: int64(len(compressed))}
	path := filepath.Join(r.store.recordingPath(r.manifest.Metadata.ID, true), chunkName(chunk.Index))
	if err := r.store.write(path, compressed); err != nil {
		return err
	}
	r.bytes += chunk.Bytes
	r.manifest.Chunks = append(r.manifest.Chunks, chunk)
	r.store.buffered -= r.buffer.Len()
	r.buffer = bytes.Buffer{}
	r.frames = 0
	return nil
}

// Finish publishes the manifest and directory atomically. Only fully published
// recordings appear in List. A failure discards this recording cleanly.
func (r *Recording) Finish() (Manifest, error) {
	s := r.store
	s.mu.Lock()
	defer s.mu.Unlock()
	if r.closed {
		return Manifest{}, ErrClosed
	}
	if err := r.flush(); err != nil {
		s.abort(r)
		return Manifest{}, err
	}
	if len(r.manifest.Chunks) == 0 {
		s.abort(r)
		return Manifest{}, fmt.Errorf("empty replay")
	}
	r.manifest.FinishedAt = time.Now().UTC()
	r.manifest.DurationMS = r.lastMS
	r.manifest.FileBytes = r.bytes
	data, err := json.Marshal(r.manifest)
	// Include the manifest itself; decimal width stabilises within a few passes.
	for i := 0; err == nil && i < 4; i++ {
		r.manifest.FileBytes = r.bytes + int64(len(data))
		data, err = json.Marshal(r.manifest)
	}
	if err == nil && len(data) > maxManifestBytes {
		err = fmt.Errorf("replay index exceeds size limit")
	}
	if err == nil {
		err = s.makeRoom(int64(len(data)), nil)
	}
	if err == nil {
		err = s.write(filepath.Join(s.recordingPath(r.manifest.Metadata.ID, true), "manifest.json"), data)
		if err == nil {
			r.bytes += int64(len(data))
		}
	}
	if err == nil {
		err = syncDirectory(s.recordingPath(r.manifest.Metadata.ID, true))
	}
	if err == nil {
		err = os.Rename(s.recordingPath(r.manifest.Metadata.ID, true), s.recordingPath(r.manifest.Metadata.ID, false))
	}
	if err == nil {
		err = syncDirectory(filepath.Join(s.directory, "complete"))
	}
	if err != nil {
		// A rename may have succeeded before the parent fsync failed. Remove both
		// possible locations so a failed publication never becomes visible later.
		_ = os.RemoveAll(s.recordingPath(r.manifest.Metadata.ID, false))
		s.abort(r)
		return Manifest{}, err
	}
	r.closed = true
	delete(s.active, r.manifest.Metadata.ID)
	s.completed[r.manifest.Metadata.ID] = cloneManifest(r.manifest)
	return cloneManifest(r.manifest), nil
}

func (r *Recording) Abort() { s := r.store; s.mu.Lock(); defer s.mu.Unlock(); s.abort(r) }

func (s *Store) abort(r *Recording) {
	if r.closed {
		return
	}
	if err := os.RemoveAll(s.recordingPath(r.manifest.Metadata.ID, true)); err == nil {
		s.used -= r.bytes
	}
	r.closed = true
	s.buffered -= r.buffer.Len()
	r.buffer = bytes.Buffer{}
	delete(s.active, r.manifest.Metadata.ID)
}

func syncDirectory(path string) error {
	f, err := os.Open(path)
	if err != nil {
		return err
	}
	defer f.Close()
	return f.Sync()
}

// AddAsset transfers one PutAsset reservation to an active recording, including
// skins first observed after capture started. Repeated references deduplicate.
func (r *Recording) AddAsset(hash string) error {
	s := r.store
	s.mu.Lock()
	defer s.mu.Unlock()
	if r.closed {
		return ErrClosed
	}
	if !validHash.MatchString(hash) {
		return fmt.Errorf("invalid asset hash")
	}
	if _, ok := s.assets[hash]; !ok {
		return ErrNotFound
	}
	if s.pending[hash] > 0 {
		s.pending[hash]--
	}
	for _, existing := range r.manifest.Assets {
		if existing == hash {
			return nil
		}
	}
	r.manifest.Assets = append(r.manifest.Assets, hash)
	return nil
}

// UpdateDetail replaces the small match descriptor before publication. It does
// not change frames, asset references or the recording identity.
func (r *Recording) UpdateDetail(detail json.RawMessage) error {
	if len(detail) > maxManifestBytes/4 || (len(detail) > 0 && !json.Valid(detail)) {
		return fmt.Errorf("invalid replay metadata")
	}
	s := r.store
	s.mu.Lock()
	defer s.mu.Unlock()
	if r.closed {
		return ErrClosed
	}
	r.manifest.Metadata.Detail = append(json.RawMessage(nil), detail...)
	return nil
}
