package main

import (
	"encoding/binary"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strings"

	_ "github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/world"
	_ "github.com/df-mc/dragonfly/server/world/biome"
)

const (
	registryHeader      = "BREG1002"
	biomeRegistryHeader = "BIOREG01"

	flagAir              uint8 = 1 << 0
	flagCubeGeometry     uint8 = 1 << 1
	flagOccludesFullFace uint8 = 1 << 2
	flagLeafModel        uint8 = 1 << 3
	allBlockFlags              = flagAir | flagCubeGeometry | flagOccludesFullFace | flagLeafModel

	maxNameBytes   = 1<<16 - 1
	maxStateBytes  = 1 << 20
	maxRecordCount = 1 << 16

	maxBiomeRecordCount = 1_024
	maxBiomeNameBytes   = 256
)

// Record is one serialized block-registry entry.
type Record struct {
	SequentialID uint32
	NetworkHash  uint32
	Flags        uint8
	Name         string
	StateJSON    []byte
}

// BiomeRecord is one stable Dragonfly network biome registry entry.
type BiomeRecord struct {
	ID   uint32
	Name string
}

func main() {
	out := flag.String("out", "", "path to write the block registry")
	biomeOut := flag.String("biome-out", "", "optional path to write the biome registry")
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "registrygen: -out is required")
		os.Exit(2)
	}

	records, err := collect(world.DefaultBlockRegistry)
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	encoded, err := encode(records)
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	if err := os.MkdirAll(filepath.Dir(*out), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: create output directory: %v\n", err)
		os.Exit(1)
	}
	if err := os.WriteFile(*out, encoded, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: write output: %v\n", err)
		os.Exit(1)
	}

	if *biomeOut == "" {
		return
	}
	biomeRecords, err := collectBiomes(world.Biomes())
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	encodedBiomes, err := encodeBiomeRegistry(biomeRecords)
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	if err := os.MkdirAll(filepath.Dir(*biomeOut), 0o755); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: create biome output directory: %v\n", err)
		os.Exit(1)
	}
	if err := os.WriteFile(*biomeOut, encodedBiomes, 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: write biome output: %v\n", err)
		os.Exit(1)
	}
}

func collectBiomes(biomes []world.Biome) ([]BiomeRecord, error) {
	if len(biomes) > maxBiomeRecordCount {
		return nil, fmt.Errorf("too many biome records: %d exceeds %d", len(biomes), maxBiomeRecordCount)
	}
	records := make([]BiomeRecord, 0, len(biomes))
	for _, biome := range biomes {
		if biome == nil {
			return nil, errors.New("biome registry contains nil biome")
		}
		id := biome.EncodeBiome()
		if id < 0 || id > math.MaxUint16 {
			return nil, fmt.Errorf("biome ID %d is outside 0..%d", id, uint16(math.MaxUint16))
		}
		name := canonicalBiomeName(biome.String())
		records = append(records, BiomeRecord{ID: uint32(id), Name: name})
	}
	return records, nil
}

func canonicalBiomeName(name string) string {
	if strings.ContainsRune(name, ':') || name == "" {
		return name
	}
	return "minecraft:" + name
}

func encodeBiomeRegistry(records []BiomeRecord) ([]byte, error) {
	if len(records) > maxBiomeRecordCount {
		return nil, fmt.Errorf("too many biome records: %d exceeds %d", len(records), maxBiomeRecordCount)
	}

	sorted := append([]BiomeRecord(nil), records...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].ID < sorted[j].ID })
	seenIDs := make(map[uint32]struct{}, len(sorted))
	seenNames := make(map[string]struct{}, len(sorted))
	for _, record := range sorted {
		if record.ID > math.MaxUint16 {
			return nil, fmt.Errorf("biome ID %d is outside 0..%d", record.ID, uint16(math.MaxUint16))
		}
		if _, exists := seenIDs[record.ID]; exists {
			return nil, fmt.Errorf("duplicate biome ID: %d", record.ID)
		}
		seenIDs[record.ID] = struct{}{}
		if record.Name == "" {
			return nil, fmt.Errorf("biome name is empty for ID %d", record.ID)
		}
		if len(record.Name) > maxBiomeNameBytes {
			return nil, fmt.Errorf("biome name too long for ID %d: %d bytes exceeds %d", record.ID, len(record.Name), maxBiomeNameBytes)
		}
		if _, exists := seenNames[record.Name]; exists {
			return nil, fmt.Errorf("duplicate biome name: %s", record.Name)
		}
		seenNames[record.Name] = struct{}{}
	}

	encoded := make([]byte, 0, len(biomeRegistryHeader)+4)
	encoded = append(encoded, biomeRegistryHeader...)
	encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(sorted)))
	for _, record := range sorted {
		encoded = binary.LittleEndian.AppendUint32(encoded, record.ID)
		encoded = binary.LittleEndian.AppendUint16(encoded, uint16(len(record.Name)))
		encoded = append(encoded, record.Name...)
	}
	return encoded, nil
}

func collect(registry world.BlockRegistry) ([]Record, error) {
	if registry == nil {
		return nil, errors.New("block registry is nil")
	}
	registry.Finalize()
	blocks := registry.Blocks()
	if len(blocks) > maxRecordCount {
		return nil, fmt.Errorf("too many records: %d exceeds %d", len(blocks), maxRecordCount)
	}

	records := make([]Record, 0, len(blocks))
	for rid, value := range blocks {
		name, properties := value.EncodeBlock()
		networkHash, ok := registry.RuntimeIDToHash(uint32(rid))
		if !ok {
			return nil, fmt.Errorf("runtime ID %d has no network hash", rid)
		}
		stateJSON, err := canonicalJSON(properties)
		if err != nil {
			return nil, fmt.Errorf("encode state properties for runtime ID %d: %w", rid, err)
		}

		records = append(records, Record{
			SequentialID: uint32(rid),
			NetworkHash:  networkHash,
			Flags:        classifyFlags(value),
			Name:         name,
			StateJSON:    stateJSON,
		})
	}
	return records, nil
}

func validRecordFlags(flags uint8) bool {
	if flags&^allBlockFlags != 0 {
		return false
	}
	air := flags&flagAir != 0
	cube := flags&flagCubeGeometry != 0
	occludes := flags&flagOccludesFullFace != 0
	leaf := flags&flagLeafModel != 0
	return (!air || flags == flagAir) && (!occludes || cube) && (!leaf || (cube && !occludes))
}

func classifyFlags(value world.Block) uint8 {
	name, properties := value.EncodeBlock()
	if name == "minecraft:air" {
		return flagAir
	}
	switch value.Model().(type) {
	case model.Leaves:
		return flagCubeGeometry | flagLeafModel
	case model.Solid:
		return flagCubeGeometry | flagOccludesFullFace
	}

	// BasicBlockRegistry uses the high half returned by Hash as its public
	// unknownBlock discriminator: math.MaxUint64 means no concrete block
	// implementation is registered. Its unknownModel deliberately looks like a
	// full cube, so model geometry cannot safely classify these states.
	_, stateHash := value.Hash()
	if stateHash == math.MaxUint64 && approvedUnknownFullCubeState(name, properties) {
		return flagCubeGeometry | flagOccludesFullFace
	}
	return 0
}

func approvedUnknownFullCubeState(name string, properties map[string]any) bool {
	switch name {
	case "minecraft:mycelium":
		return len(properties) == 0
	case "minecraft:red_mushroom_block", "minecraft:brown_mushroom_block", "minecraft:mushroom_stem":
		if len(properties) != 1 {
			return false
		}
		bits, ok := properties["huge_mushroom_bits"].(int32)
		return ok && bits >= 0 && bits <= 15
	default:
		return false
	}
}

func canonicalJSON(properties map[string]any) ([]byte, error) {
	return json.Marshal(properties)
}

func encode(records []Record) ([]byte, error) {
	if len(records) > maxRecordCount {
		return nil, fmt.Errorf("too many records: %d exceeds %d", len(records), maxRecordCount)
	}

	sorted := append([]Record(nil), records...)
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].SequentialID < sorted[j].SequentialID
	})

	seenSequentialIDs := make(map[uint32]struct{}, len(sorted))
	seenNetworkHashes := make(map[uint32]struct{}, len(sorted))
	for _, record := range sorted {
		if !validRecordFlags(record.Flags) {
			return nil, fmt.Errorf("invalid flags %#x for sequential ID %d", record.Flags, record.SequentialID)
		}
		if _, exists := seenSequentialIDs[record.SequentialID]; exists {
			return nil, fmt.Errorf("duplicate sequential ID: %d", record.SequentialID)
		}
		seenSequentialIDs[record.SequentialID] = struct{}{}
		if _, exists := seenNetworkHashes[record.NetworkHash]; exists {
			return nil, fmt.Errorf("duplicate network hash: %#x", record.NetworkHash)
		}
		seenNetworkHashes[record.NetworkHash] = struct{}{}
		if len(record.Name) > maxNameBytes {
			return nil, fmt.Errorf("name too long for sequential ID %d: %d bytes", record.SequentialID, len(record.Name))
		}
		if len(record.StateJSON) > maxStateBytes {
			return nil, fmt.Errorf("state payload too large for sequential ID %d: %d bytes", record.SequentialID, len(record.StateJSON))
		}
	}

	encoded := make([]byte, 0, len(registryHeader)+4)
	encoded = append(encoded, registryHeader...)
	encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(sorted)))
	for _, record := range sorted {
		encoded = binary.LittleEndian.AppendUint32(encoded, record.SequentialID)
		encoded = binary.LittleEndian.AppendUint32(encoded, record.NetworkHash)
		encoded = append(encoded, record.Flags)
		encoded = binary.LittleEndian.AppendUint16(encoded, uint16(len(record.Name)))
		encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(record.StateJSON)))
		encoded = append(encoded, record.Name...)
		encoded = append(encoded, record.StateJSON...)
	}
	return encoded, nil
}
