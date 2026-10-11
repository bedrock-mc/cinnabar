// Package packcache provides a bounded, persistent cache for verified resource
// pack archives. It deliberately stores only archive bytes: content keys and
// download URLs are never persisted.
package packcache

import (
	"context"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
	"unicode/utf8"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
)

const (
	DefaultQuota    uint64 = 512 << 20
	maxVersionBytes        = 256
	maxIndexEntries        = 100_000
	objectSuffix           = ".mcpack"
	tempPrefix             = ".packcache-"
)

var (
	processMu sync.Mutex
	openRoots = make(map[string]struct{})
	// ErrClosed reports an operation attempted after Close.
	ErrClosed = errors.New("packcache: cache is closed")
	// ErrInUse reports that this process or another process already holds the root lease.
	ErrInUse = errors.New("packcache: cache root is already in use")
)

// Cache is a persistent minecraft.ResourcePackCache. A Cache must be created
// with New; its zero value is not usable. Operations on different keys run
// concurrently; processMu guards only the bookkeeping below.
type Cache struct {
	root     string
	quota    uint64
	pins     map[string]uint32
	keys     map[string]*keyLock // objects with an operation in flight; never evicted
	inflight sync.WaitGroup      // operations Close waits out before releasing the lease
	index    map[string]entry
	used     uint64
	clock    func() time.Time
	lease    io.Closer
	closed   bool
}

// keyLock serializes operations on one object; refs counts holders and waiters.
type keyLock struct {
	sync.Mutex
	refs int
}

type entry struct {
	size     uint64
	used     time.Time
	checksum [32]byte
	verified bool // scanned objects acquire their checksum on the first verified load or store
	invalid  bool // rejected pinned objects stay ineligible until repaired or removed
}

type config struct{ quota uint64 }

// Option configures a Cache.
type Option func(*config) error

// WithQuota sets the maximum number of persisted archive bytes. A zero quota
// disables admission rather than meaning unlimited storage.
func WithQuota(bytes uint64) Option { return func(c *config) error { c.quota = bytes; return nil } }

// New opens or creates a quota-managed cache rooted at root. Callers should pass
// the versioned objects directory (normally .local/cinnabar/resource-packs/v1/objects).
func New(root string, options ...Option) (*Cache, error) {
	cfg := config{quota: DefaultQuota}
	for _, option := range options {
		if option == nil {
			return nil, errors.New("packcache: nil option")
		}
		if err := option(&cfg); err != nil {
			return nil, err
		}
	}
	abs, err := filepath.Abs(root)
	if err != nil || strings.TrimSpace(root) == "" {
		return nil, errors.New("packcache: invalid root")
	}
	processMu.Lock()
	defer processMu.Unlock()
	if err := os.MkdirAll(abs, 0o700); err != nil {
		return nil, fmt.Errorf("packcache: secure root: %w", err)
	}
	canonical, err := filepath.EvalSymlinks(abs)
	if err != nil {
		return nil, fmt.Errorf("packcache: canonical root: %w", err)
	}
	canonical = canonicalPlatformPath(canonical)
	if _, exists := openRoots[canonical]; exists {
		return nil, ErrInUse
	}
	leasePath := filepath.Join(canonical, ".packcache.lock")
	lease, err := lockfile.Acquire(leasePath, 0)
	if err != nil {
		if errors.Is(err, lockfile.ErrBusy) {
			return nil, ErrInUse
		}
		return nil, fmt.Errorf("packcache: acquire root lease: %w", err)
	}
	openRoots[canonical] = struct{}{}
	c := &Cache{root: canonical, quota: cfg.quota, pins: make(map[string]uint32), keys: make(map[string]*keyLock), index: make(map[string]entry), clock: time.Now, lease: lease}
	if err := c.scan(); err != nil {
		delete(openRoots, canonical)
		_ = lease.Close()
		return nil, err
	}
	if err := c.evict(0); err != nil {
		delete(openRoots, canonical)
		_ = lease.Close()
		return nil, err
	}
	return c, nil
}

// Pin prevents the exact offered key from being evicted until the returned
// release function is called. Calls may be nested and release is idempotent.
func (c *Cache) Pin(key minecraft.ResourcePackCacheKey) (func(), error) {
	processMu.Lock()
	defer processMu.Unlock()
	if err := c.checkOpen(); err != nil {
		return nil, err
	}
	name, err := objectName(key)
	if err != nil {
		return nil, err
	}
	c.pins[name]++
	var once sync.Once
	return func() {
		once.Do(func() {
			processMu.Lock()
			if c.pins[name] <= 1 {
				delete(c.pins, name)
			} else {
				c.pins[name]--
			}
			processMu.Unlock()
		})
	}, nil
}

// Close releases the cache's process and OS leases. All later operations fail
// with ErrClosed. Close is idempotent.
func (c *Cache) Close() error {
	processMu.Lock()
	if c == nil || c.closed {
		processMu.Unlock()
		return nil
	}
	c.closed = true
	processMu.Unlock()
	c.inflight.Wait()
	processMu.Lock()
	defer processMu.Unlock()
	delete(openRoots, c.root)
	c.pins = nil
	if c.lease == nil {
		return nil
	}
	err := c.lease.Close()
	c.lease = nil
	return err
}

// acquire locks key's object for one operation, which then runs without processMu.
func (c *Cache) acquire(key minecraft.ResourcePackCacheKey) (string, func(), error) {
	processMu.Lock()
	if err := c.checkOpen(); err != nil {
		processMu.Unlock()
		return "", nil, err
	}
	name, err := objectName(key)
	if err != nil {
		processMu.Unlock()
		return "", nil, err
	}
	lock := c.keys[name]
	if lock == nil {
		lock = &keyLock{}
		c.keys[name] = lock
	}
	lock.refs++
	c.inflight.Add(1)
	processMu.Unlock()
	lock.Lock()
	return name, func() {
		lock.Unlock()
		processMu.Lock()
		if lock.refs--; lock.refs == 0 {
			delete(c.keys, name)
		}
		processMu.Unlock()
		c.inflight.Done()
	}, nil
}

// Load returns a verified pack or a cache miss. Corrupt and malformed entries
// fail closed and are removed when safe so the caller can redownload them.
func (c *Cache) Load(ctx context.Context, key minecraft.ResourcePackCacheKey) (*resource.Pack, error) {
	name, release, err := c.acquire(key)
	if err != nil {
		return nil, err
	}
	defer release()
	path := filepath.Join(c.root, name)
	pack, ok, err := readVerified(ctx, path, key)
	if err != nil {
		return nil, err
	}
	processMu.Lock()
	defer processMu.Unlock()
	recorded := c.index[name]
	if recorded.invalid || !ok || (recorded.verified && recorded.checksum != pack.Checksum()) {
		c.drop(name, path)
		return nil, nil
	}
	c.touch(name, path, key.Size, pack.Checksum())
	return pack, nil
}

// Store reuses identical bytes or replaces an unpinned object with the selected archive.
func (c *Cache) Store(ctx context.Context, key minecraft.ResourcePackCacheKey, pack *resource.Pack) error {
	name, release, err := c.acquire(key)
	if err != nil {
		return err
	}
	defer release()
	if pack == nil || !key.Matches(pack) {
		return errors.New("packcache: resource pack does not match key")
	}
	if key.Size > c.quota {
		return errors.New("packcache: object exceeds quota")
	}
	dest := filepath.Join(c.root, name)
	existing, ok, err := readVerified(ctx, dest, key)
	if err != nil {
		return err
	}
	processMu.Lock()
	recorded := c.index[name]
	if ok && existing.Checksum() == pack.Checksum() && (!recorded.verified || recorded.checksum == pack.Checksum()) {
		c.touch(name, dest, key.Size, pack.Checksum())
		processMu.Unlock()
		return nil
	}
	if _, exists := c.index[name]; exists && c.pins[name] != 0 {
		processMu.Unlock()
		return errors.New("packcache: replacement deferred while archive is pinned")
	}
	c.drop(name, dest)
	if err := c.evict(key.Size); err != nil {
		processMu.Unlock()
		return err
	}
	c.record(name, entry{size: key.Size, used: c.clock()}) // reserves the quota while writing unlocked
	processMu.Unlock()
	if err := c.writeObject(ctx, key, pack, dest); err != nil {
		processMu.Lock()
		c.forget(name)
		processMu.Unlock()
		return err
	}
	processMu.Lock()
	c.touch(name, dest, key.Size, pack.Checksum())
	processMu.Unlock()
	return nil
}

// writeObject publishes pack at dest through a private temporary file. Nothing is fsynced: a torn
// object fails verification on load and is downloaded again.
func (c *Cache) writeObject(ctx context.Context, key minecraft.ResourcePackCacheKey, pack *resource.Pack, dest string) error {
	temp, err := os.CreateTemp(c.root, tempPrefix)
	if err != nil {
		return fmt.Errorf("packcache: create temporary object: %w", err)
	}
	tempPath := temp.Name()
	defer os.Remove(tempPath)
	if err := temp.Chmod(0o600); err != nil {
		temp.Close()
		return err
	}
	n, copyErr := copyContext(ctx, temp, io.NewSectionReader(pack, 0, int64(key.Size)), key.Size)
	if copyErr == nil && n != key.Size {
		copyErr = errors.New("packcache: archive changed while storing")
	}
	closeErr := temp.Close()
	if copyErr != nil {
		return copyErr
	}
	if closeErr != nil {
		return closeErr
	}
	if err := publishNoReplace(tempPath, dest); err != nil {
		if !errors.Is(err, fs.ErrExist) {
			return fmt.Errorf("packcache: publish object: %w", err)
		}
		if existing, ok, verifyErr := readVerified(ctx, dest, key); verifyErr != nil {
			return verifyErr
		} else if !ok || existing.Checksum() != pack.Checksum() {
			return errors.New("packcache: existing object differs from selected archive")
		}
	}
	return nil
}

// touch records the verified archive's hash and refreshes its eviction age.
func (c *Cache) touch(name, path string, size uint64, checksum [32]byte) {
	now := c.clock()
	_ = os.Chtimes(path, now, now)
	c.record(name, entry{size: size, used: now, checksum: checksum, verified: true})
}

func (c *Cache) checkOpen() error {
	if c == nil || c.closed {
		return ErrClosed
	}
	return nil
}

func (c *Cache) scan() error {
	if err := c.cleanupTemps(); err != nil {
		return err
	}
	dir, err := os.Open(c.root)
	if err != nil {
		return fmt.Errorf("packcache: scan: %w", err)
	}
	defer dir.Close()
	count := 0
	for {
		entries, readErr := dir.ReadDir(1024)
		for _, item := range entries {
			name := item.Name()
			if !validObjectName(name) {
				continue
			}
			count++
			if count > maxIndexEntries {
				return errors.New("packcache: object directory has too many entries")
			}
			path := filepath.Join(c.root, name)
			info, err := os.Lstat(path)
			if err != nil || !regularNoLink(info) || info.Size() < 0 {
				continue
			}
			size := uint64(info.Size())
			if math.MaxUint64-c.used < size {
				return errors.New("packcache: size overflow while scanning")
			}
			c.index[name] = entry{size: size, used: info.ModTime()}
			c.used += size
		}
		if readErr == io.EOF {
			return nil
		}
		if readErr != nil {
			return fmt.Errorf("packcache: scan: %w", readErr)
		}
	}
}

func (c *Cache) cleanupTemps() error {
	dir, err := os.Open(c.root)
	if err != nil {
		return fmt.Errorf("packcache: open object directory: %w", err)
	}
	defer dir.Close()
	for {
		items, readErr := dir.ReadDir(1024)
		for _, item := range items {
			if !strings.HasPrefix(item.Name(), tempPrefix) {
				continue
			}
			path := filepath.Join(c.root, item.Name())
			info, err := os.Lstat(path)
			if err == nil && regularNoLink(info) {
				if err := os.Remove(path); err != nil && !errors.Is(err, fs.ErrNotExist) {
					return fmt.Errorf("packcache: remove temporary object: %w", err)
				}
			}
		}
		if readErr == io.EOF {
			return nil
		}
		if readErr != nil {
			return fmt.Errorf("packcache: scan temporary objects: %w", readErr)
		}
	}
}

func (c *Cache) evict(incoming uint64) error {
	if incoming > c.quota {
		return errors.New("packcache: object exceeds quota")
	}
	if c.used <= c.quota-incoming {
		return nil
	}
	type candidate struct {
		name  string
		entry entry
	}
	items := make([]candidate, 0, len(c.index))
	for name, entry := range c.index {
		if c.pins[name] == 0 && c.keys[name] == nil {
			items = append(items, candidate{name, entry})
		}
	}
	sort.Slice(items, func(i, j int) bool {
		if items[i].entry.used.Equal(items[j].entry.used) {
			return items[i].name < items[j].name
		}
		return items[i].entry.used.Before(items[j].entry.used)
	})
	for _, item := range items {
		if c.used <= c.quota-incoming {
			return nil
		}
		path := filepath.Join(c.root, item.name)
		info, err := os.Lstat(path)
		if err != nil || !regularNoLink(info) {
			delete(c.index, item.name)
			if c.used >= item.entry.size {
				c.used -= item.entry.size
			}
			continue
		}
		if err := os.Remove(path); err != nil {
			return fmt.Errorf("packcache: evict object: %w", err)
		}
		delete(c.index, item.name)
		c.used -= item.entry.size
	}
	if c.used > c.quota-incoming {
		return errors.New("packcache: quota occupied by pinned objects")
	}
	return nil
}

// drop rejects an invalid object and removes it only when no session holds its bytes.
func (c *Cache) drop(name, path string) {
	if c.pins[name] != 0 {
		if recorded, ok := c.index[name]; ok {
			recorded.invalid = true
			c.index[name] = recorded
		}
		return
	}
	c.forget(name)
	if info, err := os.Lstat(path); err == nil && (regularNoLink(info) || hasLinkAttribute(info)) {
		_ = os.Remove(path)
	}
}

// forget removes name from the index, leaving any file in place.
func (c *Cache) forget(name string) {
	if old, ok := c.index[name]; ok {
		if c.used >= old.size {
			c.used -= old.size
		}
		delete(c.index, name)
	}
}

func (c *Cache) record(name string, next entry) {
	if old, ok := c.index[name]; ok {
		if c.used >= old.size {
			c.used -= old.size
		}
	}
	c.index[name] = next
	c.used += next.size
}

func objectName(key minecraft.ResourcePackCacheKey) (string, error) {
	if key.UUID == [16]byte{} || key.Size > uint64(^uint(0)>>1) || key.Size > math.MaxInt64 || len(key.Version) == 0 || len(key.Version) > maxVersionBytes || !utf8.ValidString(key.Version) {
		return "", errors.New("packcache: invalid resource pack key")
	}
	h := sha256.New()
	h.Write(key.UUID[:])
	var lengths [10]byte
	binary.BigEndian.PutUint16(lengths[:2], uint16(len(key.Version)))
	binary.BigEndian.PutUint64(lengths[2:], key.Size)
	h.Write(lengths[:])
	h.Write([]byte(key.Version))
	return hex.EncodeToString(h.Sum(nil)) + objectSuffix, nil
}

func validObjectName(name string) bool {
	if len(name) != 64+len(objectSuffix) || !strings.HasSuffix(name, objectSuffix) {
		return false
	}
	_, err := hex.DecodeString(strings.TrimSuffix(name, objectSuffix))
	return err == nil
}

func readVerified(ctx context.Context, path string, key minecraft.ResourcePackCacheKey) (*resource.Pack, bool, error) {
	info, err := os.Lstat(path)
	if errors.Is(err, fs.ErrNotExist) {
		return nil, false, nil
	}
	if err != nil {
		return nil, false, fmt.Errorf("packcache: inspect object: %w", err)
	}
	if !regularNoLink(info) || info.Size() < 0 || uint64(info.Size()) != key.Size {
		return nil, false, nil
	}
	f, err := openRegular(path)
	if err != nil {
		return nil, false, nil
	}
	defer f.Close()
	data := make([]byte, 0, int(key.Size))
	w := &sliceWriter{data: &data, limit: key.Size}
	n, err := copyContext(ctx, w, f, key.Size)
	if err != nil {
		return nil, false, err
	}
	if n != key.Size {
		return nil, false, nil
	}
	pack, err := resource.ReadBytes(data)
	if err != nil || !key.Matches(pack) {
		return nil, false, nil
	}
	return pack, true, nil
}

type sliceWriter struct {
	data  *[]byte
	limit uint64
}

func (w *sliceWriter) Write(p []byte) (int, error) {
	if uint64(len(*w.data))+uint64(len(p)) > w.limit {
		return 0, errors.New("packcache: object exceeds expected size")
	}
	*w.data = append(*w.data, p...)
	return len(p), nil
}

func copyContext(ctx context.Context, dst io.Writer, src io.Reader, limit uint64) (uint64, error) {
	buf := make([]byte, 64<<10)
	var total uint64
	for total < limit {
		if err := ctx.Err(); err != nil {
			return total, err
		}
		want := uint64(len(buf))
		if limit-total < want {
			want = limit - total
		}
		n, readErr := src.Read(buf[:int(want)])
		if n > 0 {
			written, err := dst.Write(buf[:n])
			total += uint64(written)
			if err != nil {
				return total, err
			}
			if written != n {
				return total, io.ErrShortWrite
			}
		}
		if readErr == io.EOF {
			break
		}
		if readErr != nil {
			return total, readErr
		}
		if n == 0 {
			return total, io.ErrNoProgress
		}
	}
	return total, nil
}

var _ minecraft.ResourcePackCache = (*Cache)(nil)
