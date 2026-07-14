package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"math"
	"os"
	"path/filepath"
	"sort"
	"strconv"
	"strings"

	_ "github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/world"
	_ "github.com/df-mc/dragonfly/server/world/biome"
	"github.com/sandertv/gophertunnel/minecraft/nbt"
)

const (
	registryHeader      = "BREG1003"
	biomeRegistryHeader = "BIOREG01"
	registryProtocol    = 1001

	flagAir              uint8 = 1 << 0
	flagCubeGeometry     uint8 = 1 << 1
	flagOccludesFullFace uint8 = 1 << 2
	flagLeafModel        uint8 = 1 << 3
	allBlockFlags              = flagAir | flagCubeGeometry | flagOccludesFullFace | flagLeafModel

	maxNameBytes               = 1<<16 - 1
	maxStateBytes              = 1 << 20
	maxRecordCount             = 1 << 16
	maxCollisionBoxesPerRecord = 7
	collisionFixedScale        = 100_000_000.0

	maxBiomeRecordCount = 1_024
	maxBiomeNameBytes   = 256
)

type ScalarKind uint8

const (
	ScalarByte ScalarKind = iota + 1
	ScalarInt
	ScalarString
)

type TypedScalar struct {
	Kind   ScalarKind
	Byte   byte
	Int    int32
	String string
}

type StateProperty struct {
	Name  string
	Value TypedScalar
}

type SourceState struct {
	Name          string
	Properties    []StateProperty
	Ordinal       uint32
	NetworkHash   uint32
	Flags         uint8
	CollisionSeed CollisionSeed
}

type ModelFamily uint8

const (
	ModelFamilyUnknown ModelFamily = iota
	ModelFamilyAir
	ModelFamilyCube
	ModelFamilyLeaves
	ModelFamilyCross
	ModelFamilyCrop
	ModelFamilyLiquid
	ModelFamilySlab
	ModelFamilyStair
	ModelFamilyDoor
	ModelFamilyTrapdoor
	ModelFamilyPane
	ModelFamilyFence
	ModelFamilyGate
	ModelFamilyChest
	ModelFamilySign
	ModelFamilyWall
	ModelFamilyBed
	ModelFamilyRail
	ModelFamilyTorch
	ModelFamilyButton
	ModelFamilyPressurePlate
	ModelFamilyCarpet
	ModelFamilyLayer
	ModelFamilyDecorative
	ModelFamilyStatue
	ModelFamilyCuboid
	ModelFamilyAquatic
	ModelFamilyCocoa
	ModelFamilyLever
	ModelFamilyInvisible
	ModelFamilyFlowerBed
	ModelFamilyVine
	ModelFamilyGlowLichen
	ModelFamilySculkVein
	ModelFamilyChiseledBookshelf
	ModelFamilyResinClump
)

const maxModelFamily = ModelFamilyResinClump

type ContributorRole uint8

const (
	ContributorPrimary ContributorRole = iota
	ContributorLiquidAdditional
	ContributorAir
)

const maxContributorRole = ContributorAir

type ModelStateField uint8

const (
	ModelStateOrientation ModelStateField = iota + 1
	ModelStateHalf
	ModelStateOpen
	ModelStateHinge
	ModelStateConnections
	ModelStateGrowth
	ModelStateLiquidDepth
	ModelStateFlags
)

const maxModelStateField = ModelStateFlags

const (
	modelFlagPowered uint32 = 1 << iota
	modelFlagPressed
	modelFlagAttached
	modelFlagHanging
	modelFlagHead
	modelFlagOccupied
	modelFlagInWall
	modelFlagUpper
)

type ModelState struct {
	Mask   uint8
	Values [8]uint32
}

func (s *ModelState) Set(field ModelStateField, value uint32) {
	if field == 0 || field > maxModelStateField {
		panic("invalid model state field")
	}
	bit := uint8(1 << (field - 1))
	s.Mask |= bit
	s.Values[field-1] = value
}

func (s ModelState) Get(field ModelStateField) (uint32, bool) {
	if field == 0 || field > maxModelStateField {
		return 0, false
	}
	bit := uint8(1 << (field - 1))
	return s.Values[field-1], s.Mask&bit != 0
}

type CollisionConfidence uint8

const (
	CollisionConfidenceNone CollisionConfidence = iota
	CollisionConfidenceCollisionOnly
	CollisionConfidenceReviewedVisibleBounds
)

const maxCollisionConfidence = CollisionConfidenceReviewedVisibleBounds

type CollisionBox struct {
	MinX int32
	MinY int32
	MinZ int32
	MaxX int32
	MaxY int32
	MaxZ int32
}

type CollisionSeed struct {
	ShapeID    uint16
	Confidence CollisionConfidence
	Boxes      []CollisionBox
}

const (
	ProvenancePMMP uint8 = 1 << iota
	ProvenanceDragonfly
	ProvenancePrismarine
	ProvenanceValentine
	allProvenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine | ProvenanceValentine
)

type RegistryMetadata struct {
	Protocol           uint32
	CanonicalNames     uint32
	CanonicalStates    uint32
	ValentineNames     uint32
	ValentineStates    uint32
	ValentineGapNames  uint32
	ValentineGapStates uint32
}

type ValentineAudit struct {
	CanonicalNames  int      `json:"canonical_names"`
	CanonicalStates int      `json:"canonical_states"`
	ValentineNames  int      `json:"valentine_names"`
	ValentineStates int      `json:"valentine_states"`
	GapNames        int      `json:"gap_names"`
	GapStates       int      `json:"gap_states"`
	Joined          int      `json:"joined"`
	Missing         int      `json:"missing"`
	Extra           int      `json:"extra"`
	Mismatched      int      `json:"mismatched"`
	MissingNames    []string `json:"missing_names"`
	MissingStates   []string `json:"missing_states"`
}

type GenerationReport struct {
	Protocol               uint32         `json:"protocol"`
	CanonicalNames         int            `json:"canonical_names"`
	CanonicalStates        int            `json:"canonical_states"`
	ValentineAudit         ValentineAudit `json:"valentine_audit"`
	PMMPPaletteSHA256      string         `json:"pmmp_palette_sha256"`
	PrismarineStateSHA256  string         `json:"prismarine_states_sha256"`
	PrismarineShapeSHA256  string         `json:"prismarine_shapes_sha256"`
	ValentinePaletteSHA256 string         `json:"valentine_palette_sha256"`
	ValentineBlocksSHA256  string         `json:"valentine_blocks_sha256"`
}

// Record is one serialized block-registry entry.
type Record struct {
	SequentialID    uint32
	NetworkHash     uint32
	Flags           uint8
	Name            string
	StateJSON       []byte
	ModelFamily     ModelFamily
	ContributorRole ContributorRole
	ModelState      ModelState
	FaceCoverage    uint8
	CollisionSeed   CollisionSeed
	Provenance      uint8
}

// BiomeRecord is one stable Dragonfly network biome registry entry.
type BiomeRecord struct {
	ID   uint32
	Name string
}

func main() {
	out := flag.String("out", "", "path to write the block registry")
	biomeOut := flag.String("biome-out", "", "optional path to write the biome registry")
	pmmpRoot := flag.String("pmmp", "", "pinned PMMP BedrockData directory")
	prismarineRoot := flag.String("prismarine", "", "pinned Prismarine minecraft-data directory")
	valentinePalette := flag.String("valentine-palette", "", "pinned Valentine block_palette.bin")
	valentineBlocks := flag.String("valentine-blocks", "", "pinned Valentine generated blocks.rs")
	flag.Parse()
	if *out == "" {
		fmt.Fprintln(os.Stderr, "registrygen: -out is required")
		os.Exit(2)
	}

	var records []Record
	var metadata RegistryMetadata
	var report GenerationReport
	var err error
	if *pmmpRoot == "" && *prismarineRoot == "" && *valentinePalette == "" && *valentineBlocks == "" {
		// The legacy source-free mode remains useful for focused Dragonfly
		// registry tests and biome-only generation. Release block registries use
		// the explicit four-source mode below.
		records, err = collect(world.DefaultBlockRegistry)
		metadata = defaultMetadata(records)
	} else {
		if *pmmpRoot == "" || *prismarineRoot == "" || *valentinePalette == "" || *valentineBlocks == "" {
			fmt.Fprintln(os.Stderr, "registrygen: -pmmp, -prismarine, -valentine-palette, and -valentine-blocks must be supplied together")
			os.Exit(2)
		}
		records, metadata, report, err = generateRegistry(*pmmpRoot, *prismarineRoot, *valentinePalette, *valentineBlocks, world.DefaultBlockRegistry)
	}
	if err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: %v\n", err)
		os.Exit(1)
	}
	encoded, err := encodeWithMetadata(metadata, records)
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
	digest := sha256.Sum256(encoded)
	shaPath := strings.TrimSuffix(*out, filepath.Ext(*out)) + ".sha256"
	if err := os.WriteFile(shaPath, []byte(fmt.Sprintf("%x\n", digest)), 0o644); err != nil {
		fmt.Fprintf(os.Stderr, "registrygen: write checksum: %v\n", err)
		os.Exit(1)
	}
	if report.Protocol != 0 {
		reportBytes, marshalErr := json.MarshalIndent(report, "", "  ")
		if marshalErr != nil {
			fmt.Fprintf(os.Stderr, "registrygen: encode report: %v\n", marshalErr)
			os.Exit(1)
		}
		fmt.Println(string(reportBytes))
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

func generateRegistry(pmmpRoot, prismarineRoot, valentinePalettePath, valentineBlocksPath string, registry world.BlockRegistry) ([]Record, RegistryMetadata, GenerationReport, error) {
	protocolPath := filepath.Join(pmmpRoot, "protocol_info.json")
	pmmpPalettePath := filepath.Join(pmmpRoot, "canonical_block_states.nbt")
	prismarineStatesPath := filepath.Join(prismarineRoot, "blockStates.json")
	prismarineShapesPath := filepath.Join(prismarineRoot, "blockCollisionShapes.json")

	protocol, err := readProtocol(protocolPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	if protocol != registryProtocol {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("PMMP protocol %d does not match required %d", protocol, registryProtocol)
	}
	pmmp, err := readNBTStates(pmmpPalettePath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("read PMMP palette: %w", err)
	}
	dragonfly, err := collectDragonflyStates(registry)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	prismarine, err := readPrismarineStates(prismarineStatesPath, prismarineShapesPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	joined, err := joinSources(pmmp, dragonfly, prismarine, canonicalStateHash)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	if len(joined) != 16_913 {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("canonical state count %d does not match required 16913", len(joined))
	}
	nameCount := uniqueNameCount(pmmp)
	if nameCount != 1_356 {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("canonical name count %d does not match required 1356", nameCount)
	}
	if err := validateSelectorCardinality(joined); err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	if err := promoteReviewedSelectorAliasCubes(joined); err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}

	valentine, err := readNBTStates(valentinePalettePath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("read Valentine palette: %w", err)
	}
	definitionCount, err := readValentineBlockCount(valentineBlocksPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	audit, err := auditValentineSubset(pmmp, valentine)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	if audit.ValentineStates != 15_845 || audit.ValentineNames != 1_321 || definitionCount != 1_321 || audit.GapStates != 1_068 || audit.GapNames != 35 || audit.Joined != 15_845 || audit.Missing != 1_068 || audit.Extra != 0 || audit.Mismatched != 0 {
		return nil, RegistryMetadata{}, GenerationReport{}, fmt.Errorf("Valentine audit cardinalities states=%d names=%d definitions=%d gaps=%d/%d, want 15845/1321/1321/1068/35", audit.ValentineStates, audit.ValentineNames, definitionCount, audit.GapStates, audit.GapNames)
	}
	valentineKeys, err := canonicalSourceIndex(valentine, "Valentine", canonicalStateHash)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	for i := range joined {
		key := canonicalRecordKey(joined[i].Name, joined[i].StateJSON)
		if _, ok := valentineKeys[key]; ok {
			joined[i].Provenance |= ProvenanceValentine
		}
	}
	if err := validateRealProvenance(joined, audit); err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}

	metadata := RegistryMetadata{
		Protocol:           protocol,
		CanonicalNames:     uint32(nameCount),
		CanonicalStates:    uint32(len(joined)),
		ValentineNames:     uint32(audit.ValentineNames),
		ValentineStates:    uint32(audit.ValentineStates),
		ValentineGapNames:  uint32(audit.GapNames),
		ValentineGapStates: uint32(audit.GapStates),
	}
	pmmpSHA, err := fileSHA256(pmmpPalettePath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	prismarineStateSHA, err := fileSHA256(prismarineStatesPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	prismarineShapeSHA, err := fileSHA256(prismarineShapesPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	valentinePaletteSHA, err := fileSHA256(valentinePalettePath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	valentineBlocksSHA, err := fileSHA256(valentineBlocksPath)
	if err != nil {
		return nil, RegistryMetadata{}, GenerationReport{}, err
	}
	report := GenerationReport{
		Protocol:               protocol,
		CanonicalNames:         nameCount,
		CanonicalStates:        len(joined),
		ValentineAudit:         audit,
		PMMPPaletteSHA256:      pmmpSHA,
		PrismarineStateSHA256:  prismarineStateSHA,
		PrismarineShapeSHA256:  prismarineShapeSHA,
		ValentinePaletteSHA256: valentinePaletteSHA,
		ValentineBlocksSHA256:  valentineBlocksSHA,
	}
	return joined, metadata, report, nil
}

func readProtocol(path string) (uint32, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return 0, err
	}
	if len(data) > 1<<20 {
		return 0, fmt.Errorf("protocol metadata exceeds 1 MiB")
	}
	var source struct {
		Version struct {
			Major    int    `json:"major"`
			Minor    int    `json:"minor"`
			Patch    int    `json:"patch"`
			Protocol uint32 `json:"protocol_version"`
		} `json:"version"`
	}
	if err := json.Unmarshal(data, &source); err != nil {
		return 0, err
	}
	if source.Version.Major != 1 || source.Version.Minor != 26 || source.Version.Patch != 30 {
		return 0, fmt.Errorf("PMMP game version %d.%d.%d does not match 1.26.30", source.Version.Major, source.Version.Minor, source.Version.Patch)
	}
	return source.Version.Protocol, nil
}

type nbtState struct {
	Name       string         `nbt:"name"`
	Properties map[string]any `nbt:"states"`
	Version    int32          `nbt:"version"`
}

func readNBTStates(path string) ([]SourceState, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if len(data) > 16<<20 {
		return nil, fmt.Errorf("NBT palette is %d bytes, exceeding 16 MiB", len(data))
	}
	return decodeNBTStates(data)
}

func decodeNBTStates(data []byte) ([]SourceState, error) {
	reader := bytes.NewReader(data)
	decoder := nbt.NewDecoder(reader)
	states := make([]SourceState, 0, 16_913)
	for reader.Len() != 0 && len(states) <= maxRecordCount {
		var state nbtState
		err := decoder.Decode(&state)
		if err != nil {
			return nil, fmt.Errorf("decode state %d: %w", len(states), err)
		}
		properties, err := typedProperties(state.Properties)
		if err != nil {
			return nil, fmt.Errorf("state %d %s: %w", len(states), state.Name, err)
		}
		states = append(states, SourceState{Name: canonicalBlockName(state.Name), Properties: properties, Ordinal: uint32(len(states))})
	}
	if len(states) > maxRecordCount {
		return nil, fmt.Errorf("NBT palette exceeds %d records", maxRecordCount)
	}
	return states, nil
}

type prismarineStateValue struct {
	Type  string          `json:"type"`
	Value json.RawMessage `json:"value"`
}

type prismarineState struct {
	Name   string                          `json:"name"`
	States map[string]prismarineStateValue `json:"states"`
}

type prismarineCollisionShapes struct {
	Blocks map[string][]uint16    `json:"blocks"`
	Shapes map[string][][]float64 `json:"shapes"`
}

func readPrismarineStates(statesPath, shapesPath string) ([]SourceState, error) {
	statesData, err := os.ReadFile(statesPath)
	if err != nil {
		return nil, err
	}
	if len(statesData) > 16<<20 {
		return nil, fmt.Errorf("Prismarine blockStates.json exceeds 16 MiB")
	}
	var rawStates []prismarineState
	if err := json.Unmarshal(statesData, &rawStates); err != nil {
		return nil, err
	}
	if len(rawStates) > maxRecordCount {
		return nil, fmt.Errorf("Prismarine states exceed %d records", maxRecordCount)
	}
	shapesData, err := os.ReadFile(shapesPath)
	if err != nil {
		return nil, err
	}
	if len(shapesData) > 4<<20 {
		return nil, fmt.Errorf("Prismarine collision shapes exceed 4 MiB")
	}
	var shapes prismarineCollisionShapes
	if err := json.Unmarshal(shapesData, &shapes); err != nil {
		return nil, err
	}
	occurrence := make(map[string]int)
	totals := make(map[string]int)
	for _, raw := range rawStates {
		totals[canonicalBlockName(raw.Name)]++
	}
	result := make([]SourceState, 0, len(rawStates))
	for ordinal, raw := range rawStates {
		name := canonicalBlockName(raw.Name)
		properties := make([]StateProperty, 0, len(raw.States))
		for key, value := range raw.States {
			scalar, err := parsePrismarineScalar(value)
			if err != nil {
				return nil, fmt.Errorf("Prismarine state %d %s property %s: %w", ordinal, name, key, err)
			}
			properties = append(properties, StateProperty{Name: key, Value: scalar})
		}
		shapeIDs, ok := shapes.Blocks[strings.TrimPrefix(name, "minecraft:")]
		if !ok || len(shapeIDs) == 0 {
			return nil, fmt.Errorf("Prismarine state %d %s has no collision-shape mapping", ordinal, name)
		}
		index := occurrence[name]
		occurrence[name] = index + 1
		shapeID, err := collisionShapeID(shapeIDs, index, totals[name])
		if err != nil {
			return nil, fmt.Errorf("Prismarine state %d %s: %w", ordinal, name, err)
		}
		seed, err := collisionSeed(shapeID, shapes.Shapes)
		if err != nil {
			return nil, fmt.Errorf("Prismarine state %d %s: %w", ordinal, name, err)
		}
		result = append(result, SourceState{Name: name, Properties: properties, Ordinal: uint32(ordinal), CollisionSeed: seed})
	}
	return result, nil
}

func collisionShapeID(ids []uint16, occurrence, total int) (uint16, error) {
	if len(ids) == 0 {
		return 0, errors.New("collision shape mapping is empty")
	}
	if len(ids) == 1 {
		if occurrence < 0 || occurrence >= total {
			return 0, fmt.Errorf("collision occurrence %d is outside %d states", occurrence, total)
		}
		return ids[0], nil
	}
	if len(ids) != total {
		return 0, fmt.Errorf("collision variant cardinality %d does not match %d states", len(ids), total)
	}
	if occurrence < 0 || occurrence >= len(ids) {
		return 0, fmt.Errorf("collision occurrence %d is outside %d variants", occurrence, len(ids))
	}
	return ids[occurrence], nil
}

func parsePrismarineScalar(value prismarineStateValue) (TypedScalar, error) {
	switch value.Type {
	case "byte":
		var number int64
		if err := json.Unmarshal(value.Value, &number); err != nil || number < 0 || number > math.MaxUint8 {
			return TypedScalar{}, fmt.Errorf("invalid byte value %s", value.Value)
		}
		return TypedScalar{Kind: ScalarByte, Byte: byte(number)}, nil
	case "int":
		var number int64
		if err := json.Unmarshal(value.Value, &number); err != nil || number < math.MinInt32 || number > math.MaxInt32 {
			return TypedScalar{}, fmt.Errorf("invalid int value %s", value.Value)
		}
		return TypedScalar{Kind: ScalarInt, Int: int32(number)}, nil
	case "string":
		var text string
		if err := json.Unmarshal(value.Value, &text); err != nil {
			return TypedScalar{}, fmt.Errorf("invalid string value %s", value.Value)
		}
		return TypedScalar{Kind: ScalarString, String: text}, nil
	default:
		return TypedScalar{}, fmt.Errorf("unsupported scalar type %q", value.Type)
	}
}

func collisionSeed(id uint16, shapes map[string][][]float64) (CollisionSeed, error) {
	raw, ok := shapes[strconv.FormatUint(uint64(id), 10)]
	if !ok {
		return CollisionSeed{}, fmt.Errorf("collision shape %d is missing", id)
	}
	if len(raw) > maxCollisionBoxesPerRecord {
		return CollisionSeed{}, fmt.Errorf("collision shape %d has %d boxes, exceeding %d", id, len(raw), maxCollisionBoxesPerRecord)
	}
	boxes := make([]CollisionBox, 0, len(raw))
	for index, coords := range raw {
		if len(coords) != 6 {
			return CollisionSeed{}, fmt.Errorf("collision shape %d box %d has %d coordinates", id, index, len(coords))
		}
		fixed := [6]int32{}
		for i, coordinate := range coords {
			scaled := math.Round(coordinate * collisionFixedScale)
			if math.IsNaN(coordinate) || math.IsInf(coordinate, 0) || scaled < math.MinInt32 || scaled > math.MaxInt32 || math.Abs(scaled-coordinate*collisionFixedScale) > 1e-6 {
				return CollisionSeed{}, fmt.Errorf("collision shape %d coordinate %g is not bounded exact 1/100000000 fixed point", id, coordinate)
			}
			fixed[i] = int32(scaled)
		}
		boxes = append(boxes, CollisionBox{MinX: fixed[0], MinY: fixed[1], MinZ: fixed[2], MaxX: fixed[3], MaxY: fixed[4], MaxZ: fixed[5]})
	}
	return CollisionSeed{ShapeID: id, Confidence: CollisionConfidenceCollisionOnly, Boxes: boxes}, nil
}

func readValentineBlockCount(path string) (int, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return 0, err
	}
	if len(data) > 16<<20 {
		return 0, fmt.Errorf("Valentine blocks.rs exceeds 16 MiB")
	}
	marker := []byte("impl BlockDef for ")
	definitions := bytes.Count(data, marker)
	constantMarker := []byte("pub const BLOCK_COUNT: usize = ")
	start := bytes.Index(data, constantMarker)
	if start < 0 {
		return 0, fmt.Errorf("Valentine blocks.rs has no BLOCK_COUNT")
	}
	start += len(constantMarker)
	end := start
	for end < len(data) && data[end] >= '0' && data[end] <= '9' {
		end++
	}
	declared, err := strconv.Atoi(string(data[start:end]))
	if err != nil || declared != definitions {
		return 0, fmt.Errorf("Valentine BLOCK_COUNT %q does not match %d definitions", data[start:end], definitions)
	}
	return definitions, nil
}

func fileSHA256(path string) (string, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return "", fmt.Errorf("hash source %s: %w", path, err)
	}
	digest := sha256.Sum256(data)
	return fmt.Sprintf("%x", digest), nil
}

func typedProperties(properties map[string]any) ([]StateProperty, error) {
	result := make([]StateProperty, 0, len(properties))
	for name, raw := range properties {
		var scalar TypedScalar
		switch value := raw.(type) {
		case bool:
			scalar.Kind = ScalarByte
			if value {
				scalar.Byte = 1
			}
		case uint8:
			scalar = TypedScalar{Kind: ScalarByte, Byte: value}
		case int8:
			scalar = TypedScalar{Kind: ScalarByte, Byte: byte(value)}
		case int32:
			scalar = TypedScalar{Kind: ScalarInt, Int: value}
		case string:
			scalar = TypedScalar{Kind: ScalarString, String: value}
		default:
			return nil, fmt.Errorf("property %s has unsupported scalar type %T", name, raw)
		}
		result = append(result, StateProperty{Name: name, Value: scalar})
	}
	return result, nil
}

func canonicalBlockName(name string) string {
	if strings.ContainsRune(name, ':') {
		return name
	}
	return "minecraft:" + name
}

type canonicalScalar struct {
	Type  string `json:"type"`
	Value any    `json:"value"`
}

func canonicalTypedState(properties []StateProperty) ([]byte, error) {
	sorted := append([]StateProperty(nil), properties...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].Name < sorted[j].Name })
	canonical := make(map[string]canonicalScalar, len(sorted))
	for _, property := range sorted {
		if property.Name == "" {
			return nil, errors.New("state property name is empty")
		}
		if _, exists := canonical[property.Name]; exists {
			return nil, fmt.Errorf("duplicate state property %q", property.Name)
		}
		var scalar canonicalScalar
		switch property.Value.Kind {
		case ScalarByte:
			scalar = canonicalScalar{Type: "byte", Value: property.Value.Byte}
		case ScalarInt:
			scalar = canonicalScalar{Type: "int", Value: property.Value.Int}
		case ScalarString:
			scalar = canonicalScalar{Type: "string", Value: property.Value.String}
		default:
			return nil, fmt.Errorf("property %q has unknown scalar type %d", property.Name, property.Value.Kind)
		}
		canonical[property.Name] = scalar
	}
	return json.Marshal(canonical)
}

func canonicalRecordKey(name string, state []byte) string {
	return canonicalBlockName(name) + "\x00" + string(state)
}

func canonicalStateHash(key []byte) uint64 {
	digest := sha256.Sum256(key)
	return binary.LittleEndian.Uint64(digest[:8])
}

func canonicalSourceIndex(states []SourceState, source string, hasher func([]byte) uint64) (map[string]SourceState, error) {
	if len(states) > maxRecordCount {
		return nil, fmt.Errorf("%s has %d states, exceeding %d", source, len(states), maxRecordCount)
	}
	index := make(map[string]SourceState, len(states))
	hashes := make(map[uint64]string, len(states))
	for _, state := range states {
		state.Name = canonicalBlockName(state.Name)
		canonical, err := canonicalTypedState(state.Properties)
		if err != nil {
			return nil, fmt.Errorf("%s state %s: %w", source, state.Name, err)
		}
		key := canonicalRecordKey(state.Name, canonical)
		if _, exists := index[key]; exists {
			return nil, fmt.Errorf("duplicate canonical key in %s: %s %s", source, state.Name, canonical)
		}
		hash := hasher([]byte(key))
		if previous, exists := hashes[hash]; exists && previous != key {
			return nil, fmt.Errorf("canonical state hash collision in %s between %q and %q", source, previous, key)
		}
		hashes[hash] = key
		index[key] = state
	}
	return index, nil
}

func indexNames(states []SourceState) map[string]int {
	result := make(map[string]int)
	for _, state := range states {
		result[canonicalBlockName(state.Name)]++
	}
	return result
}

func joinSources(pmmp, dragonfly, prismarine []SourceState, hasher func([]byte) uint64) ([]Record, error) {
	pmmpIndex, err := canonicalSourceIndex(pmmp, "PMMP", hasher)
	if err != nil {
		return nil, err
	}
	dragonflyIndex, err := canonicalSourceIndex(dragonfly, "Dragonfly", hasher)
	if err != nil {
		return nil, err
	}
	prismarineIndex, err := canonicalSourceIndex(prismarine, "Prismarine", hasher)
	if err != nil {
		return nil, err
	}
	dragonflyNames := indexNames(dragonfly)
	prismarineNames := indexNames(prismarine)
	for key, source := range pmmpIndex {
		if _, ok := dragonflyIndex[key]; !ok {
			if dragonflyNames[source.Name] != 0 {
				return nil, fmt.Errorf("typed state mismatch for %s: PMMP state %q missing from Dragonfly", source.Name, key)
			}
			return nil, fmt.Errorf("canonical state %q missing from Dragonfly", key)
		}
		if _, ok := prismarineIndex[key]; !ok {
			if prismarineNames[source.Name] != 0 {
				return nil, fmt.Errorf("typed state mismatch for %s: PMMP state %q missing from Prismarine", source.Name, key)
			}
			return nil, fmt.Errorf("canonical state %q missing from Prismarine", key)
		}
	}
	for key := range dragonflyIndex {
		if _, ok := pmmpIndex[key]; !ok {
			return nil, fmt.Errorf("canonical state %q extra in Dragonfly", key)
		}
	}
	for key := range prismarineIndex {
		if _, ok := pmmpIndex[key]; !ok {
			return nil, fmt.Errorf("canonical state %q extra in Prismarine", key)
		}
	}

	keys := make([]string, 0, len(pmmpIndex))
	for key := range pmmpIndex {
		keys = append(keys, key)
	}
	sort.Slice(keys, func(i, j int) bool {
		left, right := dragonflyIndex[keys[i]], dragonflyIndex[keys[j]]
		if left.Ordinal != right.Ordinal {
			return left.Ordinal < right.Ordinal
		}
		return keys[i] < keys[j]
	})
	result := make([]Record, 0, len(keys))
	for _, key := range keys {
		pmmpState := pmmpIndex[key]
		dragonflyState := dragonflyIndex[key]
		prismarineState := prismarineIndex[key]
		record, err := classifyRecord(pmmpState)
		if err != nil {
			return nil, err
		}
		record.SequentialID = dragonflyState.Ordinal
		record.NetworkHash = dragonflyState.NetworkHash
		record.Flags = dragonflyState.Flags
		record.CollisionSeed = prismarineState.CollisionSeed
		record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
		finalizeGeometryFacts(&record)
		result = append(result, record)
	}
	return result, nil
}

func collisionSeedIsUnit(seed CollisionSeed) bool {
	if seed.Confidence == CollisionConfidenceNone || len(seed.Boxes) != 1 {
		return false
	}
	box := seed.Boxes[0]
	return box == (CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000})
}

func chiseledBookshelfCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 1 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 1 &&
		seed.Boxes[0] == (CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000})
}

func finalizeGeometryFacts(record *Record) {
	if record.ModelFamily == ModelFamilyChiseledBookshelf {
		if chiseledBookshelfCollisionIsExact(record.CollisionSeed) {
			record.Flags = flagCubeGeometry | flagOccludesFullFace
			record.FaceCoverage = 0x3f
		} else {
			record.Flags &^= flagCubeGeometry | flagOccludesFullFace
			record.FaceCoverage = 0
		}
		return
	}
	if (record.ModelFamily == ModelFamilyStatue || record.ModelFamily == ModelFamilyCuboid) && record.CollisionSeed.Confidence != CollisionConfidenceNone && !collisionSeedIsUnit(record.CollisionSeed) {
		record.Flags &^= flagCubeGeometry | flagOccludesFullFace
		record.FaceCoverage = 0
		return
	}
	if record.Flags&flagLeafModel != 0 {
		record.ModelFamily = ModelFamilyLeaves
	} else if record.Flags&flagCubeGeometry != 0 && record.ModelFamily == ModelFamilyUnknown {
		record.ModelFamily = ModelFamilyCube
	}
	if record.Flags&flagOccludesFullFace != 0 || (record.ModelFamily == ModelFamilyCube && collisionSeedIsUnit(record.CollisionSeed)) {
		record.FaceCoverage = 0x3f
	}
}

func auditValentineSubset(canonical, valentine []SourceState) (ValentineAudit, error) {
	canonicalIndex, err := canonicalSourceIndex(canonical, "canonical", canonicalStateHash)
	if err != nil {
		return ValentineAudit{}, err
	}
	valentineIndex, err := canonicalSourceIndex(valentine, "Valentine", canonicalStateHash)
	if err != nil {
		return ValentineAudit{}, err
	}
	canonicalNames := indexNames(canonical)
	valentineNames := indexNames(valentine)
	for key, state := range valentineIndex {
		if _, ok := canonicalIndex[key]; !ok {
			if canonicalNames[state.Name] != 0 {
				return ValentineAudit{}, fmt.Errorf("Valentine typed state %q does not match canonical source", key)
			}
			return ValentineAudit{}, fmt.Errorf("Valentine state %q is outside canonical source", key)
		}
	}
	expectedOrder := make([]string, 0, len(valentineIndex))
	for _, state := range canonical {
		encoded, err := canonicalTypedState(state.Properties)
		if err != nil {
			return ValentineAudit{}, err
		}
		key := canonicalRecordKey(state.Name, encoded)
		if _, ok := valentineIndex[key]; ok {
			expectedOrder = append(expectedOrder, key)
		}
	}
	actualOrder := make([]string, 0, len(valentine))
	for _, state := range valentine {
		encoded, err := canonicalTypedState(state.Properties)
		if err != nil {
			return ValentineAudit{}, err
		}
		actualOrder = append(actualOrder, canonicalRecordKey(state.Name, encoded))
	}
	if len(expectedOrder) != len(actualOrder) {
		return ValentineAudit{}, fmt.Errorf("Valentine overlap order cardinality %d does not match %d", len(actualOrder), len(expectedOrder))
	}
	for index := range expectedOrder {
		if expectedOrder[index] != actualOrder[index] {
			return ValentineAudit{}, fmt.Errorf("Valentine overlap order differs at %d: got %q want %q", index, actualOrder[index], expectedOrder[index])
		}
	}
	missingNames := make([]string, 0)
	for name := range canonicalNames {
		if valentineNames[name] == 0 {
			missingNames = append(missingNames, name)
		}
	}
	sort.Strings(missingNames)
	missingStates := make([]string, 0, len(canonicalIndex)-len(valentineIndex))
	for key := range canonicalIndex {
		if _, ok := valentineIndex[key]; !ok {
			missingStates = append(missingStates, key)
		}
	}
	sort.Strings(missingStates)
	return ValentineAudit{
		CanonicalNames:  len(canonicalNames),
		CanonicalStates: len(canonicalIndex),
		ValentineNames:  len(valentineNames),
		ValentineStates: len(valentineIndex),
		GapNames:        len(missingNames),
		GapStates:       len(missingStates),
		Joined:          len(valentineIndex),
		Missing:         len(missingStates),
		Extra:           0,
		Mismatched:      0,
		MissingNames:    missingNames,
		MissingStates:   missingStates,
	}, nil
}

func validateRealProvenance(records []Record, audit ValentineAudit) error {
	if len(records) != audit.CanonicalStates {
		return fmt.Errorf("provenance record count %d does not match canonical %d", len(records), audit.CanonicalStates)
	}
	canonicalBits := ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
	valentineCount := 0
	for index, record := range records {
		if record.Provenance&canonicalBits != canonicalBits {
			return fmt.Errorf("record %d has incomplete canonical provenance %#x", index, record.Provenance)
		}
		if record.Provenance&ProvenanceValentine != 0 {
			valentineCount++
		}
	}
	if valentineCount != audit.ValentineStates || valentineCount != audit.Joined {
		return fmt.Errorf("Valentine provenance count %d does not match audit %d/%d", valentineCount, audit.ValentineStates, audit.Joined)
	}
	return nil
}

func uniqueNameCount(states []SourceState) int {
	return len(indexNames(states))
}

func classifyRecord(state SourceState) (Record, error) {
	state.Name = canonicalBlockName(state.Name)
	sort.Slice(state.Properties, func(i, j int) bool { return state.Properties[i].Name < state.Properties[j].Name })
	canonical, err := canonicalTypedState(state.Properties)
	if err != nil {
		return Record{}, err
	}
	record := Record{Name: state.Name, StateJSON: canonical, ContributorRole: ContributorPrimary}
	name := strings.TrimPrefix(state.Name, "minecraft:")
	switch {
	case name == "air":
		record.ModelFamily = ModelFamilyAir
		record.ContributorRole = ContributorAir
	case name == "water" || name == "flowing_water" || name == "lava" || name == "flowing_lava":
		record.ModelFamily = ModelFamilyLiquid
		record.ContributorRole = ContributorLiquidAdditional
	case strings.Contains(name, "copper_golem_statue"):
		record.ModelFamily = ModelFamilyStatue
	case name == "barrier":
		record.ModelFamily = ModelFamilyInvisible
	case name == "dragon_egg":
		record.ModelFamily = ModelFamilyDecorative
	case name == "soul_sand" || name == "mud":
		record.ModelFamily = ModelFamilyCuboid
	case isReviewedSelectorAliasCubeName(name), isGlazedTerracottaName(name):
		record.ModelFamily = ModelFamilyCube
	case strings.Contains(name, "trapdoor"):
		record.ModelFamily = ModelFamilyTrapdoor
	case strings.HasSuffix(name, "_door") || name == "wooden_door":
		record.ModelFamily = ModelFamilyDoor
	case strings.HasSuffix(name, "_stairs"):
		record.ModelFamily = ModelFamilyStair
	case strings.Contains(name, "slab"):
		record.ModelFamily = ModelFamilySlab
	case strings.Contains(name, "fence_gate"):
		record.ModelFamily = ModelFamilyGate
	case strings.HasSuffix(name, "_wall") || name == "cobblestone_wall":
		record.ModelFamily = ModelFamilyWall
	case strings.HasSuffix(name, "_fence") || name == "fence" || name == "nether_brick_fence":
		record.ModelFamily = ModelFamilyFence
	case strings.Contains(name, "glass_pane") || strings.HasSuffix(name, "_pane") || strings.HasSuffix(name, "_bars"):
		record.ModelFamily = ModelFamilyPane
	case strings.HasSuffix(name, "_bed") || name == "bed":
		record.ModelFamily = ModelFamilyBed
	case strings.Contains(name, "chest"):
		record.ModelFamily = ModelFamilyChest
	case strings.Contains(name, "sign"):
		record.ModelFamily = ModelFamilySign
	case strings.Contains(name, "rail"):
		record.ModelFamily = ModelFamilyRail
	case isTorchName(name):
		record.ModelFamily = ModelFamilyTorch
	case name == "lever":
		record.ModelFamily = ModelFamilyLever
	case strings.HasSuffix(name, "_button") || name == "stone_button":
		record.ModelFamily = ModelFamilyButton
	case strings.Contains(name, "pressure_plate"):
		record.ModelFamily = ModelFamilyPressurePlate
	case strings.HasSuffix(name, "_carpet") || name == "carpet":
		record.ModelFamily = ModelFamilyCarpet
	case name == "snow_layer" || name == "leaf_litter":
		record.ModelFamily = ModelFamilyLayer
	case isAquaticName(name):
		record.ModelFamily = ModelFamilyAquatic
	case name == "cocoa":
		record.ModelFamily = ModelFamilyCocoa
	case isCropName(name):
		record.ModelFamily = ModelFamilyCrop
	case name == "wildflowers" || name == "pink_petals":
		record.ModelFamily = ModelFamilyFlowerBed
	case name == "vine":
		record.ModelFamily = ModelFamilyVine
	case name == "glow_lichen":
		record.ModelFamily = ModelFamilyGlowLichen
	case name == "sculk_vein":
		record.ModelFamily = ModelFamilySculkVein
	case name == "chiseled_bookshelf":
		books, direction, err := chiseledBookshelfSelectors(state.Properties)
		if err != nil {
			return Record{}, err
		}
		record.ModelFamily = ModelFamilyChiseledBookshelf
		record.ModelState.Set(ModelStateConnections, books)
		record.ModelState.Set(ModelStateOrientation, direction)
	case name == "resin_clump":
		connections, err := resinClumpSelector(state.Properties)
		if err != nil {
			return Record{}, err
		}
		record.ModelFamily = ModelFamilyResinClump
		record.ModelState.Set(ModelStateConnections, connections)
	case name == "cactus":
		age, err := cactusAgeSelector(state.Properties)
		if err != nil {
			return Record{}, err
		}
		record.ModelFamily = ModelFamilyCuboid
		record.ModelState.Set(ModelStateGrowth, age)
	case name == "cake":
		bite, err := cakeBiteSelector(state.Properties)
		if err != nil {
			return Record{}, err
		}
		record.ModelFamily = ModelFamilyCuboid
		record.ModelState.Set(ModelStateGrowth, bite)
	case isCrossName(name):
		record.ModelFamily = ModelFamilyCross
	}
	var connections uint32
	var hasConnections bool
	var variantFlags uint32
	var hasVariantFlags bool
	for _, property := range state.Properties {
		propertyName := strings.TrimPrefix(property.Name, "minecraft:")
		if propertyName == "redstone_signal" && record.ModelFamily == ModelFamilyPressurePlate {
			value, ok := scalarUint(property.Value)
			if !ok || value > 15 {
				return Record{}, fmt.Errorf("pressure plate redstone_signal is outside 0..15")
			}
			if value != 0 {
				variantFlags |= modelFlagPressed
			}
			hasVariantFlags = true
		}
		switch propertyName {
		case "weirdo_direction", "direction", "facing_direction", "ground_sign_direction", "cardinal_direction", "pillar_axis", "torch_facing_direction", "rail_direction", "lever_direction":
			if record.ModelFamily != ModelFamilySign {
				if value, ok := orientationUint(propertyName, property.Value); ok {
					record.ModelState.Set(ModelStateOrientation, value)
				}
			}
		}
		if value, ok := scalarUint(property.Value); ok {
			switch propertyName {
			case "vertical_half", "upside_down_bit", "top_slot_bit":
				record.ModelState.Set(ModelStateHalf, value)
			case "open_bit":
				record.ModelState.Set(ModelStateOpen, value)
			case "door_hinge_bit":
				record.ModelState.Set(ModelStateHinge, value)
			case "multi_face_direction_bits", "vine_direction_bits":
				connections = value
				hasConnections = true
			case "wall_post_bit":
				connections = (connections &^ (1 << 8)) | ((value & 1) << 8)
				hasConnections = true
			case "growth", "growth_stage", "age", "growing_plant_age", "kelp_age":
				record.ModelState.Set(ModelStateGrowth, value)
			case "liquid_depth":
				record.ModelState.Set(ModelStateLiquidDepth, value)
			case "rail_data_bit":
				if value != 0 {
					variantFlags |= modelFlagPowered
				}
				hasVariantFlags = true
			case "button_pressed_bit":
				if value != 0 {
					variantFlags |= modelFlagPressed
				}
				hasVariantFlags = true
			case "attached_bit":
				if value != 0 {
					variantFlags |= modelFlagAttached
				}
				hasVariantFlags = true
			case "hanging":
				if value != 0 {
					variantFlags |= modelFlagHanging
				}
				hasVariantFlags = true
			case "head_piece_bit":
				if value != 0 {
					variantFlags |= modelFlagHead
				}
				hasVariantFlags = true
			case "occupied_bit":
				if value != 0 {
					variantFlags |= modelFlagOccupied
				}
				hasVariantFlags = true
			case "in_wall_bit":
				if value != 0 {
					variantFlags |= modelFlagInWall
				}
				hasVariantFlags = true
			case "upper_block_bit":
				if value != 0 {
					variantFlags |= modelFlagUpper
				}
				hasVariantFlags = true
			}
		}
		if strings.HasPrefix(propertyName, "wall_connection_type_") && property.Value.Kind == ScalarString {
			var value uint32
			switch property.Value.String {
			case "none":
				value = 0
			case "short":
				value = 1
			case "tall":
				value = 2
			default:
				continue
			}
			var shift uint
			switch strings.TrimPrefix(propertyName, "wall_connection_type_") {
			case "north":
				shift = 0
			case "east":
				shift = 2
			case "south":
				shift = 4
			case "west":
				shift = 6
			default:
				continue
			}
			connections = (connections &^ (3 << shift)) | (value << shift)
			hasConnections = true
		}
	}
	if hasConnections {
		record.ModelState.Set(ModelStateConnections, connections)
	}
	if hasVariantFlags {
		record.ModelState.Set(ModelStateFlags, variantFlags)
	}
	if record.ModelFamily == ModelFamilySign {
		orientation, err := signOrientation(name, state.Properties)
		if err != nil {
			return Record{}, err
		}
		record.ModelState.Set(ModelStateOrientation, orientation)
	}
	if record.ModelFamily == ModelFamilySlab && strings.Contains(name, "double") {
		record.ModelState.Set(ModelStateHalf, 2)
	}
	return record, nil
}

func chiseledBookshelfSelectors(properties []StateProperty) (books uint32, direction uint32, err error) {
	if len(properties) != 2 {
		return 0, 0, fmt.Errorf("chiseled_bookshelf requires exactly books_stored:int and direction:int")
	}
	seenBooks, seenDirection := false, false
	for _, property := range properties {
		name := property.Name
		if property.Value.Kind != ScalarInt {
			return 0, 0, fmt.Errorf("chiseled_bookshelf %s must be an int selector", name)
		}
		switch name {
		case "books_stored":
			if seenBooks || property.Value.Int < 0 || property.Value.Int > 63 {
				return 0, 0, fmt.Errorf("chiseled_bookshelf books_stored must be unique and inside 0..63")
			}
			seenBooks = true
			books = uint32(property.Value.Int)
		case "direction":
			if seenDirection || property.Value.Int < 0 || property.Value.Int > 3 {
				return 0, 0, fmt.Errorf("chiseled_bookshelf direction must be unique and inside 0..3")
			}
			seenDirection = true
			direction = uint32(property.Value.Int)
		default:
			return 0, 0, fmt.Errorf("chiseled_bookshelf has unsupported selector %q", name)
		}
	}
	if !seenBooks || !seenDirection {
		return 0, 0, fmt.Errorf("chiseled_bookshelf requires books_stored and direction selectors")
	}
	return books, direction, nil
}

func resinClumpSelector(properties []StateProperty) (uint32, error) {
	if len(properties) != 1 || properties[0].Name != "multi_face_direction_bits" {
		return 0, fmt.Errorf("resin_clump requires exactly multi_face_direction_bits:int")
	}
	property := properties[0]
	if property.Value.Kind != ScalarInt || property.Value.Int < 0 || property.Value.Int > 63 {
		return 0, fmt.Errorf("resin_clump multi_face_direction_bits must be an int inside 0..63")
	}
	return uint32(property.Value.Int), nil
}

func cactusAgeSelector(properties []StateProperty) (uint32, error) {
	if len(properties) != 1 || properties[0].Name != "age" {
		return 0, fmt.Errorf("cactus requires exactly age:int")
	}
	property := properties[0]
	if property.Value.Kind != ScalarInt || property.Value.Int < 0 || property.Value.Int > 15 {
		return 0, fmt.Errorf("cactus age must be an int inside 0..15")
	}
	return uint32(property.Value.Int), nil
}

func cakeBiteSelector(properties []StateProperty) (uint32, error) {
	if len(properties) != 1 || properties[0].Name != "bite_counter" {
		return 0, fmt.Errorf("cake requires exactly bite_counter:int")
	}
	property := properties[0]
	if property.Value.Kind != ScalarInt || property.Value.Int < 0 || property.Value.Int > 6 {
		return 0, fmt.Errorf("cake bite_counter must be an int inside 0..6")
	}
	return uint32(property.Value.Int), nil
}

func exactCanonicalInt(raw json.RawMessage, maximum int32) (uint32, bool) {
	var tagged map[string]json.RawMessage
	if err := json.Unmarshal(raw, &tagged); err != nil || len(tagged) != 2 {
		return 0, false
	}
	var kind string
	if err := json.Unmarshal(tagged["type"], &kind); err != nil || kind != "int" {
		return 0, false
	}
	var value int32
	if err := json.Unmarshal(tagged["value"], &value); err != nil || value < 0 || value > maximum {
		return 0, false
	}
	return uint32(value), true
}

func chiseledBookshelfCanonicalSelectors(stateJSON []byte) (books uint32, direction uint32, ok bool) {
	var state map[string]json.RawMessage
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != 2 {
		return 0, 0, false
	}
	books, booksOK := exactCanonicalInt(state["books_stored"], 63)
	direction, directionOK := exactCanonicalInt(state["direction"], 3)
	return books, direction, booksOK && directionOK
}

func resinClumpCanonicalSelector(stateJSON []byte) (uint32, bool) {
	var state map[string]json.RawMessage
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != 1 {
		return 0, false
	}
	return exactCanonicalInt(state["multi_face_direction_bits"], 63)
}

func cactusCanonicalAge(stateJSON []byte) (uint32, bool) {
	var state map[string]json.RawMessage
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != 1 {
		return 0, false
	}
	return exactCanonicalInt(state["age"], 15)
}

func cakeCanonicalBite(stateJSON []byte) (uint32, bool) {
	decoder := json.NewDecoder(bytes.NewReader(stateJSON))
	opening, err := decoder.Token()
	if err != nil || opening != json.Delim('{') || !decoder.More() {
		return 0, false
	}
	key, err := decoder.Token()
	if err != nil || key != "bite_counter" {
		return 0, false
	}
	bite, ok := exactCakeTaggedInt(decoder)
	if !ok || decoder.More() {
		return 0, false
	}
	closing, err := decoder.Token()
	if err != nil || closing != json.Delim('}') {
		return 0, false
	}
	var trailing any
	if err := decoder.Decode(&trailing); err != io.EOF {
		return 0, false
	}
	return bite, true
}

func exactCakeTaggedInt(decoder *json.Decoder) (uint32, bool) {
	opening, err := decoder.Token()
	if err != nil || opening != json.Delim('{') {
		return 0, false
	}
	var kind string
	var value int32
	seenKind, seenValue := false, false
	for decoder.More() {
		key, err := decoder.Token()
		if err != nil {
			return 0, false
		}
		switch key {
		case "type":
			if seenKind || decoder.Decode(&kind) != nil {
				return 0, false
			}
			seenKind = true
		case "value":
			if seenValue || decoder.Decode(&value) != nil {
				return 0, false
			}
			seenValue = true
		default:
			return 0, false
		}
	}
	closing, err := decoder.Token()
	if err != nil || closing != json.Delim('}') || !seenKind || !seenValue || kind != "int" || value < 0 || value > 6 {
		return 0, false
	}
	return uint32(value), true
}

func signOrientation(name string, properties []StateProperty) (uint32, error) {
	values := make(map[string]uint32, 2)
	for _, property := range properties {
		propertyName := strings.TrimPrefix(property.Name, "minecraft:")
		if propertyName != "facing_direction" && propertyName != "ground_sign_direction" {
			continue
		}
		value, ok := scalarUint(property.Value)
		if !ok {
			return 0, fmt.Errorf("%s %s is not an unsigned selector", name, propertyName)
		}
		values[propertyName] = value
	}
	if strings.Contains(name, "hanging_sign") {
		facing, hasFacing := values["facing_direction"]
		rotation, hasRotation := values["ground_sign_direction"]
		if !hasFacing || facing > 5 || !hasRotation || rotation > 15 {
			return 0, fmt.Errorf("%s requires facing_direction 0..5 and ground_sign_direction 0..15", name)
		}
		// Preserve both selectors. `hanging` chooses which one controls visible
		// geometry: wall-hanging signs use the facing nibble, while ceiling
		// hanging signs use the 16-way ground rotation nibble.
		return rotation | (facing << 4), nil
	}
	if strings.HasSuffix(name, "standing_sign") || name == "standing_sign" {
		rotation, ok := values["ground_sign_direction"]
		if !ok || rotation > 15 {
			return 0, fmt.Errorf("%s requires ground_sign_direction 0..15", name)
		}
		return rotation, nil
	}
	if strings.HasSuffix(name, "wall_sign") || name == "wall_sign" {
		facing, ok := values["facing_direction"]
		if !ok || facing > 5 {
			return 0, fmt.Errorf("%s requires facing_direction 0..5", name)
		}
		return facing, nil
	}
	return 0, fmt.Errorf("unsupported sign family %s", name)
}

func scalarUint(scalar TypedScalar) (uint32, bool) {
	switch scalar.Kind {
	case ScalarByte:
		return uint32(scalar.Byte), true
	case ScalarInt:
		if scalar.Int >= 0 {
			return uint32(scalar.Int), true
		}
	case ScalarString:
		switch scalar.String {
		case "bottom", "lower", "false", "south":
			return 0, true
		case "top", "upper", "true", "west":
			return 1, true
		case "north":
			return 2, true
		case "east":
			return 3, true
		}
	}
	return 0, false
}

func orientationUint(property string, scalar TypedScalar) (uint32, bool) {
	if scalar.Kind != ScalarString {
		return scalarUint(scalar)
	}
	if property == "lever_direction" {
		values := map[string]uint32{"down_east_west": 0, "east": 1, "west": 2, "south": 3, "north": 4, "up_north_south": 5, "up_east_west": 6, "down_north_south": 7}
		value, ok := values[scalar.String]
		return value, ok
	}
	if property == "torch_facing_direction" {
		values := map[string]uint32{"unknown": 0, "west": 1, "east": 2, "north": 3, "south": 4, "top": 5}
		value, ok := values[scalar.String]
		return value, ok
	}
	if property == "pillar_axis" {
		values := map[string]uint32{"x": 0, "y": 1, "z": 2}
		value, ok := values[scalar.String]
		return value, ok
	}
	return scalarUint(scalar)
}

func isCropName(name string) bool {
	switch name {
	case "wheat", "carrots", "potatoes", "beetroot", "nether_wart", "sweet_berry_bush", "torchflower_crop", "pitcher_crop", "melon_stem", "pumpkin_stem":
		return true
	default:
		return false
	}
}

func isCrossName(name string) bool {
	if name == "chorus_flower" {
		return false
	}
	if name == "short_grass" || name == "tall_grass" || name == "short_dry_grass" || name == "tall_dry_grass" || name == "fern" || name == "large_fern" || name == "deadbush" || name == "bush" || name == "red_flower" || name == "yellow_flower" || name == "dandelion" || name == "poppy" || name == "blue_orchid" || name == "allium" || name == "azure_bluet" || name == "oxeye_daisy" || name == "cornflower" || name == "lily_of_the_valley" || name == "wither_rose" || name == "sunflower" || name == "lilac" || name == "rose_bush" || name == "peony" || name == "brown_mushroom" || name == "red_mushroom" || name == "crimson_fungus" || name == "warped_fungus" || name == "crimson_roots" || name == "warped_roots" || name == "nether_sprouts" || name == "mangrove_propagule" || name == "hanging_roots" || name == "pale_hanging_moss" || name == "firefly_bush" || name == "reeds" || name == "weeping_vines" || name == "twisting_vines" || strings.HasPrefix(name, "cave_vines") || name == "web" || name == "fire" || name == "soul_fire" || name == "torchflower" {
		return true
	}
	return !strings.Contains(name, "flower_pot") && (strings.HasSuffix(name, "_flower") || strings.HasSuffix(name, "_sapling"))
}

func isAquaticName(name string) bool {
	return name == "seagrass" || name == "tall_seagrass" || name == "kelp" || name == "kelp_plant" || (strings.Contains(name, "coral") && !strings.Contains(name, "coral_block"))
}

func isTorchName(name string) bool {
	return name == "torch" || name == "copper_torch" || name == "soul_torch" || name == "redstone_torch" || name == "unlit_redstone_torch" || name == "underwater_torch" || strings.HasPrefix(name, "colored_torch_")
}

func isGlazedTerracottaName(name string) bool {
	return strings.HasSuffix(name, "_glazed_terracotta")
}

func isReviewedSelectorAliasCubeName(name string) bool {
	switch name {
	case "bone_block", "hay_block", "chiseled_quartz_block", "purpur_block", "quartz_block", "smooth_quartz", "tnt":
		return true
	default:
		return false
	}
}

func selectorAliasCubeCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 1 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 1 &&
		seed.Boxes[0] == (CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000})
}

func exactCanonicalString(raw json.RawMessage) (string, bool) {
	var tagged map[string]json.RawMessage
	if err := json.Unmarshal(raw, &tagged); err != nil || len(tagged) != 2 {
		return "", false
	}
	var kind string
	if err := json.Unmarshal(tagged["type"], &kind); err != nil || kind != "string" {
		return "", false
	}
	var value string
	if err := json.Unmarshal(tagged["value"], &value); err != nil {
		return "", false
	}
	return value, true
}

func exactCanonicalByte(raw json.RawMessage) (byte, bool) {
	var tagged map[string]json.RawMessage
	if err := json.Unmarshal(raw, &tagged); err != nil || len(tagged) != 2 {
		return 0, false
	}
	var kind string
	if err := json.Unmarshal(tagged["type"], &kind); err != nil || kind != "byte" {
		return 0, false
	}
	var value uint8
	if err := json.Unmarshal(tagged["value"], &value); err != nil || value > 1 {
		return 0, false
	}
	return value, true
}

func selectorAliasAxisState(stateJSON []byte, hasDeprecated bool) (axisIndex, deprecated uint32, ok bool) {
	var state map[string]json.RawMessage
	wantProperties := 1
	if hasDeprecated {
		wantProperties = 2
	}
	if err := json.Unmarshal(stateJSON, &state); err != nil || len(state) != wantProperties {
		return 0, 0, false
	}
	axis, axisOK := exactCanonicalString(state["pillar_axis"])
	if !axisOK {
		return 0, 0, false
	}
	switch axis {
	case "y":
		axisIndex = 0
	case "x":
		axisIndex = 1
	case "z":
		axisIndex = 2
	default:
		return 0, 0, false
	}
	if hasDeprecated {
		var deprecatedOK bool
		deprecated, deprecatedOK = exactCanonicalInt(state["deprecated"], 3)
		if !deprecatedOK {
			return 0, 0, false
		}
	}
	return axisIndex, deprecated, true
}

// promoteReviewedSelectorAliasCubes admits only the complete, native-reviewed
// selector products below. Validation is deliberately atomic: no BREG record is
// changed until every present reviewed product has passed exact state, ID,
// projection, collision, and pre-promotion geometry checks.
func promoteReviewedSelectorAliasCubes(records []Record) error {
	type product struct {
		base          uint32
		cardinality   int
		hasDeprecated bool
		tnt           bool
	}
	products := map[string]product{
		"minecraft:hay_block":             {base: 2907, cardinality: 12, hasDeprecated: true},
		"minecraft:bone_block":            {base: 6465, cardinality: 12, hasDeprecated: true},
		"minecraft:quartz_block":          {base: 5442, cardinality: 3},
		"minecraft:smooth_quartz":         {base: 7081, cardinality: 3},
		"minecraft:chiseled_quartz_block": {base: 14685, cardinality: 3},
		"minecraft:purpur_block":          {base: 15344, cardinality: 3},
		"minecraft:tnt":                   {base: 13112, cardinality: 2, tnt: true},
	}
	groups := make(map[string][]int, len(products))
	for index := range records {
		if _, reviewed := products[records[index].Name]; reviewed {
			groups[records[index].Name] = append(groups[records[index].Name], index)
		}
	}
	if len(groups) == 0 {
		return nil
	}
	promote := make([]int, 0, 27)
	for name, spec := range products {
		indexes := groups[name]
		if len(indexes) != spec.cardinality {
			return fmt.Errorf("%s selector cardinality is %d, want %d", name, len(indexes), spec.cardinality)
		}
		seen := make([]bool, spec.cardinality)
		for _, index := range indexes {
			record := &records[index]
			if record.ModelFamily != ModelFamilyCube || record.ContributorRole != ContributorPrimary {
				return fmt.Errorf("%s state %d has invalid family or role", name, record.SequentialID)
			}
			if record.FaceCoverage != 0x3f || !selectorAliasCubeCollisionIsExact(record.CollisionSeed) {
				return fmt.Errorf("%s state %d lacks exact unit cube evidence", name, record.SequentialID)
			}
			var offset uint32
			var sourceSolid bool
			if spec.tnt {
				var state map[string]json.RawMessage
				if err := json.Unmarshal(record.StateJSON, &state); err != nil || len(state) != 1 {
					return fmt.Errorf("%s state %d has invalid typed selector", name, record.SequentialID)
				}
				exploded, ok := exactCanonicalByte(state["explode_bit"])
				if !ok || record.ModelState.Mask != 0 {
					return fmt.Errorf("%s state %d has invalid typed selector projection", name, record.SequentialID)
				}
				offset = uint32(exploded)
				sourceSolid = exploded == 0
			} else {
				axisIndex, deprecated, ok := selectorAliasAxisState(record.StateJSON, spec.hasDeprecated)
				orientation, hasOrientation := record.ModelState.Get(ModelStateOrientation)
				wantOrientation := [3]uint32{1, 0, 2}[axisIndex]
				if !ok || !hasOrientation || orientation != wantOrientation || record.ModelState.Mask != uint8(1<<(ModelStateOrientation-1)) {
					return fmt.Errorf("%s state %d has invalid typed selector projection", name, record.SequentialID)
				}
				stride := uint32(1)
				if spec.hasDeprecated {
					stride = 4
				}
				offset = axisIndex*stride + deprecated
				if spec.hasDeprecated {
					sourceSolid = deprecated == 0
				} else {
					sourceSolid = axisIndex == 0
				}
			}
			if offset >= uint32(spec.cardinality) || record.SequentialID != spec.base+offset {
				return fmt.Errorf("%s state %d does not match canonical ID formula", name, record.SequentialID)
			}
			if seen[offset] {
				return fmt.Errorf("%s has duplicate selector offset %d", name, offset)
			}
			seen[offset] = true
			wantFlags := uint8(0)
			if sourceSolid {
				wantFlags = flagCubeGeometry | flagOccludesFullFace
			}
			if record.Flags != wantFlags {
				return fmt.Errorf("%s state %d has unexpected pre-promotion flags %#x", name, record.SequentialID, record.Flags)
			}
			if !sourceSolid {
				promote = append(promote, index)
			}
		}
		for offset, present := range seen {
			if !present {
				return fmt.Errorf("%s selector product is missing offset %d", name, offset)
			}
		}
	}
	for _, index := range promote {
		records[index].Flags = flagCubeGeometry | flagOccludesFullFace
		records[index].FaceCoverage = 0x3f
	}
	return nil
}

func validateSelectorCardinality(records []Record) error {
	groups := make(map[string][]Record)
	for _, record := range records {
		groups[record.Name] = append(groups[record.Name], record)
	}
	for name, group := range groups {
		if name == "minecraft:chiseled_bookshelf" {
			if err := validateChiseledBookshelfProduct(group); err != nil {
				return err
			}
		}
		if name == "minecraft:resin_clump" {
			if err := validateResinClumpProduct(group); err != nil {
				return err
			}
		}
		if name == "minecraft:cactus" {
			if err := validateCactusProduct(group); err != nil {
				return err
			}
		}
		if name == "minecraft:cake" {
			if err := validateCakeProduct(group); err != nil {
				return err
			}
		}
		values := make(map[string]map[string]struct{})
		for _, record := range group {
			var state map[string]canonicalScalar
			if err := json.Unmarshal(record.StateJSON, &state); err != nil {
				return fmt.Errorf("selector %s has invalid canonical state: %w", name, err)
			}
			for property, value := range state {
				encoded, _ := json.Marshal(value)
				if values[property] == nil {
					values[property] = make(map[string]struct{})
				}
				values[property][string(encoded)] = struct{}{}
			}
		}
		expected := 1
		for _, distinct := range values {
			if expected > maxRecordCount/len(distinct) {
				return fmt.Errorf("selector cardinality for %s overflows bound", name)
			}
			expected *= len(distinct)
		}
		if expected != len(group) {
			return fmt.Errorf("selector cardinality for %s is %d states but typed product is %d", name, len(group), expected)
		}
	}
	return nil
}

func validateChiseledBookshelfProduct(records []Record) error {
	if len(records) != 256 {
		return fmt.Errorf("chiseled_bookshelf selector cardinality is %d, want 256", len(records))
	}
	seen := make(map[[2]uint32]struct{}, 256)
	for _, record := range records {
		if record.ModelFamily != ModelFamilyChiseledBookshelf || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("chiseled_bookshelf state %d has invalid family or role", record.SequentialID)
		}
		if record.Flags != flagCubeGeometry|flagOccludesFullFace || !chiseledBookshelfCollisionIsExact(record.CollisionSeed) {
			return fmt.Errorf("chiseled_bookshelf state %d lacks exact solid unit geometry evidence", record.SequentialID)
		}
		canonicalBooks, canonicalDirection, hasCanonicalState := chiseledBookshelfCanonicalSelectors(record.StateJSON)
		books, hasBooks := record.ModelState.Get(ModelStateConnections)
		direction, hasDirection := record.ModelState.Get(ModelStateOrientation)
		if !hasCanonicalState || canonicalBooks != books || canonicalDirection != direction || !hasBooks || !hasDirection || record.ModelState.Mask != uint8(1<<(ModelStateOrientation-1)|1<<(ModelStateConnections-1)) || books > 63 || direction > 3 {
			return fmt.Errorf("chiseled_bookshelf state %d has invalid typed selector projection", record.SequentialID)
		}
		if record.SequentialID != 1605+books*4+direction {
			return fmt.Errorf("chiseled_bookshelf state %d does not match canonical ID formula", record.SequentialID)
		}
		key := [2]uint32{books, direction}
		if _, exists := seen[key]; exists {
			return fmt.Errorf("chiseled_bookshelf duplicate selector %v", key)
		}
		seen[key] = struct{}{}
	}
	return nil
}

func resinClumpCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 0 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 0
}

func validateResinClumpProduct(records []Record) error {
	if len(records) != 64 {
		return fmt.Errorf("resin_clump selector cardinality is %d, want 64", len(records))
	}
	seen := [64]bool{}
	for _, record := range records {
		if record.ModelFamily != ModelFamilyResinClump || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("resin_clump state %d has invalid family or role", record.SequentialID)
		}
		if record.Flags != 0 || record.FaceCoverage != 0 || !resinClumpCollisionIsExact(record.CollisionSeed) {
			return fmt.Errorf("resin_clump state %d has invalid empty geometry evidence", record.SequentialID)
		}
		canonical, hasCanonical := resinClumpCanonicalSelector(record.StateJSON)
		connections, hasConnections := record.ModelState.Get(ModelStateConnections)
		if !hasCanonical || !hasConnections || canonical != connections || connections > 63 || record.ModelState.Mask != uint8(1<<(ModelStateConnections-1)) {
			return fmt.Errorf("resin_clump state %d has invalid typed selector projection", record.SequentialID)
		}
		if record.SequentialID != 2930+connections {
			return fmt.Errorf("resin_clump state %d does not match canonical ID formula", record.SequentialID)
		}
		if seen[connections] {
			return fmt.Errorf("resin_clump duplicate selector %d", connections)
		}
		seen[connections] = true
	}
	for mask, present := range seen {
		if !present {
			return fmt.Errorf("resin_clump selector product is missing mask %d", mask)
		}
	}
	return nil
}

func cactusCollisionIsExact(seed CollisionSeed) bool {
	return seed.ShapeID == 84 &&
		seed.Confidence == CollisionConfidenceCollisionOnly &&
		len(seed.Boxes) == 1 &&
		seed.Boxes[0] == (CollisionBox{
			MinX: 6_250_000, MaxX: 93_750_000,
			MinY: 0, MaxY: 100_000_000,
			MinZ: 6_250_000, MaxZ: 93_750_000,
		})
}

func validateCactusProduct(records []Record) error {
	if len(records) != 16 {
		return fmt.Errorf("cactus selector cardinality is %d, want 16", len(records))
	}
	seen := [16]bool{}
	for _, record := range records {
		if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("cactus state %d has invalid family or role", record.SequentialID)
		}
		if record.Flags != 0 || record.FaceCoverage != 0 || !cactusCollisionIsExact(record.CollisionSeed) {
			return fmt.Errorf("cactus state %d has invalid inset geometry evidence", record.SequentialID)
		}
		canonical, hasCanonical := cactusCanonicalAge(record.StateJSON)
		age, hasAge := record.ModelState.Get(ModelStateGrowth)
		if !hasCanonical || !hasAge || canonical != age || age > 15 || record.ModelState.Mask != uint8(1<<(ModelStateGrowth-1)) {
			return fmt.Errorf("cactus state %d has invalid typed age projection", record.SequentialID)
		}
		if record.SequentialID != 13606+age {
			return fmt.Errorf("cactus state %d does not match canonical ID formula", record.SequentialID)
		}
		if seen[age] {
			return fmt.Errorf("cactus duplicate age %d", age)
		}
		seen[age] = true
	}
	for age, present := range seen {
		if !present {
			return fmt.Errorf("cactus selector product is missing age %d", age)
		}
	}
	return nil
}

func cakeCollisionIsExact(seed CollisionSeed, bite uint32) bool {
	if bite > 6 || seed.ShapeID != uint16(89+bite) ||
		seed.Confidence != CollisionConfidenceCollisionOnly || len(seed.Boxes) != 1 {
		return false
	}
	wantMinX := [...]int32{6_250_000, 18_750_000, 31_250_000, 43_750_000, 56_250_000, 68_750_000, 81_250_000}
	return seed.Boxes[0] == (CollisionBox{
		MinX: wantMinX[bite], MaxX: 93_750_000,
		MinY: 0, MaxY: 50_000_000,
		MinZ: 6_250_000, MaxZ: 93_750_000,
	})
}

func validateCakeProduct(records []Record) error {
	if len(records) != 7 {
		return fmt.Errorf("cake selector cardinality is %d, want 7", len(records))
	}
	seen := [7]bool{}
	for _, record := range records {
		if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			return fmt.Errorf("cake state %d has invalid family or role", record.SequentialID)
		}
		canonical, hasCanonical := cakeCanonicalBite(record.StateJSON)
		bite, hasBite := record.ModelState.Get(ModelStateGrowth)
		if !hasCanonical || !hasBite || canonical != bite || bite > 6 ||
			record.ModelState.Mask != uint8(1<<(ModelStateGrowth-1)) {
			return fmt.Errorf("cake state %d has invalid typed bite projection", record.SequentialID)
		}
		if record.Flags != 0 || record.FaceCoverage != 0 || !cakeCollisionIsExact(record.CollisionSeed, bite) {
			return fmt.Errorf("cake state %d has invalid exact geometry evidence", record.SequentialID)
		}
		if record.SequentialID != 14055+bite {
			return fmt.Errorf("cake state %d does not match canonical ID formula", record.SequentialID)
		}
		if seen[bite] {
			return fmt.Errorf("cake duplicate bite %d", bite)
		}
		seen[bite] = true
	}
	for bite, present := range seen {
		if !present {
			return fmt.Errorf("cake selector product is missing bite %d", bite)
		}
	}
	return nil
}

func collectDragonflyStates(registry world.BlockRegistry) ([]SourceState, error) {
	if registry == nil {
		return nil, errors.New("block registry is nil")
	}
	registry.Finalize()
	blocks := registry.Blocks()
	if len(blocks) > maxRecordCount {
		return nil, fmt.Errorf("too many records: %d exceeds %d", len(blocks), maxRecordCount)
	}
	states := make([]SourceState, 0, len(blocks))
	for rid, value := range blocks {
		name, properties := value.EncodeBlock()
		networkHash, ok := registry.RuntimeIDToHash(uint32(rid))
		if !ok {
			return nil, fmt.Errorf("runtime ID %d has no network hash", rid)
		}
		typed, err := typedProperties(properties)
		if err != nil {
			return nil, fmt.Errorf("runtime ID %d: %w", rid, err)
		}
		states = append(states, SourceState{Name: name, Properties: typed, Ordinal: uint32(rid), NetworkHash: networkHash, Flags: classifyFlags(value)})
	}
	return states, nil
}

func collect(registry world.BlockRegistry) ([]Record, error) {
	states, err := collectDragonflyStates(registry)
	if err != nil {
		return nil, err
	}
	records := make([]Record, 0, len(states))
	for _, state := range states {
		record, err := classifyRecord(state)
		if err != nil {
			return nil, fmt.Errorf("classify runtime ID %d: %w", state.Ordinal, err)
		}
		record.SequentialID = state.Ordinal
		record.NetworkHash = state.NetworkHash
		record.Flags = state.Flags
		record.Provenance = ProvenanceDragonfly
		finalizeGeometryFacts(&record)
		records = append(records, record)
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

func defaultMetadata(records []Record) RegistryMetadata {
	names := make(map[string]struct{}, len(records))
	for _, record := range records {
		names[record.Name] = struct{}{}
	}
	return RegistryMetadata{
		Protocol:           registryProtocol,
		CanonicalNames:     uint32(len(names)),
		CanonicalStates:    uint32(len(records)),
		ValentineGapNames:  uint32(len(names)),
		ValentineGapStates: uint32(len(records)),
	}
}

func encode(records []Record) ([]byte, error) {
	compat := append([]Record(nil), records...)
	for i := range compat {
		if compat[i].Provenance == 0 {
			compat[i].Provenance = ProvenanceDragonfly
		}
	}
	return encodeWithMetadata(defaultMetadata(compat), compat)
}

func encodeWithMetadata(metadata RegistryMetadata, records []Record) ([]byte, error) {
	if len(records) > maxRecordCount {
		return nil, fmt.Errorf("too many records: %d exceeds %d", len(records), maxRecordCount)
	}
	if metadata.Protocol != registryProtocol {
		return nil, fmt.Errorf("registry protocol %d does not match %d", metadata.Protocol, registryProtocol)
	}
	if metadata.CanonicalStates != uint32(len(records)) {
		return nil, fmt.Errorf("metadata canonical state count %d does not match %d records", metadata.CanonicalStates, len(records))
	}
	if metadata.ValentineStates+metadata.ValentineGapStates != metadata.CanonicalStates {
		return nil, fmt.Errorf("Valentine state span %d+%d does not match canonical %d", metadata.ValentineStates, metadata.ValentineGapStates, metadata.CanonicalStates)
	}
	if metadata.ValentineNames+metadata.ValentineGapNames != metadata.CanonicalNames {
		return nil, fmt.Errorf("Valentine name span %d+%d does not match canonical %d", metadata.ValentineNames, metadata.ValentineGapNames, metadata.CanonicalNames)
	}

	sorted := append([]Record(nil), records...)
	sort.Slice(sorted, func(i, j int) bool {
		return sorted[i].SequentialID < sorted[j].SequentialID
	})

	seenSequentialIDs := make(map[uint32]struct{}, len(sorted))
	seenNetworkHashes := make(map[uint32]struct{}, len(sorted))
	canonicalNames := make(map[string]struct{}, len(sorted))
	valentineNames := make(map[string]struct{}, len(sorted))
	valentineStates := 0
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
		if record.ModelFamily > maxModelFamily {
			return nil, fmt.Errorf("invalid model family %d for sequential ID %d", record.ModelFamily, record.SequentialID)
		}
		if record.ContributorRole > maxContributorRole {
			return nil, fmt.Errorf("invalid contributor role %d for sequential ID %d", record.ContributorRole, record.SequentialID)
		}
		if record.ModelState.Mask&^uint8((1<<maxModelStateField)-1) != 0 {
			return nil, fmt.Errorf("invalid model-state mask %#x for sequential ID %d", record.ModelState.Mask, record.SequentialID)
		}
		for field, value := range record.ModelState.Values {
			present := record.ModelState.Mask&(1<<field) != 0
			if !present && value != 0 {
				return nil, fmt.Errorf("absent model-state field %d is non-zero for sequential ID %d", field+1, record.SequentialID)
			}
		}
		if record.FaceCoverage&^uint8(0x3f) != 0 {
			return nil, fmt.Errorf("invalid face coverage %#x for sequential ID %d", record.FaceCoverage, record.SequentialID)
		}
		if record.CollisionSeed.Confidence > maxCollisionConfidence {
			return nil, fmt.Errorf("invalid collision confidence %d for sequential ID %d", record.CollisionSeed.Confidence, record.SequentialID)
		}
		if len(record.CollisionSeed.Boxes) > maxCollisionBoxesPerRecord {
			return nil, fmt.Errorf("collision boxes %d exceed %d for sequential ID %d", len(record.CollisionSeed.Boxes), maxCollisionBoxesPerRecord, record.SequentialID)
		}
		if record.CollisionSeed.Confidence == CollisionConfidenceNone && len(record.CollisionSeed.Boxes) != 0 {
			return nil, fmt.Errorf("collision boxes without confidence for sequential ID %d", record.SequentialID)
		}
		if record.Provenance&^allProvenance != 0 || record.Provenance == 0 {
			return nil, fmt.Errorf("invalid provenance %#x for sequential ID %d", record.Provenance, record.SequentialID)
		}
		canonicalNames[record.Name] = struct{}{}
		if record.Provenance&ProvenanceValentine != 0 {
			valentineStates++
			valentineNames[record.Name] = struct{}{}
		}
	}
	if len(canonicalNames) != int(metadata.CanonicalNames) {
		return nil, fmt.Errorf("canonical provenance name count %d does not match metadata %d", len(canonicalNames), metadata.CanonicalNames)
	}
	if valentineStates != int(metadata.ValentineStates) {
		return nil, fmt.Errorf("Valentine provenance state count %d does not match metadata %d", valentineStates, metadata.ValentineStates)
	}
	if len(valentineNames) != int(metadata.ValentineNames) {
		return nil, fmt.Errorf("Valentine provenance name count %d does not match metadata %d", len(valentineNames), metadata.ValentineNames)
	}

	encoded := make([]byte, 0, len(registryHeader)+7*4)
	encoded = append(encoded, registryHeader...)
	encoded = binary.LittleEndian.AppendUint32(encoded, metadata.Protocol)
	encoded = binary.LittleEndian.AppendUint32(encoded, metadata.CanonicalNames)
	encoded = binary.LittleEndian.AppendUint32(encoded, metadata.CanonicalStates)
	encoded = binary.LittleEndian.AppendUint32(encoded, metadata.ValentineNames)
	encoded = binary.LittleEndian.AppendUint32(encoded, metadata.ValentineStates)
	encoded = binary.LittleEndian.AppendUint32(encoded, metadata.ValentineGapNames)
	encoded = binary.LittleEndian.AppendUint32(encoded, metadata.ValentineGapStates)
	for _, record := range sorted {
		encoded = binary.LittleEndian.AppendUint32(encoded, record.SequentialID)
		encoded = binary.LittleEndian.AppendUint32(encoded, record.NetworkHash)
		encoded = append(encoded, record.Flags)
		encoded = append(encoded, byte(record.ModelFamily))
		encoded = append(encoded, byte(record.ContributorRole))
		encoded = append(encoded, record.ModelState.Mask)
		encoded = append(encoded, record.FaceCoverage)
		encoded = append(encoded, byte(record.CollisionSeed.Confidence))
		encoded = append(encoded, record.Provenance)
		encoded = append(encoded, byte(len(record.CollisionSeed.Boxes)))
		encoded = binary.LittleEndian.AppendUint16(encoded, record.CollisionSeed.ShapeID)
		encoded = binary.LittleEndian.AppendUint16(encoded, uint16(len(record.Name)))
		encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(record.StateJSON)))
		for _, value := range record.ModelState.Values {
			encoded = binary.LittleEndian.AppendUint32(encoded, value)
		}
		for _, box := range record.CollisionSeed.Boxes {
			encoded = binary.LittleEndian.AppendUint32(encoded, uint32(box.MinX))
			encoded = binary.LittleEndian.AppendUint32(encoded, uint32(box.MinY))
			encoded = binary.LittleEndian.AppendUint32(encoded, uint32(box.MinZ))
			encoded = binary.LittleEndian.AppendUint32(encoded, uint32(box.MaxX))
			encoded = binary.LittleEndian.AppendUint32(encoded, uint32(box.MaxY))
			encoded = binary.LittleEndian.AppendUint32(encoded, uint32(box.MaxZ))
		}
		encoded = append(encoded, record.Name...)
		encoded = append(encoded, record.StateJSON...)
	}
	return encoded, nil
}
