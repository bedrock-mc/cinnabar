package packcache

import (
	"errors"
	"os"
	"path/filepath"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// Reference pins an existing archive and returns its absolute path until release is called.
// The receiver must verify the file's size and the selected pack's checksum before using it.
// Missing files and disabled caches return an error so the caller can stream the archive instead.
func (c *Cache) Reference(pack *resource.Pack) (path string, release func(), err error) {
	if pack == nil {
		return "", nil, errors.New("packcache: nil pack")
	}
	key := minecraft.ResourcePackCacheKey{UUID: pack.UUID(), Version: pack.Version(), Size: uint64(pack.Size())}
	name, finish, err := c.acquire(key)
	if err != nil {
		return "", nil, err
	}
	defer finish()
	path = filepath.Join(c.root, name)
	info, err := os.Lstat(path)
	if err != nil {
		return "", nil, err
	}
	if !regularNoLink(info) || info.Size() < 0 || uint64(info.Size()) != key.Size {
		return "", nil, errors.New("packcache: cached archive is not a regular file of the expected size")
	}
	release, err = c.Pin(key)
	if err != nil {
		return "", nil, err
	}
	return path, release, nil
}
