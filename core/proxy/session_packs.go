package proxy

import (
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// packArchiveCache can lend an on-disk archive until the session releases its pin.
type packArchiveCache interface {
	Reference(*resource.Pack) (path string, checksum [32]byte, release func(), err error)
}

// referenceSessionPacks replaces cached archive streams with references, retaining stack indices.
// Missing or disabled caches keep the original byte stream. The returned function releases all pins.
func referenceSessionPacks(cache minecraft.ResourcePackCache, selected []sessionPack, packs []*resource.Pack) func() {
	var releases []func()
	if cache, ok := cache.(packArchiveCache); ok {
		for index, pack := range packs {
			path, checksum, release, err := cache.Reference(pack)
			if err != nil {
				continue
			}
			selected[index].Cache = &sessionCachedArchive{Path: path, SHA256: checksum}
			packs[index] = nil
			releases = append(releases, release)
		}
	}
	return func() {
		for _, release := range releases {
			release()
		}
	}
}
