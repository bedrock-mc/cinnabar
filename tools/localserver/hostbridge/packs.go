package hostbridge

import (
	"encoding/hex"
	"slices"

	"github.com/df-mc/dragonfly/server"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
	"github.com/sandertv/gophertunnel/minecraft/resource"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/internal/hostcontract"
)

// resources retains the exact immutable Config.New resource list, including its generated pack.
type resources struct {
	snapshot hostcontract.Snapshot
	packs    []*resource.Pack
}

// captureResources never reopens, scans, downloads or rebuilds a configured resource pack.
func captureResources(conf server.Config) (resources, error) {
	r := resources{
		packs: slices.Clone(conf.Resources),
		snapshot: hostcontract.Snapshot{
			ProtocolVersion: uint32(protocol.CurrentProtocol), MaxPlayers: uint32(conf.MaxPlayers),
			MaxChunkRadius: uint32(conf.MaxChunkRadius), Required: conf.ResourcesRequired,
			Packs: make([]hostcontract.Pack, 0, len(conf.Resources)),
		},
	}
	for _, p := range r.packs {
		if p == nil || p.Size() <= 0 {
			return resources{}, errProtocol
		}
		checksum := p.Checksum()
		identity := ""
		if p.Encrypted() {
			identity = p.Manifest().Header.UUID.String()
		}
		r.snapshot.Packs = append(r.snapshot.Packs, hostcontract.Pack{
			Id: p.UUID().String(), Version: p.Version(), Size: uint64(p.Size()),
			Sha256: hex.EncodeToString(checksum[:]), ContentKey: p.ContentKey(),
			ContentIdentity: identity, DownloadUrl: p.DownloadURL(), PackType: packType(p),
		})
	}
	return r, nil
}

// packType preserves the selected Go delivery owner's module-priority policy.
func packType(p *resource.Pack) uint32 {
	switch {
	case p.HasWorldTemplate():
		return packet.ResourcePackTypeWorldTemplate
	case p.HasTextures() && (p.HasBehaviours() || p.HasScripts()):
		return packet.ResourcePackTypeAddon
	case !p.HasTextures() && (p.HasBehaviours() || p.HasScripts()):
		return packet.ResourcePackTypeBehaviour
	case p.HasTextures():
		return packet.ResourcePackTypeResources
	default:
		return packet.ResourcePackTypeSkins
	}
}

// read returns exactly the original bounded range, with no alternate source or chunk-name lookup.
func (r *resources) read(request hostcontract.Range) ([]byte, error) {
	if request.Archive >= uint32(len(r.packs)) || request.Length == 0 || request.Length > hostcontract.MAX_PACK_READ_BYTES {
		return nil, errProtocol
	}
	p := r.packs[request.Archive]
	size := uint64(p.Size())
	if request.Offset > size || uint64(request.Length) > size-request.Offset {
		return nil, errProtocol
	}
	b := make([]byte, request.Length)
	n, err := p.ReadAt(b, int64(request.Offset))
	if err != nil || n != len(b) {
		return nil, errProtocol
	}
	return b, nil
}
