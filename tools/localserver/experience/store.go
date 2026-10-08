package experience

import (
	"cmp"
	"context"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"path/filepath"
	"regexp"
	"slices"
	"strings"
	"sync"
	"time"
)

// storeSchema is the schema number of every file the store writes and accepts.
const storeSchema = 1

// installedFile records the ids of every Experience ever installed in the world. Its leading
// underscore keeps it apart from every Experience data file.
const installedFile = "_installed.json"

// dataFileName matches the file of one Experience's data: its id followed by ".json".
var dataFileName = regexp.MustCompile(`^[a-z][a-z0-9_]{0,31}\.json$`)

// ErrQuota is returned by SetData when the write would exceed the Experience's data quota.
var ErrQuota = errors.New("experience data quota exceeded")

// ErrNotPlaced is returned by SetData when the position has no entry for the Experience.
var ErrNotPlaced = errors.New("no placed experience block at position")

// Key identifies a block position. Dim is 0 for the overworld, 1 for the nether and 2 for the end.
type Key struct {
	Dim     int
	X, Y, Z int32
}

// Token identifies one state of an entry: a placement generation and a data revision within it.
type Token struct{ Generation, Revision uint64 }

type entry struct {
	token Token
	// data is nil when absent. A stored slice is never mutated, so Flush may encode it unlocked.
	data []byte
}

type experienceData struct {
	entries map[Key]entry
	used    uint64
	dirty   bool
}

// Store is the Go-owned private block data of every Experience, persisted as one JSON file per
// Experience in its directory.
type Store struct {
	dir string

	mu              sync.Mutex
	experiences     map[string]*experienceData
	nextGeneration  uint64
	installed       []string
	installedBlocks map[string][]string

	// flushMu serialises Flush calls so two never write the same temp file.
	flushMu sync.Mutex
}

type fileEntry struct {
	Dim        int     `json:"dim"`
	X          int32   `json:"x"`
	Y          int32   `json:"y"`
	Z          int32   `json:"z"`
	Generation uint64  `json:"generation"`
	Revision   uint64  `json:"revision"`
	Data       *string `json:"data"`
}

type experienceFile struct {
	Schema         int         `json:"schema"`
	NextGeneration uint64      `json:"next_generation"`
	Entries        []fileEntry `json:"entries"`
}

// OpenStore loads every <id>.json file and _installed.json in dir (normally
// <world dir>/experience-data), creating dir if needed and deleting temp files left by an
// interrupted flush. A corrupt or unexpected file fails the open, naming the file.
func OpenStore(dir string) (*Store, error) {
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return nil, fmt.Errorf("create experience data dir: %w", err)
	}
	names, err := os.ReadDir(dir)
	if err != nil {
		return nil, fmt.Errorf("read experience data dir: %w", err)
	}
	s := &Store{dir: dir, experiences: map[string]*experienceData{}, nextGeneration: 1}
	for _, d := range names {
		name := d.Name()
		path := filepath.Join(dir, name)
		switch {
		case strings.HasSuffix(name, ".tmp"):
			if err := os.Remove(path); err != nil {
				return nil, fmt.Errorf("remove stray temp file %s: %w", path, err)
			}
		case name == installedFile:
			if err := s.loadInstalled(path); err != nil {
				return nil, fmt.Errorf("load %s: %w", path, err)
			}
		case dataFileName.MatchString(name):
			if err := s.loadExperience(strings.TrimSuffix(name, ".json"), path); err != nil {
				return nil, fmt.Errorf("load %s: %w", path, err)
			}
		default:
			return nil, fmt.Errorf("unexpected file %s in experience data dir", path)
		}
	}
	return s, nil
}

func (s *Store) loadExperience(id, path string) error {
	raw, err := os.ReadFile(path)
	if err != nil {
		return err
	}
	var f experienceFile
	if err := json.Unmarshal(raw, &f); err != nil {
		return err
	}
	if f.Schema != storeSchema {
		return fmt.Errorf("schema %d, want %d", f.Schema, storeSchema)
	}
	e := &experienceData{entries: make(map[Key]entry, len(f.Entries))}
	s.nextGeneration = max(s.nextGeneration, f.NextGeneration)
	for _, fe := range f.Entries {
		k := Key{Dim: fe.Dim, X: fe.X, Y: fe.Y, Z: fe.Z}
		if _, dup := e.entries[k]; dup {
			return fmt.Errorf("duplicate entry at %+v", k)
		}
		var data []byte
		if fe.Data != nil {
			if data, err = hex.DecodeString(*fe.Data); err != nil {
				return fmt.Errorf("entry at %+v: %w", k, err)
			}
		}
		e.entries[k] = entry{token: Token{Generation: fe.Generation, Revision: fe.Revision}, data: data}
		e.used += uint64(len(data))
		s.nextGeneration = max(s.nextGeneration, fe.Generation+1)
	}
	s.experiences[id] = e
	return nil
}

// sortedUnique returns a sorted copy of ids without duplicates.
func sortedUnique(ids []string) []string {
	return slices.Compact(slices.Sorted(slices.Values(ids)))
}

// experience returns the Experience's data, creating it when create is set. Callers hold s.mu.
func (s *Store) experience(exp string, create bool) *experienceData {
	e := s.experiences[exp]
	if e == nil && create {
		e = &experienceData{entries: map[Key]entry{}}
		s.experiences[exp] = e
	}
	return e
}

// Place records a placement or replacement at k: a new generation with revision 0 and no data.
func (s *Store) Place(exp string, k Key) Token {
	s.mu.Lock()
	defer s.mu.Unlock()
	e := s.experience(exp, true)
	tok := Token{Generation: s.nextGeneration}
	s.nextGeneration++
	e.used -= uint64(len(e.entries[k].data))
	e.entries[k] = entry{token: tok}
	e.dirty = true
	return tok
}

// Remove deletes the entry at k and returns its data, with had false when no data was present.
func (s *Store) Remove(exp string, k Key) (prev []byte, had bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	e := s.experience(exp, false)
	if e == nil {
		return nil, false
	}
	old, ok := e.entries[k]
	if !ok {
		return nil, false
	}
	delete(e.entries, k)
	e.used -= uint64(len(old.data))
	e.dirty = true
	return slices.Clone(old.data), old.data != nil
}

// Token returns the entry's current token, or false when there is no entry at k.
func (s *Store) Token(exp string, k Key) (Token, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if e := s.experience(exp, false); e != nil {
		if en, ok := e.entries[k]; ok {
			return en.token, true
		}
	}
	return Token{}, false
}

// Data returns a copy of the entry's data: (nil, false) when absent, ([]byte{}, true) when empty.
func (s *Store) Data(exp string, k Key) ([]byte, bool) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if e := s.experience(exp, false); e != nil {
		if en, ok := e.entries[k]; ok && en.data != nil {
			return slices.Clone(en.data), true
		}
	}
	return nil, false
}

// SetData replaces the data of the entry at k (absent when present is false) and advances its
// revision. It returns ErrNotPlaced when there is no entry, and ErrQuota when the Experience's
// total data would exceed dataQuota; on error nothing changes.
func (s *Store) SetData(exp string, k Key, data []byte, present bool) error {
	s.mu.Lock()
	defer s.mu.Unlock()
	e := s.experience(exp, false)
	if e == nil {
		return ErrNotPlaced
	}
	en, ok := e.entries[k]
	if !ok {
		return ErrNotPlaced
	}
	var stored []byte
	if present {
		stored = append(make([]byte, 0, len(data)), data...)
	}
	used := e.used - uint64(len(en.data)) + uint64(len(stored))
	if used > dataQuota {
		return ErrQuota
	}
	e.used = used
	e.entries[k] = entry{token: Token{Generation: en.token.Generation, Revision: en.token.Revision + 1}, data: stored}
	e.dirty = true
	return nil
}

// Budget returns how many more data bytes the Experience may store.
func (s *Store) Budget(exp string) uint64 {
	s.mu.Lock()
	defer s.mu.Unlock()
	var used uint64
	if e := s.experience(exp, false); e != nil {
		used = e.used
	}
	if used >= dataQuota {
		return 0
	}
	return dataQuota - used
}

// Flush atomically rewrites the file of every Experience changed since its last flush. A failed
// Experience stays dirty so a later flush retries it.
func (s *Store) Flush() error {
	s.flushMu.Lock()
	defer s.flushMu.Unlock()

	type pending struct {
		id      string
		next    uint64
		keys    []Key
		entries []entry
	}
	var work []pending
	s.mu.Lock()
	for id, e := range s.experiences {
		if !e.dirty {
			continue
		}
		e.dirty = false
		p := pending{id: id, next: s.nextGeneration, keys: make([]Key, 0, len(e.entries)), entries: make([]entry, 0, len(e.entries))}
		for k, en := range e.entries {
			p.keys = append(p.keys, k)
			p.entries = append(p.entries, en)
		}
		work = append(work, p)
	}
	s.mu.Unlock()

	// Stored data slices are never mutated, so they are encoded outside the lock.
	var errs []error
	for _, p := range work {
		f := experienceFile{Schema: storeSchema, NextGeneration: p.next, Entries: make([]fileEntry, len(p.keys))}
		for i, k := range p.keys {
			en := p.entries[i]
			fe := fileEntry{Dim: k.Dim, X: k.X, Y: k.Y, Z: k.Z, Generation: en.token.Generation, Revision: en.token.Revision}
			if en.data != nil {
				h := hex.EncodeToString(en.data)
				fe.Data = &h
			}
			f.Entries[i] = fe
		}
		slices.SortFunc(f.Entries, func(a, b fileEntry) int {
			return cmp.Or(cmp.Compare(a.Dim, b.Dim), cmp.Compare(a.X, b.X), cmp.Compare(a.Y, b.Y), cmp.Compare(a.Z, b.Z))
		})
		if err := s.writeJSON(p.id+".json", f); err != nil {
			errs = append(errs, err)
			s.mu.Lock()
			s.experiences[p.id].dirty = true
			s.mu.Unlock()
		}
	}
	return errors.Join(errs...)
}

// writeJSON writes v to name in the store directory through a synced temp file and a rename, so
// the file always holds either its old or its new content.
func (s *Store) writeJSON(name string, v any) error {
	raw, err := json.Marshal(v)
	if err != nil {
		return fmt.Errorf("encode %s: %w", name, err)
	}
	path := filepath.Join(s.dir, name)
	tmp := path + ".tmp"
	f, err := os.OpenFile(tmp, os.O_WRONLY|os.O_CREATE|os.O_TRUNC, 0o644)
	if err != nil {
		return fmt.Errorf("write %s: %w", path, err)
	}
	_, err = f.Write(raw)
	if err == nil {
		err = f.Sync()
	}
	if cerr := f.Close(); err == nil {
		err = cerr
	}
	if err == nil {
		err = os.Rename(tmp, path)
	}
	if err != nil {
		_ = os.Remove(tmp)
		return fmt.Errorf("write %s: %w", path, err)
	}
	return nil
}

// RunFlusher flushes dirty Experiences every flushInterval until ctx is done. Shutdown flushes
// separately with Flush.
func (s *Store) RunFlusher(ctx context.Context) {
	ticker := time.NewTicker(flushInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ctx.Done():
			return
		case <-ticker.C:
			if err := s.Flush(); err != nil {
				slog.Error("experience data flush failed", "err", err)
			}
		}
	}
}
