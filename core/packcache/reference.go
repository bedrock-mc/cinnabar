package packcache

import (
	"errors"
	"os"
	"path/filepath"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// Reference pins an existing archive and returns its absolute path until release is called.
// The receiver must verify the file's size and the recorded checksum before using it.
// Missing files and disabled caches return an error so the caller can stream the archive instead.
func (c *Cache) Reference(pack *resource.Pack) (path string, checksum [32]byte, release func(), err error) {
	if pack == nil {
		return "", checksum, nil, errors.New("packcache: nil pack")
	}
	key := minecraft.ResourcePackCacheKey{UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size())}
	name, finish, err := c.acquire(key)
	if err != nil {
		return "", checksum, nil, err
	}
	defer finish()
	path = filepath.Join(c.root, name)
	info, err := os.Lstat(path)
	if err != nil {
		return "", checksum, nil, err
	}
	if !regularNoLink(info) || info.Size() < 0 || uint64(info.Size()) != key.Size {
		return "", checksum, nil, errors.New("packcache: cached archive is not a regular file of the expected size")
	}
	processMu.Lock()
	recorded := c.index[name]
	processMu.Unlock()
	if recorded.invalid || !recorded.verified || recorded.checksum != pack.Checksum() {
		return "", checksum, nil, errors.New("packcache: cached archive differs from selected pack or is not verified")
	}
	checksum = recorded.checksum
	release, err = c.Pin(key)
	if err != nil {
		return "", checksum, nil, err
	}
	return path, checksum, release, nil
}
