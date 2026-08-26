package main

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
)

// The protocol-2168 fallback rekey re-keys the provisional vanilla-fallback
// identity table from protocol-1001 network hashes to protocol-2168 network
// hashes without touching any visual payload. Entries join across the two
// checked-in registries by the exact canonical name+state key the projection
// pipeline uses, every entry must carry the identity fingerprint its joined
// record derives, and an entry landing on a cinnabar:reserved v2168 record is
// defensively excluded and counted. Network hashes are content-derived from
// the canonical identity, so a vanilla identity that survives a protocol bump
// keeps its hash while a genuinely re-keyed one moves; joining through the
// canonical key handles both, and for the current pinned corpus every entry
// joins unchanged, making the emitted bytes equal the input, which the
// real-registry test witnesses rather than assumes. Failure classes split by
// evidence style: identities unmatched in either registry accumulate into one
// exhaustive sorted entry-by-entry listing, while a stored fingerprint
// disagreeing with its joined legacy or current identity, duplicate legacy
// network hashes or duplicate current canonical keys inside the registry
// inputs themselves, and two entries colliding on one emitted network hash
// each fail closed immediately, naming the offending entries.
// The envelope geometry, texture alpha, and provenance inputs below are
// preserved verbatim from the reviewed v1001 tranche because this mode changes
// identity keying only.

const (
	fallbackRekeySchema = "cinnabar-vanilla-fallback-source-v1"
	fallbackRekeyStatus = "provisional-vanilla-fallback"

	// The pack and envelope constants below must change together with the
	// matching fields of assets/vanilla-fallback-source-v1001.json: the rekey
	// deliberately preserves the reviewed v1001 visual provenance verbatim
	// instead of deriving new values.
	fallbackRekeyPack    = "v1.26.30.32-preview"
	fallbackRekeyBlocks  = "a53c486ba078d5824ae9694bb5ad360d8b64645b935c903cf8b4e410c75c1592"
	fallbackRekeyTerrain = "fe8c2199f6b21c095f5f4612ea183f0ab358c74d6ba5580f3c1cabe00fc29329"

	fallbackRekeyEnvelopeRepo    = "CloudburstMC/Data"
	fallbackRekeyEnvelopeCommit  = "fb969c547236d87a17181941cd585a0eb18f7ceb"
	fallbackRekeyEnvelopePath    = "block_properties.json"
	fallbackRekeyEnvelopeSHA256  = "6ba1bfbb6a5824cacc39baedadcc5defce845fca14aaa77a48d756106c2bcdf59"
	fallbackRekeyContractMessage = "Known canonical vanilla identities only. Geometry is a conservative visualShape envelope; zero-volume envelopes use a neutral full cube. Resolvable pinned-pack textures are preferred, otherwise canonical stone is used. This status does not satisfy exact vanilla visual coverage."
)

// fallbackRekeyStats counts the input entries, emitted entries, defensively
// excluded reserved collisions, distinct emitted block names, and zero-volume
// envelopes of one rekey pass.
type fallbackRekeyStats struct {
	InputEntries     int
	OutputEntries    int
	ReservedExcluded int
	DistinctNames    int
	ZeroVolume       int
}

// rekeyedFallbackEntry is one emitted inventory row keyed by its new
// protocol-2168 network hash with the source payload preserved verbatim.
type rekeyedFallbackEntry struct {
	networkHash uint32
	bytes       [fallbackEntryBytes]byte
}

// vanillaFallbackSourceInventory binds the emitted fallback inventory binary
// by repository path, envelope schema/version, fixed entry width, and SHA-256.
type vanillaFallbackSourceInventory struct {
	Path       string `json:"path"`
	Schema     string `json:"schema"`
	Version    uint32 `json:"version"`
	EntryBytes int    `json:"entry_bytes"`
	SHA256     string `json:"sha256"`
}

// vanillaFallbackSourceRegistry binds one checked-in block registry binary by
// repository path and SHA-256.
type vanillaFallbackSourceRegistry struct {
	Path   string `json:"path"`
	SHA256 string `json:"sha256"`
}

// vanillaFallbackSourcePack pins the reviewed Mojang bedrock-samples release
// whose blocks and terrain-texture payloads supplied the visual materials.
type vanillaFallbackSourcePack struct {
	Release              string `json:"release"`
	BlocksSHA256         string `json:"blocks_sha256"`
	TerrainTextureSHA256 string `json:"terrain_texture_sha256"`
}

// vanillaFallbackSourceEnvelope pins the CloudburstMC Data commit whose block
// properties supplied the conservative geometry envelopes.
type vanillaFallbackSourceEnvelope struct {
	Repository string `json:"repository"`
	Commit     string `json:"commit"`
	Path       string `json:"path"`
	SHA256     string `json:"sha256"`
}

// vanillaFallbackSourceManifest is the deterministic provenance manifest
// published beside a compiled fallback inventory; field order is part of the
// committed artifact contract.
type vanillaFallbackSourceManifest struct {
	Schema              string                         `json:"schema"`
	Protocol            uint32                         `json:"protocol"`
	States              int                            `json:"states"`
	Names               int                            `json:"names"`
	ZeroVolumeEnvelopes int                            `json:"zero_volume_envelopes"`
	Status              string                         `json:"status"`
	Inventory           vanillaFallbackSourceInventory `json:"inventory"`
	InputInventory      vanillaFallbackSourceRegistry  `json:"input_inventory"`
	Registry            vanillaFallbackSourceRegistry  `json:"registry"`
	LegacyRegistry      vanillaFallbackSourceRegistry  `json:"legacy_registry"`
	BedrockPack         vanillaFallbackSourcePack      `json:"bedrock_pack"`
	EnvelopeSource      vanillaFallbackSourceEnvelope  `json:"envelope_source"`
	Contract            string                         `json:"contract"`
}

// fallbackRekeyReport is the success-path stdout summary echoed when no source
// manifest was requested; with a manifest, stdout stays silent. Input,
// reserved-excluded, and emitted counts each carry their own field so no
// figure silently changes meaning.
type fallbackRekeyReport struct {
	InputEntries     int    `json:"input_entries"`
	ReservedExcluded int    `json:"reserved_excluded"`
	OutputEntries    int    `json:"output_entries"`
	Output           string `json:"output"`
}

// fallbackIdentityFingerprint mirrors the consumer's FNV-style canonical
// identity fingerprint exactly: offset basis, then for each byte of the name,
// the zero separator, and the canonical state JSON one exclusive-or followed
// by one multiply modulo 2^64.
func fallbackIdentityFingerprint(name string, state []byte) uint64 {
	const offsetBasis = 14695981039346656037
	const prime = 1099511628211
	hash := uint64(offsetBasis)
	for index := 0; index < len(name); index++ {
		hash ^= uint64(name[index])
		hash *= prime
	}
	hash ^= uint64(0)
	hash *= prime
	for index := 0; index < len(state); index++ {
		hash ^= uint64(state[index])
		hash *= prime
	}
	return hash
}

// writeRekeyedFallback rekeys the input inventory against the checked-in
// legacy and current registries, writes the sibling lowercase SHA-256 sidecar
// beside the output like every other checked-in data artifact, and optionally
// writes a source manifest that mirrors
// assets/vanilla-fallback-source-v1001.json with the current BREG's exact
// SHA-256 binding plus the input inventory and legacy registry provenance.
// It returns the rekey stats so the caller can echo a stdout summary when no
// manifest was requested.
func writeRekeyedFallback(inputPath, legacyBREGPath, newBREGPath, outputPath, manifestPath string) (fallbackRekeyStats, error) {
	input, err := os.ReadFile(inputPath)
	if err != nil {
		return fallbackRekeyStats{}, fmt.Errorf("read fallback inventory: %w", err)
	}
	legacyBytes, err := os.ReadFile(legacyBREGPath)
	if err != nil {
		return fallbackRekeyStats{}, fmt.Errorf("read legacy fallback BREG: %w", err)
	}
	currentBytes, err := os.ReadFile(newBREGPath)
	if err != nil {
		return fallbackRekeyStats{}, fmt.Errorf("read current fallback BREG: %w", err)
	}
	_, legacyRecords, err := decodeBREGRecords(legacyBytes, registryProtocol)
	if err != nil {
		return fallbackRekeyStats{}, err
	}
	_, currentRecords, err := decodeBREGRecords(currentBytes, v2168BlockProtocol)
	if err != nil {
		return fallbackRekeyStats{}, err
	}
	output, stats, err := rekeyFallbackInventory(input, legacyRecords, currentRecords)
	if err != nil {
		return fallbackRekeyStats{}, err
	}
	if err := os.MkdirAll(filepath.Dir(outputPath), 0o755); err != nil {
		return fallbackRekeyStats{}, fmt.Errorf("create fallback rekey output directory: %w", err)
	}
	if err := os.WriteFile(outputPath, output, 0o644); err != nil {
		return fallbackRekeyStats{}, fmt.Errorf("write fallback rekey output: %w", err)
	}
	digest := sha256.Sum256(output)
	if err := os.WriteFile(strings.TrimSuffix(outputPath, filepath.Ext(outputPath))+".sha256", []byte(fmt.Sprintf("%x\n", digest)), 0o644); err != nil {
		return fallbackRekeyStats{}, fmt.Errorf("write fallback rekey checksum: %w", err)
	}
	if manifestPath == "" {
		return stats, nil
	}
	manifest := vanillaFallbackSourceManifest{
		Schema:              fallbackRekeySchema,
		Protocol:            v2168BlockProtocol,
		States:              stats.OutputEntries,
		Names:               stats.DistinctNames,
		ZeroVolumeEnvelopes: stats.ZeroVolume,
		Status:              fallbackRekeyStatus,
		Inventory: vanillaFallbackSourceInventory{
			Path:       filepath.ToSlash(outputPath),
			Schema:     "CVFB1001",
			Version:    1,
			EntryBytes: fallbackEntryBytes,
			SHA256:     fmt.Sprintf("%x", digest),
		},
		InputInventory: vanillaFallbackSourceRegistry{
			Path:   filepath.ToSlash(inputPath),
			SHA256: fmt.Sprintf("%x", sha256.Sum256(input)),
		},
		Registry: vanillaFallbackSourceRegistry{
			Path:   filepath.ToSlash(newBREGPath),
			SHA256: fmt.Sprintf("%x", sha256.Sum256(currentBytes)),
		},
		LegacyRegistry: vanillaFallbackSourceRegistry{
			Path:   filepath.ToSlash(legacyBREGPath),
			SHA256: fmt.Sprintf("%x", sha256.Sum256(legacyBytes)),
		},
		BedrockPack: vanillaFallbackSourcePack{
			Release:              fallbackRekeyPack,
			BlocksSHA256:         fallbackRekeyBlocks,
			TerrainTextureSHA256: fallbackRekeyTerrain,
		},
		EnvelopeSource: vanillaFallbackSourceEnvelope{
			Repository: fallbackRekeyEnvelopeRepo,
			Commit:     fallbackRekeyEnvelopeCommit,
			Path:       fallbackRekeyEnvelopePath,
			SHA256:     fallbackRekeyEnvelopeSHA256,
		},
		Contract: fallbackRekeyContractMessage,
	}
	manifestBytes, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return fallbackRekeyStats{}, fmt.Errorf("encode fallback rekey manifest: %w", err)
	}
	manifestBytes = append(manifestBytes, '\n')
	if err := os.WriteFile(manifestPath, manifestBytes, 0o644); err != nil {
		return fallbackRekeyStats{}, fmt.Errorf("write fallback rekey manifest: %w", err)
	}
	return stats, nil
}

// rekeyFallbackInventory rewrites every input entry under its protocol-2168
// network hash. It fails closed naming each entry whose stored fingerprint
// disagrees with its joined identity, or whose legacy or current identity is
// unmatched, and excludes entries that land on reserved records while counting
// them. Output entries are strictly sorted by the new network hash so the
// runtime binary search stays valid.
func rekeyFallbackInventory(input []byte, legacyRecords, currentRecords []Record) ([]byte, fallbackRekeyStats, error) {
	inputCount, err := parseFallbackInventoryHeader(input)
	if err != nil {
		return nil, fallbackRekeyStats{}, err
	}
	stats := fallbackRekeyStats{InputEntries: inputCount}

	legacyByHash := make(map[uint32]Record, len(legacyRecords))
	for _, record := range legacyRecords {
		if previous, exists := legacyByHash[record.NetworkHash]; exists {
			return nil, fallbackRekeyStats{}, fmt.Errorf("legacy fallback BREG hash %#x is shared by %s and %s", record.NetworkHash, previous.Name, record.Name)
		}
		legacyByHash[record.NetworkHash] = record
	}
	currentByKey := make(map[string]Record, len(currentRecords))
	for _, record := range currentRecords {
		key := canonicalRecordKey(record.Name, record.StateJSON)
		if previous, exists := currentByKey[key]; exists {
			return nil, fallbackRekeyStats{}, fmt.Errorf("current fallback BREG key %q is shared by records %d and %d", key, previous.SequentialID, record.SequentialID)
		}
		currentByKey[key] = record
	}

	emitted := make([]rekeyedFallbackEntry, 0, inputCount)
	names := make(map[string]struct{}, inputCount)
	hashOwners := make(map[uint32]string, inputCount)
	var unmatched []string
	for index := 0; index < inputCount; index++ {
		start := fallbackHeaderBytes + index*fallbackEntryBytes
		entry := input[start : start+fallbackEntryBytes]
		entryHash := binary.LittleEndian.Uint32(entry[0:4])
		storedFingerprint := binary.LittleEndian.Uint64(entry[4:12])
		legacy, ok := legacyByHash[entryHash]
		if !ok {
			unmatched = append(unmatched, fmt.Sprintf("network hash %#x", entryHash))
			continue
		}
		if fingerprint := fallbackIdentityFingerprint(legacy.Name, legacy.StateJSON); fingerprint != storedFingerprint {
			return nil, fallbackRekeyStats{}, fmt.Errorf("fallback entry %d (%s) fails identity fingerprint verification: stored %#016x want %#016x", index, legacy.Name, storedFingerprint, fingerprint)
		}
		current, ok := currentByKey[canonicalRecordKey(legacy.Name, legacy.StateJSON)]
		if !ok {
			unmatched = append(unmatched, fmt.Sprintf("%s %s", legacy.Name, legacy.StateJSON))
			continue
		}
		if fingerprint := fallbackIdentityFingerprint(current.Name, current.StateJSON); fingerprint != storedFingerprint {
			return nil, fallbackRekeyStats{}, fmt.Errorf("fallback entry %d (%s) has a divergent identity between registries: stored %#016x want %#016x", index, current.Name, storedFingerprint, fingerprint)
		}
		if current.Name == retailReservedName {
			stats.ReservedExcluded++
			continue
		}
		if owner, exists := hashOwners[current.NetworkHash]; exists {
			return nil, fallbackRekeyStats{}, fmt.Errorf("fallback entries %q and %q collide on new network hash %#x", owner, legacy.Name, current.NetworkHash)
		}
		hashOwners[current.NetworkHash] = legacy.Name
		var rekeyed rekeyedFallbackEntry
		rekeyed.networkHash = current.NetworkHash
		binary.LittleEndian.PutUint32(rekeyed.bytes[0:4], current.NetworkHash)
		copy(rekeyed.bytes[4:], entry[4:])
		names[current.Name] = struct{}{}
		if isZeroVolumeEnvelope(entry[12:25]) {
			stats.ZeroVolume++
		}
		emitted = append(emitted, rekeyed)
	}
	if len(unmatched) > 0 {
		sort.Strings(unmatched)
		return nil, fallbackRekeyStats{}, fmt.Errorf("fallback rekey fails closed: %d unmatched entries have no joined identity: %s", len(unmatched), strings.Join(unmatched, "; "))
	}
	sort.Slice(emitted, func(i, j int) bool { return emitted[i].networkHash < emitted[j].networkHash })

	output := make([]byte, 0, fallbackHeaderBytes+len(emitted)*fallbackEntryBytes)
	output = append(output, "CVFB1001"...)
	output = binary.LittleEndian.AppendUint32(output, 1)
	output = binary.LittleEndian.AppendUint32(output, uint32(len(emitted)))
	for _, entry := range emitted {
		output = append(output, entry.bytes[:]...)
	}
	stats.OutputEntries = len(emitted)
	stats.DistinctNames = len(names)
	return output, stats, nil
}

func isZeroVolumeEnvelope(payload []byte) bool {
	for axis := 0; axis < 3; axis++ {
		min := int16(binary.LittleEndian.Uint16(payload[axis*2:]))
		max := int16(binary.LittleEndian.Uint16(payload[6+axis*2:]))
		if min >= max {
			return true
		}
	}
	return false
}
