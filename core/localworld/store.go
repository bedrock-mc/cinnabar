package localworld

import (
	"crypto/rand"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"sync"
	"time"
)

const metaFile = "world.json"

var idPattern = regexp.MustCompile(`^[0-9a-f]{16}$`)

// Store keeps one directory per world under a per-user root: world.json beside the server's data folders.
type Store struct {
	root           string
	now            func() time.Time
	mu             sync.Mutex
	defaultBackend string
}

// SetDefaultBackend sets the backend of worlds created without an explicit one.
func (store *Store) SetDefaultBackend(backend string) {
	store.mu.Lock()
	store.defaultBackend = backend
	store.mu.Unlock()
}

// DefaultBackend is the backend of worlds created without an explicit one.
func (store *Store) DefaultBackend() string {
	store.mu.Lock()
	defer store.mu.Unlock()
	return store.defaultBackendLocked()
}

func (store *Store) defaultBackendLocked() string {
	if store.defaultBackend == "" {
		return BackendDragonfly
	}
	return store.defaultBackend
}

// OpenStore creates root if needed.
func OpenStore(root string) (*Store, error) {
	if root == "" {
		return nil, errors.New("localworld: worlds directory is required")
	}
	if err := os.MkdirAll(root, 0o700); err != nil {
		return nil, fmt.Errorf("localworld: create worlds directory: %w", err)
	}
	return &Store{root: root, now: time.Now}, nil
}

// Dir returns the data directory of a world id, or ErrNotFound for a malformed id.
func (store *Store) Dir(id string) (string, error) {
	if !idPattern.MatchString(id) {
		return "", ErrNotFound
	}
	return filepath.Join(store.root, id), nil
}

func (store *Store) read(id string) (World, error) {
	dir, err := store.Dir(id)
	if err != nil {
		return World{}, err
	}
	raw, err := os.ReadFile(filepath.Join(dir, metaFile))
	if errors.Is(err, os.ErrNotExist) {
		return World{}, ErrNotFound
	}
	if err != nil {
		return World{}, fmt.Errorf("localworld: read world metadata: %w", err)
	}
	var metadata struct {
		World
		Cheats *bool `json:"allow_cheats"`
	}
	if err := json.Unmarshal(raw, &metadata); err != nil || metadata.ID != id {
		return World{}, fmt.Errorf("localworld: corrupt world metadata for %s", id)
	}
	world := metadata.World
	if world.Backend == "" {
		world.Backend = BackendDragonfly // worlds saved before backends existed
	}
	if world.Generator == "" {
		world.Generator = GeneratorFlat // the original local server created flat worlds
		if world.Backend == BackendBDS {
			world.Generator = GeneratorNormal
		}
	}
	world.AllowCheats = world.Backend == BackendDragonfly
	if metadata.Cheats != nil {
		world.AllowCheats = *metadata.Cheats
	}
	return world, nil
}

func (store *Store) write(world World) error {
	dir, err := store.Dir(world.ID)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return fmt.Errorf("localworld: create world directory: %w", err)
	}
	world.SizeBytes = 0
	raw, err := json.MarshalIndent(world, "", "  ")
	if err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, metaFile+".*.tmp")
	if err != nil {
		return fmt.Errorf("localworld: write world metadata: %w", err)
	}
	_, writeErr := tmp.Write(raw)
	closeErr := tmp.Close()
	if err := errors.Join(writeErr, closeErr); err != nil {
		_ = os.Remove(tmp.Name())
		return fmt.Errorf("localworld: write world metadata: %w", err)
	}
	if err := os.Rename(tmp.Name(), filepath.Join(dir, metaFile)); err != nil {
		_ = os.Remove(tmp.Name())
		return fmt.Errorf("localworld: write world metadata: %w", err)
	}
	return nil
}

func randomBytes(n int) ([]byte, error) {
	buf := make([]byte, n)
	if _, err := rand.Read(buf); err != nil {
		return nil, err
	}
	return buf, nil
}

// Create validates spec and persists a new world.
func (store *Store) Create(spec Spec) (World, error) {
	spec, err := spec.normalize()
	if err != nil {
		return World{}, err
	}
	id, err := randomBytes(8)
	if err != nil {
		return World{}, err
	}
	var seed int64
	if spec.Seed != nil {
		seed = *spec.Seed
	} else {
		raw, err := randomBytes(8)
		if err != nil {
			return World{}, err
		}
		seed = int64(binary.BigEndian.Uint64(raw))
	}
	now := store.now().Unix()
	store.mu.Lock()
	defer store.mu.Unlock()
	if spec.Backend == "" {
		spec.Backend = store.defaultBackendLocked()
	}
	world := World{
		ID: hex.EncodeToString(id), Name: spec.Name, GameMode: spec.GameMode, Generator: spec.Generator,
		Difficulty: spec.Difficulty, AllowCheats: spec.AllowCheats, Backend: spec.Backend, Seed: seed, CreatedUnix: now, LastPlayedUnix: now,
	}
	if err := store.write(world); err != nil {
		return World{}, err
	}
	return world, nil
}

// Get returns one world's metadata.
func (store *Store) Get(id string) (World, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	return store.read(id)
}

// List returns every readable world, most recently played first; unreadable directories are skipped.
func (store *Store) List() ([]World, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	entries, err := os.ReadDir(store.root)
	if err != nil {
		return nil, fmt.Errorf("localworld: list worlds: %w", err)
	}
	worlds := make([]World, 0, len(entries))
	for _, entry := range entries {
		if !entry.IsDir() || !idPattern.MatchString(entry.Name()) {
			continue
		}
		if world, err := store.read(entry.Name()); err == nil {
			world.SizeBytes = dirSize(filepath.Join(store.root, entry.Name()))
			worlds = append(worlds, world)
		}
	}
	sort.SliceStable(worlds, func(i, j int) bool {
		if worlds[i].LastPlayedUnix != worlds[j].LastPlayedUnix {
			return worlds[i].LastPlayedUnix > worlds[j].LastPlayedUnix
		}
		return worlds[i].ID < worlds[j].ID
	})
	return worlds, nil
}

// dirSize totals the regular files under dir; unreadable entries count as empty.
func dirSize(dir string) int64 {
	var total int64
	_ = filepath.WalkDir(dir, func(_ string, entry os.DirEntry, err error) error {
		if err == nil && entry.Type().IsRegular() {
			if info, err := entry.Info(); err == nil {
				total += info.Size()
			}
		}
		return nil
	})
	return total
}

// Update validates and applies a settings change; the new game mode and difficulty take effect on the next open.
func (store *Store) Update(id string, update Update) (World, error) {
	store.mu.Lock()
	defer store.mu.Unlock()
	world, err := store.read(id)
	if err != nil {
		return World{}, err
	}
	if update.Name != nil {
		if world.Name, err = ValidateName(*update.Name); err != nil {
			return World{}, err
		}
	}
	if update.GameMode != nil {
		if world.GameMode, err = oneOf(*update.GameMode, world.GameMode, GameModeSurvival, GameModeCreative, GameModeAdventure); err != nil {
			return World{}, err
		}
	}
	if update.Difficulty != nil {
		if world.Difficulty, err = oneOf(*update.Difficulty, world.Difficulty, DifficultyPeaceful, DifficultyEasy, DifficultyNormal, DifficultyHard); err != nil {
			return World{}, err
		}
	}
	return world, store.write(world)
}

// Touch records that a world was just played.
func (store *Store) Touch(id string) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	world, err := store.read(id)
	if err != nil {
		return err
	}
	world.LastPlayedUnix = store.now().Unix()
	return store.write(world)
}

// Delete removes a world and all of its saved data.
func (store *Store) Delete(id string) error {
	store.mu.Lock()
	defer store.mu.Unlock()
	dir, err := store.Dir(id)
	if err != nil {
		return err
	}
	if _, err := os.Stat(dir); err != nil {
		return ErrNotFound
	}
	if err := os.RemoveAll(dir); err != nil {
		return fmt.Errorf("localworld: delete world: %w", err)
	}
	return nil
}
