package replay

import (
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"sync"
	"syscall"

	"github.com/klauspost/compress/zstd"
)

var validID = regexp.MustCompile(`^[a-zA-Z0-9_-]{1,160}$`)
var validHash = regexp.MustCompile(`^[a-f0-9]{64}$`)

type Store struct {
	lockFile     *os.File
	mu           sync.Mutex
	directory    string
	limit, used  int64
	buffered     int
	completed    map[string]Manifest
	active       map[string]*Recording
	assets       map[string]int64
	pending      map[string]int
	encoder      *zstd.Encoder
	decoder      *zstd.Decoder
	assetDecoder *zstd.Decoder
}

func Open(config Config) (*Store, error) {
	if config.Directory == "" {
		return nil, fmt.Errorf("replay directory is required")
	}
	if config.MaxBytes == 0 {
		config.MaxBytes = DefaultMaxBytes
	}
	if config.MaxBytes < 1 {
		return nil, fmt.Errorf("invalid replay quota")
	}
	s := &Store{directory: config.Directory, limit: config.MaxBytes, completed: map[string]Manifest{}, active: map[string]*Recording{}, assets: map[string]int64{}, pending: map[string]int{}}
	var err error
	if err = os.MkdirAll(config.Directory, 0700); err != nil {
		return nil, err
	}
	s.lockFile, err = os.OpenFile(filepath.Join(config.Directory, ".lock"), os.O_CREATE|os.O_RDWR, 0600)
	if err != nil {
		return nil, err
	}
	if err = syscall.Flock(int(s.lockFile.Fd()), syscall.LOCK_EX|syscall.LOCK_NB); err != nil {
		s.lockFile.Close()
		return nil, fmt.Errorf("replay directory already in use: %w", err)
	}
	s.encoder, err = zstd.NewWriter(nil, zstd.WithEncoderConcurrency(1), zstd.WithEncoderLevel(zstd.SpeedFastest), zstd.WithWindowSize(1<<20))
	if err != nil {
		s.Close()
		return nil, err
	}
	s.decoder, err = zstd.NewReader(nil, zstd.WithDecoderConcurrency(1), zstd.WithDecoderMaxMemory(16<<20), zstd.WithDecodeAllCapLimit(true))
	if err != nil {
		s.Close()
		return nil, err
	}
	s.assetDecoder, err = zstd.NewReader(nil, zstd.WithDecoderConcurrency(1), zstd.WithDecoderMaxMemory(128<<20), zstd.WithDecodeAllCapLimit(true))
	if err != nil {
		s.Close()
		return nil, err
	}
	if err = s.recover(); err != nil {
		s.Close()
		return nil, err
	}
	return s, nil
}

// Close requires callers to stop appending first. Unfinished recordings are
// discarded; published recordings remain durable.
func (s *Store) Close() {
	s.mu.Lock()
	defer s.mu.Unlock()
	for _, recording := range s.active {
		s.abort(recording)
	}
	if s.encoder != nil {
		s.encoder.Close()
	}
	if s.decoder != nil {
		s.decoder.Close()
	}
	if s.assetDecoder != nil {
		s.assetDecoder.Close()
	}
	if s.lockFile != nil {
		_ = syscall.Flock(int(s.lockFile.Fd()), syscall.LOCK_UN)
		_ = s.lockFile.Close()
		s.lockFile = nil
	}
}

// PutAsset stores immutable arena bytes or skin PNGs. Kind is descriptive;
// identity depends only on content so identical content is stored once.
func (s *Store) PutAsset(kind string, data []byte) (string, error) {
	if len(data) > maxAssetBytes {
		return "", fmt.Errorf("replay asset exceeds size limit")
	}
	hashBytes := sha256.Sum256(data)
	hash := hex.EncodeToString(hashBytes[:])
	s.mu.Lock()
	defer s.mu.Unlock()
	if _, exists := s.assets[hash]; exists {
		s.pending[hash]++
		return hash, nil
	}
	encoded := s.encodeAsset(kind, data)
	if err := s.makeRoom(int64(len(encoded)), map[string]bool{hash: true}); err != nil {
		return "", err
	}
	if err := s.write(s.assetPath(hash), encoded); err != nil {
		return "", err
	}
	s.assets[hash] = int64(len(encoded))
	s.pending[hash]++
	return hash, nil
}

func (s *Store) Begin(metadata Metadata, assets []string) (*Recording, error) {
	if len(metadata.Detail) > maxManifestBytes/4 || !validID.MatchString(metadata.ID) || !json.Valid(metadata.Detail) && len(metadata.Detail) > 0 {
		return nil, fmt.Errorf("invalid replay metadata")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if len(s.active) >= maxActiveRecordings {
		return nil, fmt.Errorf("too many active recordings")
	}
	if _, ok := s.completed[metadata.ID]; ok {
		return nil, fmt.Errorf("replay ID already exists")
	}
	if _, ok := s.active[metadata.ID]; ok {
		return nil, fmt.Errorf("replay ID already active")
	}
	seen := map[string]bool{}
	for _, hash := range assets {
		if !validHash.MatchString(hash) {
			return nil, fmt.Errorf("invalid asset hash")
		}
		if _, exists := s.assets[hash]; !exists {
			return nil, fmt.Errorf("asset %s: %w", hash, ErrNotFound)
		}
		seen[hash] = true
	}
	metadata.Detail = append(json.RawMessage(nil), metadata.Detail...)
	r := &Recording{store: s, manifest: Manifest{Version: FormatVersion, Metadata: metadata, Assets: make([]string, 0, len(seen))}, lastMS: -1}
	for hash := range seen {
		r.manifest.Assets = append(r.manifest.Assets, hash)
	}
	sort.Strings(r.manifest.Assets)
	if err := os.Mkdir(s.recordingPath(metadata.ID, true), 0700); err != nil {
		return nil, err
	}
	s.active[metadata.ID] = r
	for _, hash := range assets {
		if s.pending[hash] > 0 {
			s.pending[hash]--
		}
	}
	return r, nil
}

func (s *Store) Stats() (used, limit int64) { s.mu.Lock(); defer s.mu.Unlock(); return s.used, s.limit }

func (s *Store) List() []Manifest {
	s.mu.Lock()
	defer s.mu.Unlock()
	result := make([]Manifest, 0, len(s.completed))
	for _, manifest := range s.completed {
		result = append(result, cloneManifest(manifest))
	}
	sort.Slice(result, func(i, j int) bool { return result[i].Metadata.StartedAt.After(result[j].Metadata.StartedAt) })
	return result
}

func (s *Store) Get(id string) (Manifest, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	m, ok := s.completed[id]
	if !ok {
		return Manifest{}, ErrNotFound
	}
	return cloneManifest(m), nil
}

func cloneManifest(m Manifest) Manifest {
	m.Metadata.Detail = append(json.RawMessage(nil), m.Metadata.Detail...)
	m.Assets = append([]string(nil), m.Assets...)
	m.Chunks = append([]Chunk(nil), m.Chunks...)
	return m
}

func (s *Store) assetPath(hash string) string { return filepath.Join(s.directory, "assets", hash) }
func (s *Store) recordingPath(id string, active bool) string {
	folder := "complete"
	if active {
		folder = "active"
	}
	return filepath.Join(s.directory, folder, id)
}
func chunkName(index int) string { return fmt.Sprintf("%08d.zst", index) }

// write holds the repository lock; quota includes temporary data exactly once.
// Files are flushed before rename, and publication flushes its parent directory.
func (s *Store) write(path string, data []byte) error {
	temp := path + ".tmp"
	f, err := os.OpenFile(temp, os.O_CREATE|os.O_EXCL|os.O_WRONLY, 0600)
	if err != nil {
		return err
	}
	written, err := f.Write(data)
	if err == nil && written != len(data) {
		err = fmt.Errorf("short replay write")
	}
	if err == nil {
		err = f.Sync()
	}
	closeErr := f.Close()
	if err == nil {
		err = closeErr
	}
	if err == nil {
		err = os.Rename(temp, path)
	}
	if err != nil {
		_ = os.Remove(temp)
		return err
	}
	s.used += int64(len(data))
	return nil
}

// ReleaseAsset releases one PutAsset reservation after an abandoned setup.
func (s *Store) ReleaseAsset(hash string) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.pending[hash] > 0 {
		s.pending[hash]--
	}
}
