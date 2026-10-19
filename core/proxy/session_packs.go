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

// addInterceptedPacks places an interceptor's packs above and below the server's selection, keeping
// the first entry for each identity so a pack the server selected keeps its server position.
func addInterceptedPacks(selected []sessionPack, packs []*resource.Pack, above, below []*resource.Pack) ([]sessionPack, []*resource.Pack) {
	if len(above) == 0 && len(below) == 0 {
		return selected, packs
	}
	seen := make(map[string]bool, len(selected)+len(above)+len(below))
	for _, entry := range selected {
		seen[resourcePackIdentity(entry.UUID, entry.Version)] = true
	}
	var mergedSelected []sessionPack
	var mergedPacks []*resource.Pack
	add := func(additions []*resource.Pack) {
		for _, pack := range additions {
			if pack == nil {
				continue
			}
			id := resourcePackIdentity(pack.UUID().String(), pack.Version())
			if seen[id] {
				continue
			}
			seen[id] = true
			mergedSelected = append(mergedSelected, sessionPack{
				UUID: pack.UUID().String(), Version: pack.Version(),
				ContentKey: pack.ContentKey(), Size: uint64(max(pack.Size(), 0)),
			})
			mergedPacks = append(mergedPacks, pack)
		}
	}
	add(above)
	mergedSelected = append(mergedSelected, selected...)
	mergedPacks = append(mergedPacks, packs...)
	add(below)
	return mergedSelected, mergedPacks
}
