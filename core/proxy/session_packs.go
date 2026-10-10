package proxy

import (
	"github.com/hashimthearab/rust-mcbe/core/internal/sessionwire"
	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/resource"
)

// packArchiveCache can lend an on-disk archive until the session releases its pin.
type packArchiveCache interface {
	Reference(*resource.Pack) (path string, release func(), err error)
}

// referenceSessionPacks replaces cached archive streams with references, retaining stack indices.
// Missing or disabled caches keep the original byte stream. The returned function releases all pins.
func referenceSessionPacks(cache minecraft.ResourcePackCache, selected []sessionwire.Pack, packs []*resource.Pack) func() {
	var releases []func()
	if cache, ok := cache.(packArchiveCache); ok {
		for index, pack := range packs {
			path, release, err := cache.Reference(pack)
			if err != nil {
				continue
			}
			selected[index].Cache = &sessionwire.CachedArchive{Path: path, SHA256: pack.Checksum()}
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
