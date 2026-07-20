package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"image/color"
	"maps"
	"math"
	"math/rand"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/block/model"
	"github.com/df-mc/dragonfly/server/world"
	"github.com/sandertv/gophertunnel/minecraft/nbt"
)

type fixtureLightRegistry struct {
	emission map[uint32]uint8
	filter   map[uint32]uint8
}

type fixtureBlockItem struct {
	itemName  string
	metadata  int16
	blockName string
	state     map[string]any
}

func (f fixtureBlockItem) EncodeItem() (string, int16)           { return f.itemName, f.metadata }
func (f fixtureBlockItem) EncodeBlock() (string, map[string]any) { return f.blockName, f.state }
func (fixtureBlockItem) Hash() (uint64, uint64)                  { return 1, 1 }
func (fixtureBlockItem) Model() world.BlockModel                 { return nil }

func TestGenerateBlockItemRoutesPreservesExactMetadataAndState(t *testing.T) {
	state, err := canonicalTypedState([]StateProperty{{Name: "variant", Value: TypedScalar{Kind: ScalarInt, Int: 3}}})
	if err != nil {
		t.Fatal(err)
	}
	records := []bregLightIdentity{{SequentialID: 0, Name: "minecraft:air", StateJSON: []byte("{}")}, {SequentialID: 1, Name: "minecraft:test_block", StateJSON: state}}
	breg := []byte("reviewed BREG bytes")
	table, err := generateBlockItemRouteTable([]world.Item{fixtureBlockItem{itemName: "minecraft:test_item", metadata: -1, blockName: "minecraft:test_block", state: map[string]any{"variant": int32(3)}}}, records, breg)
	if err != nil {
		t.Fatalf("generate routes: %v", err)
	}
	if len(table.Routes) != 1 || table.Routes[0].Identifier != "minecraft:test_item" || table.Routes[0].Metadata != math.MaxUint32 || table.Routes[0].BlockVisual != 1 || !bytes.Equal(table.Routes[0].BlockState, state) {
		t.Fatalf("unexpected exact route: %#v", table.Routes)
	}
	if table.BREGSHA256 != fmt.Sprintf("%x", sha256.Sum256(breg)) || table.DragonflyModuleSum == "" {
		t.Fatalf("missing provenance: %#v", table)
	}
}

func TestGenerateBlockItemRoutesRejectsDuplicateMissingAndAmbiguousStates(t *testing.T) {
	item := fixtureBlockItem{itemName: "minecraft:test_item", blockName: "minecraft:test_block"}
	record := bregLightIdentity{SequentialID: 0, Name: "minecraft:test_block", StateJSON: []byte("{}")}
	if _, err := generateBlockItemRouteTable([]world.Item{item, item}, []bregLightIdentity{record}, []byte("breg")); err == nil {
		t.Fatal("duplicate route accepted")
	}
	if _, err := generateBlockItemRouteTable([]world.Item{item}, nil, []byte("breg")); err == nil {
		t.Fatal("missing state accepted")
	}
	if _, err := generateBlockItemRouteTable([]world.Item{item}, []bregLightIdentity{record, record}, []byte("breg")); err == nil {
		t.Fatal("ambiguous state accepted")
	}
}

func TestCheckedInBlockItemRoutesMatchPinnedGenerator(t *testing.T) {
	breg, err := os.ReadFile("../../crates/assets/data/block-registry-v1001.bin")
	if err != nil {
		t.Fatal(err)
	}
	records, err := readBREG1003LightIdentities(breg)
	if err != nil {
		t.Fatal(err)
	}
	table, err := generateBlockItemRouteTable(world.Items(), records, breg)
	if err != nil {
		t.Fatal(err)
	}
	encoded, err := json.MarshalIndent(table, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	encoded = append(encoded, '\n')
	want, err := os.ReadFile("../../crates/assets/data/block-item-routes-v1001.json")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(encoded, want) {
		t.Fatal("checked-in block item routes are stale")
	}
}

func (r fixtureLightRegistry) LightBlock(runtimeID uint32) uint8 {
	return r.emission[runtimeID]
}

func (r fixtureLightRegistry) FilteringBlock(runtimeID uint32) uint8 {
	return r.filter[runtimeID]
}

func TestEncodeLREG1001BindsExactBREGAndSortsBySequentialID(t *testing.T) {
	records := []Record{testRecord(2, 30), testRecord(0, 10), testRecord(1, 20)}
	registry := fixtureLightRegistry{
		emission: map[uint32]uint8{0: 1, 1: 2, 2: 3},
		filter:   map[uint32]uint8{0: 4, 1: 5, 2: 6},
	}
	breg := []byte("exact BREG1003 fixture bytes")

	first, err := encodeLightRegistry(breg, records, registry)
	if err != nil {
		t.Fatalf("encode light registry: %v", err)
	}
	second, err := encodeLightRegistry(breg, slices.Clone(records), registry)
	if err != nil {
		t.Fatalf("repeat light registry: %v", err)
	}
	if !bytes.Equal(first, second) {
		t.Fatal("LREG1001 encoding is not deterministic")
	}
	if got := string(first[:8]); got != "LREG1001" {
		t.Fatalf("magic = %q", got)
	}
	if got := binary.LittleEndian.Uint32(first[8:12]); got != registryProtocol {
		t.Fatalf("protocol = %d", got)
	}
	if got := binary.LittleEndian.Uint32(first[12:16]); got != 3 {
		t.Fatalf("record count = %d", got)
	}
	wantBREGHash := sha256.Sum256(breg)
	if !bytes.Equal(first[16:48], wantBREGHash[:]) {
		t.Fatalf("BREG digest = %x, want %x", first[16:48], wantBREGHash)
	}
	if got, want := first[48:51], []byte{0x41, 0x52, 0x63}; !bytes.Equal(got, want) {
		t.Fatalf("packed state bytes = %x, want %x", got, want)
	}
	wantDigest := sha256.Sum256(first[:51])
	if !bytes.Equal(first[51:], wantDigest[:]) {
		t.Fatalf("LREG digest = %x, want %x", first[51:], wantDigest)
	}
}

func TestEncodePREG1001BindsExactBREGAndCanonicalOrder(t *testing.T) {
	records := []Record{testRecord(2, 30), testRecord(0, 10), testRecord(1, 20)}
	for i := range records {
		records[i].CollisionSeed = CollisionSeed{Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}}}
	}
	breg := []byte("exact BREG1003 fixture bytes")
	properties := map[string]PMMPLightProperties{}
	for _, record := range records {
		properties[record.Name] = PMMPLightProperties{Friction: 0.6}
	}
	physics, err := buildPhysicsRecords(records, syntheticPhysicsSources(records, properties))
	if err != nil {
		t.Fatalf("build physics records: %v", err)
	}
	first, err := encodePhysicsRegistry(breg, physics, 3)
	if err != nil {
		t.Fatalf("encode physics registry: %v", err)
	}
	second, err := encodePhysicsRegistry(breg, physics, 3)
	if err != nil {
		t.Fatalf("repeat physics registry: %v", err)
	}
	if !bytes.Equal(first, second) {
		t.Fatal("PREG1001 encoding is not deterministic")
	}
	if got := string(first[:8]); got != "PREG1001" {
		t.Fatalf("magic = %q", got)
	}
	if got := binary.LittleEndian.Uint32(first[8:12]); got != registryProtocol {
		t.Fatalf("protocol = %d", got)
	}
	if got := binary.LittleEndian.Uint32(first[12:16]); got != 3 {
		t.Fatalf("record count = %d", got)
	}
	wantBREGHash := sha256.Sum256(breg)
	if !bytes.Equal(first[16:48], wantBREGHash[:]) {
		t.Fatalf("BREG digest = %x, want %x", first[16:48], wantBREGHash)
	}
	if got := binary.LittleEndian.Uint32(first[48:52]); got != 0 {
		t.Fatalf("first sequential ID = %d", got)
	}
}

func TestPREG1001ProductionCardinalityIsExactly16913(t *testing.T) {
	records := make([]PhysicsRecord, physicsRecordCount)
	for index := range records {
		records[index] = PhysicsRecord{
			SequentialID:        uint32(index),
			NetworkHash:         uint32(index) + 1,
			FrictionQ1E8:        defaultFrictionQ1E8,
			HorizontalSpeedQ1E8: defaultSpeedQ1E8,
			VerticalSpeedQ1E8:   defaultSpeedQ1E8,
			Flags:               physicsFlagPassable,
		}
	}
	encoded, err := encodePhysicsRegistry([]byte("production BREG fixture"), records, physicsRecordCount)
	if err != nil {
		t.Fatalf("encode production cardinality: %v", err)
	}
	if got := binary.LittleEndian.Uint32(encoded[12:16]); got != physicsRecordCount {
		t.Fatalf("record count = %d, want %d", got, physicsRecordCount)
	}
}

func TestPREG1001RejectsIncompleteOrInvalidFacts(t *testing.T) {
	valid := []PhysicsRecord{
		{SequentialID: 0, NetworkHash: 10, FrictionQ1E8: 60_000_000, HorizontalSpeedQ1E8: 100_000_000, VerticalSpeedQ1E8: 100_000_000, Flags: physicsFlagPassable},
		{SequentialID: 1, NetworkHash: 20, FrictionQ1E8: 60_000_000, HorizontalSpeedQ1E8: 100_000_000, VerticalSpeedQ1E8: 100_000_000, Flags: physicsFlagWater | physicsFlagPassable, FluidHeightQ1E8: 100_000_000},
	}
	for _, test := range []struct {
		name   string
		mutate func([]PhysicsRecord) []PhysicsRecord
		want   string
	}{
		{"missing", func(records []PhysicsRecord) []PhysicsRecord { return records[:1] }, "count"},
		{"extra", func(records []PhysicsRecord) []PhysicsRecord { return append(records, records[1]) }, "count"},
		{"duplicate", func(records []PhysicsRecord) []PhysicsRecord { records[1].SequentialID = 0; return records }, "sequential"},
		{"water lava", func(records []PhysicsRecord) []PhysicsRecord { records[1].Flags |= physicsFlagLava; return records }, "water and lava"},
		{"bubble without water", func(records []PhysicsRecord) []PhysicsRecord {
			records[0].SurfaceResponse = SurfaceBubbleUp
			return records
		}, "bubble"},
		{"unknown enum", func(records []PhysicsRecord) []PhysicsRecord {
			records[0].SurfaceResponse = SurfaceResponse(255)
			return records
		}, "surface response"},
		{"unknown flags", func(records []PhysicsRecord) []PhysicsRecord {
			records[0].Flags |= physicsFlagReserved
			return records
		}, "flags"},
		{"zero friction", func(records []PhysicsRecord) []PhysicsRecord { records[0].FrictionQ1E8 = 0; return records }, "friction"},
		{"too many boxes", func(records []PhysicsRecord) []PhysicsRecord {
			records[0].Boxes = make([]CollisionBox, maxPhysicsBoxes+1)
			return records
		}, "box count"},
		{"inverted box", func(records []PhysicsRecord) []PhysicsRecord {
			records[0].Boxes = []CollisionBox{{MinX: 2, MaxX: 1, MaxY: 1, MaxZ: 1}}
			return records
		}, "box"},
	} {
		t.Run(test.name, func(t *testing.T) {
			copyRecords := append([]PhysicsRecord(nil), valid...)
			_, err := encodePhysicsRegistry([]byte("breg"), test.mutate(copyRecords), 2)
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want %q", err, test.want)
			}
		})
	}
}

func TestPhysicsFactsCoverSpecialMovementFamilies(t *testing.T) {
	record := func(id uint32, name string) Record {
		return Record{SequentialID: id, NetworkHash: id + 100, Name: name}
	}
	records := []Record{
		record(0, "minecraft:air"),
		record(1, "minecraft:ladder"),
		record(2, "minecraft:water"),
		record(3, "minecraft:lava"),
		record(4, "minecraft:web"),
		record(5, "minecraft:slime"),
		record(6, "minecraft:bed"),
		record(7, "minecraft:soul_sand"),
		record(8, "minecraft:honey_block"),
		record(9, "minecraft:bubble_column"),
		record(10, "minecraft:bubble_column"),
		record(11, "minecraft:stone"),
	}
	records[2].ModelState.Set(ModelStateLiquidDepth, 0)
	records[3].ModelState.Set(ModelStateLiquidDepth, 7)
	records[9].StateJSON = []byte(`{"drag_down":{"type":"byte","value":0}}`)
	records[10].StateJSON = []byte(`{"drag_down":{"type":"byte","value":1}}`)
	properties := map[string]PMMPLightProperties{}
	for _, record := range records {
		properties[record.Name] = PMMPLightProperties{Friction: 0.6}
	}
	properties["minecraft:air"] = PMMPLightProperties{Friction: 0.9}
	properties["minecraft:slime"] = PMMPLightProperties{Friction: 0.8}
	properties["minecraft:honey_block"] = PMMPLightProperties{Friction: 0.8}
	physics, err := buildPhysicsRecords(records, syntheticPhysicsSources(records, properties))
	if err != nil {
		t.Fatal(err)
	}
	if physics[0].Flags&physicsFlagPassable == 0 || len(physics[0].Boxes) != 0 {
		t.Fatal("air is not explicit passable empty collision")
	}
	if physics[1].Flags&physicsFlagClimbable == 0 {
		t.Fatal("ladder is not climbable")
	}
	if physics[2].Flags&(physicsFlagWater|physicsFlagPassable) != physicsFlagWater|physicsFlagPassable || physics[2].FluidHeightQ1E8 <= physics[3].FluidHeightQ1E8 {
		t.Fatal("state-dependent fluids are not explicit")
	}
	if physics[3].Flags&physicsFlagLava == 0 || physics[4].Flags&physicsFlagCobweb == 0 {
		t.Fatal("lava/cobweb flags missing")
	}
	responses := []SurfaceResponse{physics[5].SurfaceResponse, physics[6].SurfaceResponse, physics[7].SurfaceResponse, physics[8].SurfaceResponse, physics[9].SurfaceResponse, physics[10].SurfaceResponse}
	want := []SurfaceResponse{SurfaceSlime, SurfaceBed, SurfaceSoulSand, SurfaceHoney, SurfaceBubbleUp, SurfaceBubbleDown}
	if !reflect.DeepEqual(responses, want) {
		t.Fatalf("responses = %v, want %v", responses, want)
	}
	if physics[11].FrictionQ1E8 != defaultFrictionQ1E8 || physics[11].HorizontalSpeedQ1E8 != defaultSpeedQ1E8 {
		t.Fatal("ordinary block factors are not explicit")
	}
	if physics[0].FrictionQ1E8 != 90_000_000 || physics[5].FrictionQ1E8 != 80_000_000 || physics[8].FrictionQ1E8 != 80_000_000 {
		t.Fatal("PMMP friction facts were not normalized into Q1e8")
	}

	// Soul sand's horizontal factor is a Bedrock movement constant, and the
	// repository's selected Bedrock authority is the pinned bedsim module. Its
	// simulation.go multiplies the grounded movement speed by 0.543 keyed on
	// "minecraft:soul_sand", at the same point in the force law that crates/sim
	// applies this factor. 0.4 is the Java Edition value and must not be
	// substituted for it.
	if physics[7].HorizontalSpeedQ1E8 != soulSandSpeedQ1E8 {
		t.Fatalf("soul sand horizontal speed = %d, want the pinned bedsim %d", physics[7].HorizontalSpeedQ1E8, soulSandSpeedQ1E8)
	}
	// Honey deliberately keeps a different, explicitly unproven value: bedsim
	// v0.1.3 implements no honey stratum, so there is no Bedrock oracle to
	// correct it against and it must not be silently aliased onto soul sand's.
	if physics[8].HorizontalSpeedQ1E8 != unprovenHoneySpeedQ1E8 {
		t.Fatalf("honey horizontal speed = %d, want the documented unproven %d", physics[8].HorizontalSpeedQ1E8, unprovenHoneySpeedQ1E8)
	}
	if soulSandSpeedQ1E8 == unprovenHoneySpeedQ1E8 {
		t.Fatal("soul sand and honey factors must stay independently sourced")
	}
}

func TestPhysicsJoinRejectsMissingAndInvalidPMMPFriction(t *testing.T) {
	records := []Record{{SequentialID: 0, NetworkHash: 1, Name: "minecraft:stone"}}
	if _, err := buildPhysicsRecords(records, PhysicsSourceCatalog{}); err == nil || !strings.Contains(err.Error(), "missing") {
		t.Fatalf("missing source error = %v", err)
	}
	for _, friction := range []float64{0, -1, math.NaN(), math.Inf(1)} {
		sources := syntheticPhysicsSources(records, map[string]PMMPLightProperties{
			"minecraft:stone": {Friction: friction},
		})
		_, err := buildPhysicsRecords(records, sources)
		if err == nil || !strings.Contains(err.Error(), "friction") {
			t.Fatalf("friction %v error = %v", friction, err)
		}
	}
}

func syntheticPhysicsSources(records []Record, pmmp map[string]PMMPLightProperties) PhysicsSourceCatalog {
	prismarine := map[string]PrismarinePhysicsFact{}
	dragonfly := map[string][]string{}
	for _, record := range records {
		name := strings.TrimPrefix(record.Name, "minecraft:")
		fact := prismarine[name]
		fact.StateCount++
		fact.BoundingBox = "block"
		if len(record.CollisionSeed.Boxes) == 0 {
			fact.BoundingBox = "empty"
		}
		if override, ok := reviewedPhysicsOverrideFor(record.Name); ok {
			fact.BoundingBox = override.BoundingBox
		}
		prismarine[name] = fact
		dragonfly[record.Name] = []string{"fixture.Block"}
		if override, ok := reviewedPhysicsOverrideFor(record.Name); ok {
			dragonfly[record.Name] = strings.Split(override.DragonflyTypes, ",")
		}
	}
	return PhysicsSourceCatalog{PMMP: pmmp, Prismarine: prismarine, DragonflyTypes: dragonfly}
}

func TestPinnedPhysicsSourceHashRejectsValidJSONMutation(t *testing.T) {
	for _, test := range []struct {
		name, original, mutated string
	}{
		{"PMMP", `{"minecraft:stone":{"friction":0.6}}`, `{"minecraft:stone":{"friction":0.7}}`},
		{"Prismarine", `[{"name":"stone","boundingBox":"block","minStateId":0,"maxStateId":0}]`, `[{"name":"stone","boundingBox":"empty","minStateId":0,"maxStateId":0}]`},
	} {
		t.Run(test.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "physics.json")
			original := []byte(test.original)
			if err := os.WriteFile(path, original, 0o600); err != nil {
				t.Fatal(err)
			}
			want := fmt.Sprintf("%x", sha256.Sum256(original))
			if err := requirePinnedPhysicsFile(path, want, test.name); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(path, []byte(test.mutated), 0o600); err != nil {
				t.Fatal(err)
			}
			if err := requirePinnedPhysicsFile(path, want, test.name); err == nil || !strings.Contains(err.Error(), "SHA-256") {
				t.Fatalf("valid-key mutation error = %v", err)
			}
		})
	}
	if err := validateDragonflyPhysicsProvenanceFields("v0.11.1-mutated", pinnedDragonflyModuleSum, false); err == nil || !strings.Contains(err.Error(), "pinned") {
		t.Fatalf("Dragonfly version mutation error = %v", err)
	}
	if err := validateDragonflyPhysicsProvenanceFields(pinnedDragonflyVersion, "h1:mutated", false); err == nil || !strings.Contains(err.Error(), "sum") {
		t.Fatalf("Dragonfly module-sum mutation error = %v", err)
	}
	if err := validateDragonflyPhysicsProvenanceFields(pinnedDragonflyVersion, pinnedDragonflyModuleSum, true); err == nil || !strings.Contains(err.Error(), "replacement") {
		t.Fatalf("Dragonfly replacement mutation error = %v", err)
	}
}

func TestPinnedPhysicsHashesMatchAcquisitionManifestAndGoModule(t *testing.T) {
	data, err := os.ReadFile(filepath.Join("..", "..", "assets", "block-data-sources.json"))
	if err != nil {
		t.Fatal(err)
	}
	var manifest struct {
		Sources []struct {
			ID    string `json:"id"`
			Files []struct {
				Path string `json:"install_path"`
				SHA  string `json:"sha256"`
			} `json:"files"`
		} `json:"sources"`
	}
	if err := json.Unmarshal(data, &manifest); err != nil {
		t.Fatal(err)
	}
	want := map[string]string{
		"pmmp-bedrock-data/block_properties_table.json":         pinnedPMMPPhysicsSHA,
		"prismarinejs-minecraft-data/blocks.json":               pinnedPrismarineBlocksSHA,
		"prismarinejs-minecraft-data/blockStates.json":          pinnedPrismarineStatesSHA,
		"prismarinejs-minecraft-data/blockCollisionShapes.json": pinnedPrismarineShapesSHA,
	}
	seen := map[string]string{}
	for _, source := range manifest.Sources {
		for _, file := range source.Files {
			key := source.ID + "/" + file.Path
			if _, required := want[key]; required {
				seen[key] = file.SHA
			}
		}
	}
	if !reflect.DeepEqual(seen, want) {
		t.Fatalf("physics source manifest pins = %v, want %v", seen, want)
	}
	goMod, err := os.ReadFile("go.mod")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(goMod, []byte("github.com/df-mc/dragonfly "+pinnedDragonflyVersion)) {
		t.Fatalf("go.mod does not pin Dragonfly %s", pinnedDragonflyVersion)
	}
	goSum, err := os.ReadFile("go.sum")
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(goSum, []byte("github.com/df-mc/dragonfly "+pinnedDragonflyVersion+" "+pinnedDragonflyModuleSum)) {
		t.Fatal("go.sum does not pin the Dragonfly module content hash")
	}
}

func TestReviewedPhysicsOverridesRejectIndependentBehaviorMutations(t *testing.T) {
	records := make([]Record, 0, 78)
	properties := map[string]PMMPLightProperties{}
	for _, name := range []string{"minecraft:cave_vines", "minecraft:cave_vines_body_with_berries", "minecraft:cave_vines_head_with_berries"} {
		properties[name] = PMMPLightProperties{Friction: 0.6}
		for state := 0; state < 26; state++ {
			records = append(records, Record{SequentialID: uint32(len(records)), NetworkHash: uint32(len(records) + 1), Name: name})
		}
	}
	sources := syntheticPhysicsSources(records, properties)
	physics, err := buildPhysicsRecords(records, sources)
	if err != nil {
		t.Fatal(err)
	}
	for index, record := range physics {
		if record.Flags&physicsFlagClimbable == 0 {
			t.Fatalf("cave-vine state %d is not climbable", index)
		}
	}

	badPrismarine := sources
	badPrismarine.Prismarine = maps.Clone(sources.Prismarine)
	fact := badPrismarine.Prismarine["cave_vines"]
	fact.BoundingBox = "block"
	badPrismarine.Prismarine["cave_vines"] = fact
	if _, err := buildPhysicsRecords(records, badPrismarine); err == nil || !strings.Contains(err.Error(), "Prismarine") {
		t.Fatalf("Prismarine mutation error = %v", err)
	}

	badDragonfly := sources
	badDragonfly.DragonflyTypes = maps.Clone(sources.DragonflyTypes)
	delete(badDragonfly.DragonflyTypes, "minecraft:cave_vines")
	if _, err := buildPhysicsRecords(records, badDragonfly); err == nil || !strings.Contains(err.Error(), "Dragonfly") {
		t.Fatalf("Dragonfly mutation error = %v", err)
	}
}

func TestBubbleDirectionFactsFailClosed(t *testing.T) {
	record := Record{SequentialID: 0, NetworkHash: 1, Name: "minecraft:bubble_column"}
	properties := map[string]PMMPLightProperties{record.Name: {Friction: 0.6}}
	for _, state := range [][]byte{
		nil,
		[]byte(`{`),
		[]byte(`{}`),
		[]byte(`{"drag_down":{"type":"string","value":"sideways"}}`),
		[]byte(`{"drag_down":{"type":"byte","value":2}}`),
		[]byte(`{"drag_down":{"type":"byte","value":0},"unknown":{"type":"byte","value":0}}`),
		[]byte(`{"drag_down":{"type":"byte","value":0},"drag_down":{"type":"byte","value":1}}`),
	} {
		record.StateJSON = state
		sources := syntheticPhysicsSources([]Record{record}, properties)
		if _, err := buildPhysicsRecords([]Record{record}, sources); err == nil || !strings.Contains(err.Error(), "drag_down") {
			t.Fatalf("state %q error = %v", state, err)
		}
	}
}

func TestEncodeLREG1001RequiresOneBoundedBytePerBREGState(t *testing.T) {
	valid := fixtureLightRegistry{
		emission: map[uint32]uint8{0: 1, 1: 2},
		filter:   map[uint32]uint8{0: 3, 1: 4},
	}
	for _, test := range []struct {
		name     string
		records  []Record
		registry fixtureLightRegistry
		want     string
	}{
		{"gap", []Record{testRecord(0, 1), testRecord(2, 2)}, valid, "contiguous"},
		{"duplicate", []Record{testRecord(0, 1), testRecord(0, 2)}, valid, "duplicate"},
		{"emission nibble", []Record{testRecord(0, 1)}, fixtureLightRegistry{emission: map[uint32]uint8{0: 16}, filter: map[uint32]uint8{0: 0}}, "emission"},
		{"filter nibble", []Record{testRecord(0, 1)}, fixtureLightRegistry{emission: map[uint32]uint8{0: 0}, filter: map[uint32]uint8{0: 16}}, "filter"},
	} {
		t.Run(test.name, func(t *testing.T) {
			_, err := encodeLightRegistry([]byte("breg"), test.records, test.registry)
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want %q", err, test.want)
			}
		})
	}
}

func TestLightBindingRequiresExactBREGIdentityOrderAndCount(t *testing.T) {
	records := []Record{testRecord(0, 10), testRecord(1, 20)}
	records[0].Name, records[0].StateJSON = "minecraft:first", []byte(`{"lit":0}`)
	records[1].Name, records[1].StateJSON = "minecraft:second", []byte(`{"lit":1}`)
	for i := range records {
		records[i].Provenance = ProvenanceDragonfly
	}
	breg, err := encode(records)
	if err != nil {
		t.Fatalf("encode binding BREG: %v", err)
	}
	if err := validateLightBindingBREG(breg, records); err != nil {
		t.Fatalf("validate exact binding: %v", err)
	}

	fewer := slices.Clone(records[:1])
	if err := validateLightBindingBREG(breg, fewer); err == nil || !strings.Contains(err.Error(), "count") {
		t.Fatalf("count mismatch error = %v", err)
	}
	wrongState := slices.Clone(records)
	wrongState[1].StateJSON = []byte(`{"lit":0}`)
	if err := validateLightBindingBREG(breg, wrongState); err == nil || !strings.Contains(err.Error(), "identity") {
		t.Fatalf("state mismatch error = %v", err)
	}
	wrongHash := slices.Clone(records)
	wrongHash[0].NetworkHash++
	if err := validateLightBindingBREG(breg, wrongHash); err == nil || !strings.Contains(err.Error(), "identity") {
		t.Fatalf("hash mismatch error = %v", err)
	}
}

func TestPinnedDragonflyStateAwareLightAccessors(t *testing.T) {
	for _, test := range []struct {
		name       string
		properties map[string]TypedScalar
		emission   uint8
		filter     uint8
	}{
		{"minecraft:air", nil, 0, 0},
		{"minecraft:glass", nil, 0, 0},
		{"minecraft:oak_leaves", nil, 0, 1},
		{"minecraft:water", nil, 0, 2},
		{"minecraft:lava", nil, 15, 2},
		{"minecraft:stone", nil, 0, 15},
		{"minecraft:campfire", map[string]TypedScalar{"extinguished": {Kind: ScalarByte, Byte: 0}}, 15, 0},
		{"minecraft:campfire", map[string]TypedScalar{"extinguished": {Kind: ScalarByte, Byte: 1}}, 0, 0},
		{"minecraft:sea_pickle", map[string]TypedScalar{"cluster_count": {Kind: ScalarInt, Int: 0}, "dead_bit": {Kind: ScalarByte, Byte: 0}}, 6, 0},
		{"minecraft:sea_pickle", map[string]TypedScalar{"cluster_count": {Kind: ScalarInt, Int: 3}, "dead_bit": {Kind: ScalarByte, Byte: 0}}, 15, 0},
		{"minecraft:sea_pickle", map[string]TypedScalar{"cluster_count": {Kind: ScalarInt, Int: 3}, "dead_bit": {Kind: ScalarByte, Byte: 1}}, 0, 0},
		{"minecraft:furnace", nil, 0, 15},
		{"minecraft:lit_furnace", nil, 13, 15},
		{"minecraft:candle", map[string]TypedScalar{"candles": {Kind: ScalarInt, Int: 0}, "lit": {Kind: ScalarByte, Byte: 0}}, 0, 0},
		{"minecraft:candle", map[string]TypedScalar{"candles": {Kind: ScalarInt, Int: 0}, "lit": {Kind: ScalarByte, Byte: 1}}, 3, 0},
		{"minecraft:candle", map[string]TypedScalar{"candles": {Kind: ScalarInt, Int: 3}, "lit": {Kind: ScalarByte, Byte: 1}}, 12, 0},
	} {
		t.Run(test.name+canonicalPropertySuffix(test.properties), func(t *testing.T) {
			emission, filter := pinnedDragonflyLight(t, test.name, test.properties)
			if emission != test.emission || filter != test.filter {
				t.Fatalf("light = emission %d/filter %d, want %d/%d", emission, filter, test.emission, test.filter)
			}
		})
	}
}

func TestPMMPFallbackIsNarrowUniformAndProvenanceTagged(t *testing.T) {
	records, err := collect(world.DefaultBlockRegistry)
	if err != nil {
		t.Fatalf("collect registry: %v", err)
	}
	pmmp := map[string]PMMPLightProperties{
		"minecraft:redstone_lamp":     {Brightness: 0, Opacity: 0},
		"minecraft:lit_redstone_lamp": {Brightness: 15, Opacity: 0},
		// An exact PMMP entry cannot widen the audited fallback allowlist.
		"minecraft:hard_pink_stained_glass": {Brightness: 15, Opacity: 0},
	}
	properties, report, err := resolveAuthoritativeLightProperties(records, world.DefaultBlockRegistry, pmmp)
	if err != nil {
		t.Fatalf("resolve authoritative lights: %v", err)
	}
	if got, want := report.PMMPFallbackIdentifiers, []string{"minecraft:lit_redstone_lamp", "minecraft:redstone_lamp"}; !slices.Equal(got, want) {
		t.Fatalf("fallback identifiers = %v, want %v", got, want)
	}
	if report.DragonflyAccessorStates != 16_911 || report.PMMPFallbackStates != 2 ||
		!slices.Equal(report.PMMPFallbackSequentialIDs, []uint32{1309, 6853}) {
		t.Fatalf("fallback provenance = %+v", report)
	}
	for _, record := range records {
		switch record.Name {
		case "minecraft:redstone_lamp":
			if got := properties[record.SequentialID]; got != 0x00 {
				t.Fatalf("unlit lamp runtime %d light = %#x", record.SequentialID, got)
			}
		case "minecraft:lit_redstone_lamp":
			if got := properties[record.SequentialID]; got != 0x0f {
				t.Fatalf("lit lamp runtime %d light = %#x", record.SequentialID, got)
			}
		}
	}

	delete(pmmp, "minecraft:lit_redstone_lamp")
	if _, _, err := resolveAuthoritativeLightProperties(records, world.DefaultBlockRegistry, pmmp); err == nil || !strings.Contains(err.Error(), "lit_redstone_lamp") {
		t.Fatalf("missing exact PMMP fallback error = %v", err)
	}
	pmmp["minecraft:lit_redstone_lamp"] = PMMPLightProperties{Brightness: 16, Opacity: 0}
	if _, _, err := resolveAuthoritativeLightProperties(records, world.DefaultBlockRegistry, pmmp); err == nil || !strings.Contains(err.Error(), "brightness") {
		t.Fatalf("out-of-range fallback error = %v", err)
	}
}

func TestAuthoritativeLightGenerationRejectsMissingPMMP(t *testing.T) {
	if _, _, err := encodeAuthoritativeLightRegistry(nil, nil, world.DefaultBlockRegistry, ""); err == nil || !strings.Contains(err.Error(), "PMMP") {
		t.Fatalf("missing PMMP source error = %v", err)
	}
}

func pinnedDragonflyLight(t *testing.T, name string, required map[string]TypedScalar) (uint8, uint8) {
	t.Helper()
	states, err := collectDragonflyStates(world.DefaultBlockRegistry)
	if err != nil {
		t.Fatalf("collect Dragonfly states: %v", err)
	}
	for _, state := range states {
		if state.Name != name {
			continue
		}
		properties := make(map[string]TypedScalar, len(state.Properties))
		for _, property := range state.Properties {
			properties[property.Name] = property.Value
		}
		matches := true
		for property, want := range required {
			if properties[property] != want {
				matches = false
				break
			}
		}
		if matches {
			return world.DefaultBlockRegistry.LightBlock(state.Ordinal), world.DefaultBlockRegistry.FilteringBlock(state.Ordinal)
		}
	}
	t.Fatalf("Dragonfly state %s%v not found", name, required)
	return 0, 0
}

func canonicalPropertySuffix(properties map[string]TypedScalar) string {
	if len(properties) == 0 {
		return ""
	}
	names := make([]string, 0, len(properties))
	for name := range properties {
		names = append(names, name)
	}
	slices.Sort(names)
	var suffix strings.Builder
	for _, name := range names {
		fmt.Fprintf(&suffix, "/%s=%v", name, properties[name])
	}
	return suffix.String()
}

type registryBiome struct {
	id   int
	name string
}

func (b registryBiome) Temperature() float64    { return 0.8 }
func (b registryBiome) Rainfall() float64       { return 0.4 }
func (b registryBiome) Depth() float64          { return 0.1 }
func (b registryBiome) Scale() float64          { return 0.2 }
func (b registryBiome) WaterColour() color.RGBA { return color.RGBA{} }
func (b registryBiome) Tags() []string          { return nil }
func (b registryBiome) String() string          { return b.name }
func (b registryBiome) EncodeBiome() int        { return b.id }

func TestEncodeBiomeRegistrySortsByStableEncodeBiomeID(t *testing.T) {
	records := []BiomeRecord{
		{ID: 48, Name: "minecraft:bamboo_jungle"},
		{ID: 1, Name: "minecraft:plains"},
	}

	got, err := encodeBiomeRegistry(records)
	if err != nil {
		t.Fatalf("encode biome registry: %v", err)
	}
	want := []byte{
		'B', 'I', 'O', 'R', 'E', 'G', '0', '1',
		0x02, 0x00, 0x00, 0x00,
		0x01, 0x00, 0x00, 0x00,
		0x10, 0x00,
		'm', 'i', 'n', 'e', 'c', 'r', 'a', 'f', 't', ':', 'p', 'l', 'a', 'i', 'n', 's',
		0x30, 0x00, 0x00, 0x00,
		0x17, 0x00,
		'm', 'i', 'n', 'e', 'c', 'r', 'a', 'f', 't', ':', 'b', 'a', 'm', 'b', 'o', 'o', '_', 'j', 'u', 'n', 'g', 'l', 'e',
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("encoded biome bytes:\n got: %x\nwant: %x", got, want)
	}
	if records[0].ID != 48 || records[1].ID != 1 {
		t.Fatal("encodeBiomeRegistry mutated input order")
	}
}

func TestEncodeBiomeRegistryRejectsDuplicatesAndBounds(t *testing.T) {
	tests := []struct {
		name    string
		records []BiomeRecord
		wantErr string
	}{
		{
			name:    "duplicate id",
			records: []BiomeRecord{{ID: 1, Name: "minecraft:plains"}, {ID: 1, Name: "minecraft:other"}},
			wantErr: "duplicate biome ID",
		},
		{
			name:    "duplicate name",
			records: []BiomeRecord{{ID: 1, Name: "minecraft:plains"}, {ID: 2, Name: "minecraft:plains"}},
			wantErr: "duplicate biome name",
		},
		{
			name:    "name bound",
			records: []BiomeRecord{{ID: 1, Name: strings.Repeat("x", maxBiomeNameBytes+1)}},
			wantErr: "biome name too long",
		},
		{
			name:    "empty name",
			records: []BiomeRecord{{ID: 1}},
			wantErr: "biome name is empty",
		},
		{
			name:    "record count",
			records: make([]BiomeRecord, maxBiomeRecordCount+1),
			wantErr: "too many biome records",
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			_, err := encodeBiomeRegistry(test.records)
			if err == nil || !strings.Contains(err.Error(), test.wantErr) {
				t.Fatalf("encode error = %v, want %q", err, test.wantErr)
			}
		})
	}
}

func TestCollectBiomesCanonicalizesNamesAndRejectsInvalidIDs(t *testing.T) {
	records, err := collectBiomes([]world.Biome{
		registryBiome{id: 48, name: "bamboo_jungle"},
		registryBiome{id: 900, name: "example:violet_marsh"},
	})
	if err != nil {
		t.Fatalf("collect biomes: %v", err)
	}
	want := []BiomeRecord{
		{ID: 48, Name: "minecraft:bamboo_jungle"},
		{ID: 900, Name: "example:violet_marsh"},
	}
	if !slices.Equal(records, want) {
		t.Fatalf("records = %#v, want %#v", records, want)
	}

	for _, id := range []int{-1, math.MaxUint16 + 1} {
		_, err := collectBiomes([]world.Biome{registryBiome{id: id, name: "invalid"}})
		if err == nil || !strings.Contains(err.Error(), "biome ID") {
			t.Fatalf("collect biome ID %d error = %v", id, err)
		}
	}
}

func TestCollectRegisteredDragonflyBiomesUsesStableNetworkIDs(t *testing.T) {
	records, err := collectBiomes(world.Biomes())
	if err != nil {
		t.Fatalf("collect registered biomes: %v", err)
	}
	if len(records) != 88 {
		t.Fatalf("biome record count = %d, want 88", len(records))
	}
	for _, want := range []BiomeRecord{
		{ID: 0, Name: "minecraft:ocean"},
		{ID: 1, Name: "minecraft:plains"},
		{ID: 48, Name: "minecraft:bamboo_jungle"},
		{ID: 194, Name: "minecraft:sulfur_caves"},
	} {
		found := false
		for _, record := range records {
			if record == want {
				found = true
				break
			}
		}
		if !found {
			t.Errorf("missing Dragonfly biome %#v", want)
		}
	}
}

type classifierBlock struct {
	name       string
	properties map[string]any
	stateHash  uint64
	model      world.BlockModel
}

func (b classifierBlock) EncodeBlock() (string, map[string]any) {
	return b.name, b.properties
}

func (b classifierBlock) Hash() (uint64, uint64) {
	return 1, b.stateHash
}

func (b classifierBlock) Model() world.BlockModel {
	return b.model
}

func TestEncodeSortsRecordsAndMatchesExactBytes(t *testing.T) {
	records := []Record{
		{
			SequentialID: 2,
			NetworkHash:  0xaabbccdd,
			Flags:        flagCubeGeometry | flagOccludesFullFace,
			Name:         "zeta",
			StateJSON:    []byte(`{}`),
		},
		{
			SequentialID: 1,
			NetworkHash:  0x11223344,
			Flags:        flagAir,
			Name:         "alpha",
			StateJSON:    []byte(`{"a":1}`),
		},
	}

	got, err := encode(records)
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	want, err := hex.DecodeString("4252454731303033e90300000200000002000000000000000000000002000000020000000100000044332211010000000000020000000500070000000000000000000000000000000000000000000000000000000000000000000000616c7068617b2261223a317d02000000ddccbbaa0600000000000200000004000200000000000000000000000000000000000000000000000000000000000000000000007a6574617b7d")
	if err != nil {
		t.Fatalf("decode expected BREG1003 fixture: %v", err)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("encoded bytes:\n got: %x\nwant: %x", got, want)
	}
	if records[0].SequentialID != 2 || records[1].SequentialID != 1 {
		t.Fatal("encode mutated the input order")
	}
}

func TestEncodeRejectsDuplicateSequentialIDs(t *testing.T) {
	records := []Record{
		testRecord(7, 100),
		testRecord(7, 101),
	}
	_, err := encode(records)
	if err == nil || !strings.Contains(err.Error(), "duplicate sequential ID") {
		t.Fatalf("encode error = %v, want duplicate sequential ID", err)
	}
}

func TestEncodeRejectsDuplicateNetworkHashes(t *testing.T) {
	records := []Record{
		testRecord(7, 100),
		testRecord(8, 100),
	}
	_, err := encode(records)
	if err == nil || !strings.Contains(err.Error(), "duplicate network hash") {
		t.Fatalf("encode error = %v, want duplicate network hash", err)
	}
}

func TestEncodeRejectsInvalidFlagSemantics(t *testing.T) {
	tests := []struct {
		name     string
		flags    uint8
		wantFlag string
	}{
		{name: "unknown bit", flags: 1 << 4, wantFlag: "0x10"},
		{name: "air and cube", flags: flagAir | flagCubeGeometry, wantFlag: "0x3"},
		{name: "occluder without cube", flags: flagOccludesFullFace, wantFlag: "0x4"},
		{name: "leaf without cube", flags: flagLeafModel, wantFlag: "0x8"},
		{
			name:     "leaf and occluder",
			flags:    flagCubeGeometry | flagOccludesFullFace | flagLeafModel,
			wantFlag: "0xe",
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			record := testRecord(77, 100)
			record.Flags = test.flags
			_, err := encode([]Record{record})
			if err == nil || !strings.Contains(err.Error(), "sequential ID 77") || !strings.Contains(err.Error(), test.wantFlag) {
				t.Fatalf("encode error = %v, want sequential ID 77 and flag %s", err, test.wantFlag)
			}
		})
	}
}

func TestCanonicalJSONSortsPropertyKeys(t *testing.T) {
	got, err := canonicalJSON(map[string]any{
		"z": int32(3),
		"a": map[string]any{"b": int32(2), "a": int32(1)},
		"m": "value",
	})
	if err != nil {
		t.Fatalf("canonical JSON: %v", err)
	}
	want := []byte(`{"a":{"a":1,"b":2},"m":"value","z":3}`)
	if !bytes.Equal(got, want) {
		t.Fatalf("canonical JSON = %s, want %s", got, want)
	}
}

func TestApprovedUnknownFullCubeStateAcceptsOnlyExactPinnedSchemas(t *testing.T) {
	accepted := []struct {
		name       string
		properties map[string]any
	}{
		{name: "minecraft:mycelium", properties: nil},
		{name: "minecraft:mycelium", properties: map[string]any{}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(15)}},
		{name: "minecraft:brown_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(7)}},
		{name: "minecraft:mushroom_stem", properties: map[string]any{"huge_mushroom_bits": int32(12)}},
	}
	for _, test := range accepted {
		if !approvedUnknownFullCubeState(test.name, test.properties) {
			t.Errorf("approvedUnknownFullCubeState(%q, %#v) = false, want true", test.name, test.properties)
		}
	}

	rejected := []struct {
		name       string
		properties map[string]any
	}{
		{name: "minecraft:stone", properties: map[string]any{}},
		{name: "minecraft:red_mushroom", properties: map[string]any{"huge_mushroom_bits": int32(0)}},
		{name: "minecraft:mycelium", properties: map[string]any{"unexpected": int32(0)}},
		{name: "minecraft:red_mushroom_block", properties: nil},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(0), "unexpected": int32(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(-1)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int32(16)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": uint8(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": int(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": float64(0)}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": "0"}},
		{name: "minecraft:red_mushroom_block", properties: map[string]any{"huge_mushroom_bits": false}},
	}
	for _, test := range rejected {
		if approvedUnknownFullCubeState(test.name, test.properties) {
			t.Errorf("approvedUnknownFullCubeState(%q, %#v) = true, want false", test.name, test.properties)
		}
	}
}

func TestClassifyFlagsSeparatesSolidLeavesAndOtherModels(t *testing.T) {
	if got := classifyFlags(block.Stone{}); got != flagCubeGeometry|flagOccludesFullFace {
		t.Fatalf("stone flags = %#x", got)
	}
	leaf := block.Leaves{Type: block.CherryLeaves(), Persistent: true}
	if got := classifyFlags(leaf); got != flagCubeGeometry|flagLeafModel {
		t.Fatalf("cherry leaf flags = %#x", got)
	}
	if got := classifyFlags(block.Torch{}); got != 0 {
		t.Fatalf("torch flags = %#x", got)
	}
}

func TestClassifyFlagsPreservesSolidAndFailsClosedOverrides(t *testing.T) {
	tests := []struct {
		name  string
		block classifierBlock
		want  uint8
	}{
		{
			name:  "implemented solid",
			block: classifierBlock{name: "minecraft:stone", stateHash: 1, model: model.Solid{}},
			want:  flagCubeGeometry | flagOccludesFullFace,
		},
		{
			name:  "approved unknown",
			block: classifierBlock{name: "minecraft:mycelium", stateHash: math.MaxUint64, model: model.Empty{}},
			want:  flagCubeGeometry | flagOccludesFullFace,
		},
		{
			name:  "implemented non-solid target",
			block: classifierBlock{name: "minecraft:mycelium", stateHash: 1, model: model.Empty{}},
			want:  0,
		},
		{
			name:  "unapproved unknown",
			block: classifierBlock{name: "minecraft:acacia_sapling", stateHash: math.MaxUint64, model: model.Empty{}},
			want:  0,
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			if got := classifyFlags(test.block); got != test.want {
				t.Fatalf("classifyFlags() = %#x, want %#x", got, test.want)
			}
		})
	}
}

func TestEncodeIsStableAcrossShuffledInputs(t *testing.T) {
	validFlags := []uint8{
		0,
		flagAir,
		flagCubeGeometry,
		flagCubeGeometry | flagOccludesFullFace,
		flagCubeGeometry | flagLeafModel,
	}
	records := make([]Record, 16)
	for i := range records {
		records[i] = Record{
			SequentialID: uint32(i),
			NetworkHash:  uint32(10_000 + i),
			Flags:        validFlags[i%len(validFlags)],
			Name:         "minecraft:block_" + string(rune('a'+i)),
			StateJSON:    []byte(`{"value":1}`),
		}
	}
	want, err := encode(records)
	if err != nil {
		t.Fatalf("encode baseline: %v", err)
	}

	rng := rand.New(rand.NewSource(42))
	for iteration := 0; iteration < 100; iteration++ {
		shuffled := append([]Record(nil), records...)
		rng.Shuffle(len(shuffled), func(i, j int) {
			shuffled[i], shuffled[j] = shuffled[j], shuffled[i]
		})
		got, err := encode(shuffled)
		if err != nil {
			t.Fatalf("encode shuffle %d: %v", iteration, err)
		}
		if !bytes.Equal(got, want) {
			t.Fatalf("shuffle %d produced different bytes", iteration)
		}
	}
}

func TestEncodeRejectsOversizedInputs(t *testing.T) {
	tests := []struct {
		name    string
		records []Record
		wantErr string
	}{
		{
			name:    "record count",
			records: make([]Record, maxRecordCount+1),
			wantErr: "too many records",
		},
		{
			name: "name",
			records: []Record{{
				SequentialID: 1,
				NetworkHash:  2,
				Name:         strings.Repeat("a", maxNameBytes+1),
				StateJSON:    []byte(`{}`),
			}},
			wantErr: "name too long",
		},
		{
			name: "state",
			records: []Record{{
				SequentialID: 1,
				NetworkHash:  2,
				Name:         "minecraft:test",
				StateJSON:    bytes.Repeat([]byte{'x'}, maxStateBytes+1),
			}},
			wantErr: "state payload too large",
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			_, err := encode(tt.records)
			if err == nil || !strings.Contains(err.Error(), tt.wantErr) {
				t.Fatalf("encode error = %v, want %q", err, tt.wantErr)
			}
		})
	}
}

func TestCollectDefaultBlockRegistry(t *testing.T) {
	records, err := collect(world.DefaultBlockRegistry)
	if err != nil {
		t.Fatalf("collect registry: %v", err)
	}
	if len(records) != 16_913 {
		t.Fatalf("registry record count = %d, want 16913", len(records))
	}
	air := findByName(t, records, "minecraft:air")
	if air.Flags != flagAir {
		t.Fatalf("air flags = %#x, want %#x", air.Flags, flagAir)
	}
	if air.NetworkHash != 0xdbf44120 {
		t.Fatalf("air hash = %#x", air.NetworkHash)
	}
	stone := findByName(t, records, "minecraft:stone")
	if stone.Flags != flagCubeGeometry|flagOccludesFullFace {
		t.Fatalf("stone flags = %#x, want cube+occluder", stone.Flags)
	}

	var cubeCount, occluderCount, leafCount, airCount int
	leafNames := map[string]struct{}{}
	for _, record := range records {
		if !validRecordFlags(record.Flags) {
			t.Fatalf("record %d has invalid flags %#x", record.SequentialID, record.Flags)
		}
		if record.Flags&flagCubeGeometry != 0 {
			cubeCount++
		}
		if record.Flags&flagOccludesFullFace != 0 {
			occluderCount++
		}
		if record.Flags&flagLeafModel != 0 {
			leafCount++
			leafNames[record.Name] = struct{}{}
			want := flagCubeGeometry | flagLeafModel
			if record.Flags != want {
				t.Errorf("leaf %s state %s flags = %#x, want %#x", record.Name, record.StateJSON, record.Flags, want)
			}
		}
		if record.Flags&flagAir != 0 {
			airCount++
		}
	}
	if cubeCount != 713 || occluderCount != 669 || leafCount != 44 || airCount != 1 {
		t.Fatalf("flag counts cube=%d occluder=%d leaf=%d air=%d, want 713/669/44/1", cubeCount, occluderCount, leafCount, airCount)
	}
	wantLeafNames := []string{
		"minecraft:acacia_leaves",
		"minecraft:azalea_leaves",
		"minecraft:azalea_leaves_flowered",
		"minecraft:birch_leaves",
		"minecraft:cherry_leaves",
		"minecraft:dark_oak_leaves",
		"minecraft:jungle_leaves",
		"minecraft:mangrove_leaves",
		"minecraft:oak_leaves",
		"minecraft:pale_oak_leaves",
		"minecraft:spruce_leaves",
	}
	if len(leafNames) != len(wantLeafNames) {
		t.Fatalf("distinct leaf names = %d, want %d: %#v", len(leafNames), len(wantLeafNames), leafNames)
	}
	for _, name := range wantLeafNames {
		if _, ok := leafNames[name]; !ok {
			t.Errorf("leaf name %s is absent", name)
		}
	}

	for name, wantCount := range map[string]int{
		"minecraft:mycelium":             1,
		"minecraft:red_mushroom_block":   16,
		"minecraft:brown_mushroom_block": 16,
		"minecraft:mushroom_stem":        16,
	} {
		matches := findAllByName(records, name)
		if len(matches) != wantCount {
			t.Errorf("%s state count = %d, want %d", name, len(matches), wantCount)
			continue
		}
		for _, record := range matches {
			if record.Flags != flagCubeGeometry|flagOccludesFullFace {
				t.Errorf("%s state %s flags = %#x, want cube+occluder", name, record.StateJSON, record.Flags)
			}
		}
	}

	for _, name := range []string{
		"minecraft:acacia_sapling",
		"minecraft:cactus_flower",
		"minecraft:flower_pot",
		"minecraft:iron_door",
		"minecraft:iron_trapdoor",
	} {
		matches := findAllByName(records, name)
		if len(matches) == 0 {
			t.Errorf("negative-control block %s is absent", name)
			continue
		}
		for _, record := range matches {
			if record.Flags&flagCubeGeometry != 0 {
				t.Errorf("negative-control block %s state %s was marked cube geometry", name, record.StateJSON)
			}
		}
	}
}

func testRecord(sequentialID, networkHash uint32) Record {
	return Record{
		SequentialID: sequentialID,
		NetworkHash:  networkHash,
		Name:         "minecraft:test",
		StateJSON:    []byte(`{}`),
	}
}

func findByName(t *testing.T, records []Record, name string) Record {
	t.Helper()
	for _, record := range records {
		if record.Name == name {
			return record
		}
	}
	t.Fatalf("record %q not found", name)
	return Record{}
}

func findAllByName(records []Record, name string) []Record {
	var matches []Record
	for _, record := range records {
		if record.Name == name {
			matches = append(matches, record)
		}
	}
	return matches
}

func byteState(name string, value byte) StateProperty {
	return StateProperty{Name: name, Value: TypedScalar{Kind: ScalarByte, Byte: value}}
}

func intState(name string, value int32) StateProperty {
	return StateProperty{Name: name, Value: TypedScalar{Kind: ScalarInt, Int: value}}
}

func stringState(name, value string) StateProperty {
	return StateProperty{Name: name, Value: TypedScalar{Kind: ScalarString, String: value}}
}

func sourceState(name string, properties ...StateProperty) SourceState {
	return SourceState{Name: name, Properties: properties}
}

func TestJoinSourcesBijection(t *testing.T) {
	pmmp := []SourceState{
		sourceState("minecraft:test", intState("level", 2), byteState("open", 1)),
		sourceState("minecraft:air"),
	}
	dragonfly := []SourceState{
		sourceState("minecraft:air"),
		// Property order is deliberately reversed. Identity is the sorted typed
		// compound, never input order.
		sourceState("minecraft:test", byteState("open", 1), intState("level", 2)),
	}
	prismarine := []SourceState{
		sourceState("minecraft:test", byteState("open", 1), intState("level", 2)),
		sourceState("minecraft:air"),
	}

	joined, err := joinSources(pmmp, dragonfly, prismarine, canonicalStateHash)
	if err != nil {
		t.Fatalf("join sources: %v", err)
	}
	if len(joined) != 2 {
		t.Fatalf("joined records = %d, want 2", len(joined))
	}
	if got, want := string(joined[0].StateJSON), `{}`; got != want {
		t.Fatalf("air state = %s, want %s", got, want)
	}
	if got, want := string(joined[1].StateJSON), `{"level":{"type":"int","value":2},"open":{"type":"byte","value":1}}`; got != want {
		t.Fatalf("typed state = %s, want %s", got, want)
	}

	t.Run("duplicate property", func(t *testing.T) {
		bad := []SourceState{sourceState("minecraft:test", intState("level", 1), intState("level", 1))}
		_, err := joinSources(bad, []SourceState{sourceState("minecraft:test", intState("level", 1))}, []SourceState{sourceState("minecraft:test", intState("level", 1))}, canonicalStateHash)
		if err == nil || !strings.Contains(err.Error(), "duplicate state property") {
			t.Fatalf("error = %v, want duplicate state property", err)
		}
	})

	t.Run("missing and extra records", func(t *testing.T) {
		_, err := joinSources(pmmp, dragonfly[:1], prismarine, canonicalStateHash)
		if err == nil || !strings.Contains(err.Error(), "missing from Dragonfly") {
			t.Fatalf("missing error = %v", err)
		}
		extra := append(append([]SourceState(nil), prismarine...), sourceState("minecraft:extra"))
		_, err = joinSources(pmmp, dragonfly, extra, canonicalStateHash)
		if err == nil || !strings.Contains(err.Error(), "extra in Prismarine") {
			t.Fatalf("extra error = %v", err)
		}
	})

	t.Run("deliberate canonical hash collision", func(t *testing.T) {
		constantHash := func([]byte) uint64 { return 7 }
		_, err := joinSources(pmmp, dragonfly, prismarine, constantHash)
		if err == nil || !strings.Contains(err.Error(), "canonical state hash collision") {
			t.Fatalf("collision error = %v", err)
		}
	})
}

func TestJoinSourcesRejectsTypedStateMismatch(t *testing.T) {
	pmmp := []SourceState{sourceState("minecraft:test", intState("value", 1))}

	tests := []struct {
		name       string
		dragonfly  []SourceState
		prismarine []SourceState
	}{
		{
			name:       "scalar type",
			dragonfly:  []SourceState{sourceState("minecraft:test", byteState("value", 1))},
			prismarine: pmmp,
		},
		{
			name:       "unequal value",
			dragonfly:  []SourceState{sourceState("minecraft:test", intState("value", 2))},
			prismarine: pmmp,
		},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			_, err := joinSources(pmmp, test.dragonfly, test.prismarine, canonicalStateHash)
			if err == nil || !strings.Contains(err.Error(), "typed state mismatch") {
				t.Fatalf("error = %v, want typed state mismatch", err)
			}
		})
	}
}

func TestValentineSubsetAudit(t *testing.T) {
	canonical := []SourceState{
		sourceState("minecraft:air"),
		sourceState("minecraft:new_block", intState("age", 0)),
		sourceState("minecraft:test", byteState("open", 0)),
		sourceState("minecraft:test", byteState("open", 1)),
	}
	valentine := []SourceState{
		sourceState("minecraft:air"),
		sourceState("minecraft:test", byteState("open", 0)),
		sourceState("minecraft:test", byteState("open", 1)),
	}

	audit, err := auditValentineSubset(canonical, valentine)
	if err != nil {
		t.Fatalf("audit Valentine subset: %v", err)
	}
	if audit.CanonicalStates != 4 || audit.ValentineStates != 3 || audit.GapStates != 1 {
		t.Fatalf("state counts = %+v", audit)
	}
	if audit.CanonicalNames != 3 || audit.ValentineNames != 2 || audit.GapNames != 1 {
		t.Fatalf("name counts = %+v", audit)
	}
	if !slices.Equal(audit.MissingNames, []string{"minecraft:new_block"}) {
		t.Fatalf("missing names = %#v", audit.MissingNames)
	}
	if audit.Joined != 3 || audit.Missing != 1 || audit.Extra != 0 || audit.Mismatched != 0 {
		t.Fatalf("join disposition = %+v", audit)
	}

	reordered := []SourceState{valentine[2], valentine[0], valentine[1]}
	_, err = auditValentineSubset(canonical, reordered)
	if err == nil || !strings.Contains(err.Error(), "Valentine overlap order") {
		t.Fatalf("order error = %v", err)
	}
	if len(audit.MissingStates) != 1 || !strings.Contains(audit.MissingStates[0], "minecraft:new_block") {
		t.Fatalf("missing states = %#v", audit.MissingStates)
	}

	bad := append([]SourceState(nil), valentine...)
	bad[0] = sourceState("minecraft:test", intState("open", 1))
	_, err = auditValentineSubset(canonical, bad)
	if err == nil || !strings.Contains(err.Error(), "Valentine typed state") {
		t.Fatalf("mismatch error = %v", err)
	}
}

func TestFileSHA256ReturnsReadError(t *testing.T) {
	if _, err := fileSHA256(filepath.Join(t.TempDir(), "missing.bin")); err == nil {
		t.Fatal("fileSHA256 accepted a missing source")
	}
}

func TestSelectorCardinality(t *testing.T) {
	var stairs []Record
	for upsideDown := byte(0); upsideDown < 2; upsideDown++ {
		for facing := int32(0); facing < 4; facing++ {
			r, err := classifyRecord(sourceState(
				"minecraft:oak_stairs",
				intState("weirdo_direction", facing),
				byteState("upside_down_bit", upsideDown),
			))
			if err != nil {
				t.Fatalf("classify stair: %v", err)
			}
			stairs = append(stairs, r)
		}
	}
	if err := validateSelectorCardinality(stairs); err != nil {
		t.Fatalf("stair selector: %v", err)
	}
	for _, record := range stairs {
		if record.ModelFamily != ModelFamilyStair || record.ContributorRole != ContributorPrimary {
			t.Fatalf("stair classification = family %v role %v", record.ModelFamily, record.ContributorRole)
		}
		if record.FaceCoverage != 0 {
			t.Fatalf("stair coverage = %#x, want conservative empty", record.FaceCoverage)
		}
	}

	fixtures := []struct {
		state  SourceState
		family ModelFamily
		role   ContributorRole
		field  ModelStateField
		value  uint32
	}{
		{sourceState("minecraft:short_grass"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:fern"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:red_flower", intState("flower_type", 0)), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:wheat", intState("growth", 7)), ModelFamilyCrop, ContributorPrimary, ModelStateGrowth, 7},
		{sourceState("minecraft:melon_stem", intState("growth", 4)), ModelFamilyCrop, ContributorPrimary, ModelStateGrowth, 4},
		{sourceState("minecraft:water", intState("liquid_depth", 15)), ModelFamilyLiquid, ContributorLiquidAdditional, ModelStateLiquidDepth, 15},
		{sourceState("minecraft:flowing_water", intState("liquid_depth", 3)), ModelFamilyLiquid, ContributorLiquidAdditional, ModelStateLiquidDepth, 3},
		{sourceState("minecraft:stone_block_slab", intState("vertical_half", 0)), ModelFamilySlab, ContributorPrimary, ModelStateHalf, 0},
		{sourceState("minecraft:stone_block_slab", intState("vertical_half", 1)), ModelFamilySlab, ContributorPrimary, ModelStateHalf, 1},
		{sourceState("minecraft:double_stone_block_slab"), ModelFamilySlab, ContributorPrimary, ModelStateHalf, 2},
		{sourceState("minecraft:cinnabar_slab", StateProperty{Name: "minecraft:vertical_half", Value: TypedScalar{Kind: ScalarString, String: "top"}}), ModelFamilySlab, ContributorPrimary, ModelStateHalf, 1},
		{sourceState("minecraft:test", StateProperty{Name: "cardinal_direction", Value: TypedScalar{Kind: ScalarString, String: "north"}}), ModelFamilyUnknown, ContributorPrimary, ModelStateOrientation, 2},
		{sourceState("minecraft:vine", intState("vine_direction_bits", 9)), ModelFamilyVine, ContributorPrimary, ModelStateConnections, 9},
		{sourceState("minecraft:iron_bars"), ModelFamilyPane, ContributorPrimary, 0, 0},
		{sourceState("minecraft:red_bed", intState("direction", 2)), ModelFamilyBed, ContributorPrimary, ModelStateOrientation, 2},
		{sourceState("minecraft:dandelion"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:tall_seagrass"), ModelFamilyAquatic, ContributorPrimary, 0, 0},
		{sourceState("minecraft:cocoa", intState("age", 2)), ModelFamilyCocoa, ContributorPrimary, ModelStateGrowth, 2},
		{sourceState("minecraft:tube_coral_block"), ModelFamilyUnknown, ContributorPrimary, 0, 0},
		{sourceState("minecraft:sea_pickle", intState("cluster_count", 2)), ModelFamilyUnknown, ContributorPrimary, 0, 0},
		{sourceState("minecraft:copper_bars"), ModelFamilyPane, ContributorPrimary, 0, 0},
		{sourceState("minecraft:torchflower"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:lever", StateProperty{Name: "lever_direction", Value: TypedScalar{Kind: ScalarString, String: "north"}}), ModelFamilyLever, ContributorPrimary, ModelStateOrientation, 4},
		{sourceState("minecraft:barrier"), ModelFamilyInvisible, ContributorPrimary, 0, 0},
		{sourceState("minecraft:dragon_egg"), ModelFamilyDecorative, ContributorPrimary, 0, 0},
		{sourceState("minecraft:brown_mushroom"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:deadbush"), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:cave_vines", intState("growing_plant_age", 4)), ModelFamilyCross, ContributorPrimary, 0, 0},
		{sourceState("minecraft:white_glazed_terracotta", intState("facing_direction", 2)), ModelFamilyCube, ContributorPrimary, ModelStateOrientation, 2},
		{sourceState("minecraft:chorus_flower", intState("age", 3)), ModelFamilyUnknown, ContributorPrimary, ModelStateGrowth, 3},
		{sourceState("minecraft:waxed_copper_golem_statue", intState("copper_golem_pose", 1)), ModelFamilyStatue, ContributorPrimary, 0, 0},
		{sourceState("minecraft:soul_sand"), ModelFamilyCuboid, ContributorPrimary, 0, 0},
		{sourceState("minecraft:mud"), ModelFamilyCuboid, ContributorPrimary, 0, 0},
		{sourceState("minecraft:cobblestone_wall",
			StateProperty{Name: "wall_connection_type_north", Value: TypedScalar{Kind: ScalarString, String: "short"}},
			StateProperty{Name: "wall_connection_type_east", Value: TypedScalar{Kind: ScalarString, String: "tall"}},
			StateProperty{Name: "wall_connection_type_south", Value: TypedScalar{Kind: ScalarString, String: "none"}},
			StateProperty{Name: "wall_connection_type_west", Value: TypedScalar{Kind: ScalarString, String: "short"}},
			byteState("wall_post_bit", 1)), ModelFamilyWall, ContributorPrimary, ModelStateConnections, 329},
	}
	for _, fixture := range fixtures {
		record, err := classifyRecord(fixture.state)
		if err != nil {
			t.Fatalf("classify %s: %v", fixture.state.Name, err)
		}
		if record.ModelFamily != fixture.family || record.ContributorRole != fixture.role {
			t.Errorf("%s = family %v role %v", fixture.state.Name, record.ModelFamily, record.ContributorRole)
		}
		if fixture.field != 0 {
			value, ok := record.ModelState.Get(fixture.field)
			if !ok || value != fixture.value {
				t.Errorf("%s field %v = %d/%v, want %d", fixture.state.Name, fixture.field, value, ok, fixture.value)
			}
		}
	}

	broken := append([]Record(nil), stairs[:7]...)
	if err := validateSelectorCardinality(broken); err == nil || !strings.Contains(err.Error(), "selector cardinality") {
		t.Fatalf("broken selector error = %v", err)
	}
}

func TestVineClassificationCoversEveryProtocol1001DirectionMask(t *testing.T) {
	for mask := uint32(0); mask < 16; mask++ {
		record, err := classifyRecord(sourceState("minecraft:vine", intState("vine_direction_bits", int32(mask))))
		if err != nil {
			t.Fatalf("mask %d: classify: %v", mask, err)
		}
		if record.ModelFamily != ModelFamilyVine {
			t.Errorf("mask %d: family=%v, want Vine", mask, record.ModelFamily)
		}
		connections, ok := record.ModelState.Get(ModelStateConnections)
		if !ok || connections != mask {
			t.Errorf("mask %d: connections=(%d, %t)", mask, connections, ok)
		}
		if record.Flags&(flagCubeGeometry|flagOccludesFullFace) != 0 || record.FaceCoverage != 0 {
			t.Errorf("mask %d: vine acquired full-block geometry/occlusion: flags=%#x coverage=%#x", mask, record.Flags, record.FaceCoverage)
		}
	}
}

func TestChiseledBookshelfClassificationRequiresExactTypedSelectors(t *testing.T) {
	const wantMask = uint8(1<<(ModelStateOrientation-1) | 1<<(ModelStateConnections-1))
	for books := int32(0); books < 64; books++ {
		for direction := int32(0); direction < 4; direction++ {
			record, err := classifyRecord(sourceState(
				"minecraft:chiseled_bookshelf",
				intState("books_stored", books),
				intState("direction", direction),
			))
			if err != nil {
				t.Fatalf("classify books=%d direction=%d: %v", books, direction, err)
			}
			if record.ModelFamily != ModelFamilyChiseledBookshelf {
				t.Fatalf("books=%d direction=%d family=%v", books, direction, record.ModelFamily)
			}
			if record.ModelState.Mask != wantMask {
				t.Fatalf("books=%d direction=%d mask=%#x, want %#x", books, direction, record.ModelState.Mask, wantMask)
			}
			if got, ok := record.ModelState.Get(ModelStateConnections); !ok || got != uint32(books) {
				t.Fatalf("books selector=%d/%v, want %d/true", got, ok, books)
			}
			if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != uint32(direction) {
				t.Fatalf("direction selector=%d/%v, want %d/true", got, ok, direction)
			}
		}
	}

	invalid := []SourceState{
		sourceState("minecraft:chiseled_bookshelf", intState("books_stored", -1), intState("direction", 0)),
		sourceState("minecraft:chiseled_bookshelf", intState("books_stored", 64), intState("direction", 0)),
		sourceState("minecraft:chiseled_bookshelf", intState("books_stored", 0), intState("direction", -1)),
		sourceState("minecraft:chiseled_bookshelf", intState("books_stored", 0), intState("direction", 4)),
		sourceState("minecraft:chiseled_bookshelf", byteState("books_stored", 0), intState("direction", 0)),
		sourceState("minecraft:chiseled_bookshelf", intState("books_stored", 0), byteState("direction", 0)),
		sourceState("minecraft:chiseled_bookshelf", intState("minecraft:books_stored", 0), intState("direction", 0)),
		sourceState("minecraft:chiseled_bookshelf", intState("books_stored", 0), intState("minecraft:direction", 0)),
		sourceState("minecraft:chiseled_bookshelf", intState("books_stored", 0)),
		sourceState("minecraft:chiseled_bookshelf", intState("books_stored", 0), intState("direction", 0), intState("extra", 0)),
	}
	for index, state := range invalid {
		if _, err := classifyRecord(state); err == nil {
			t.Errorf("invalid selector fixture %d was accepted", index)
		}
	}

	unrelated, err := classifyRecord(sourceState(
		"minecraft:bookshelf",
		intState("books_stored", 1),
		intState("direction", 2),
	))
	if err != nil {
		t.Fatalf("classify unrelated shelf: %v", err)
	}
	if unrelated.ModelFamily == ModelFamilyChiseledBookshelf {
		t.Fatal("non-exact bookshelf name entered chiseled family")
	}
}

func TestBeeHousingClassificationRequiresExactOrderIndependentTypedSelectors(t *testing.T) {
	const wantMask = uint8(1<<(ModelStateOrientation-1) | 1<<(ModelStateGrowth-1))
	for _, name := range []string{"minecraft:bee_nest", "minecraft:beehive"} {
		for honey := int32(0); honey < 6; honey++ {
			for direction := int32(0); direction < 4; direction++ {
				for _, properties := range [][]StateProperty{
					{intState("direction", direction), intState("honey_level", honey)},
					{intState("honey_level", honey), intState("direction", direction)},
				} {
					record, err := classifyRecord(sourceState(name, properties...))
					if err != nil {
						t.Fatalf("%s honey=%d direction=%d: %v", name, honey, direction, err)
					}
					if record.ModelFamily != ModelFamilyCube || record.ContributorRole != ContributorPrimary {
						t.Fatalf("%s honey=%d direction=%d family/role=%v/%v", name, honey, direction, record.ModelFamily, record.ContributorRole)
					}
					if record.ModelState.Mask != wantMask {
						t.Fatalf("%s honey=%d direction=%d mask=%#x, want %#x", name, honey, direction, record.ModelState.Mask, wantMask)
					}
					if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != uint32(direction) {
						t.Fatalf("%s direction projection=%d/%v, want %d/true", name, got, ok, direction)
					}
					if got, ok := record.ModelState.Get(ModelStateGrowth); !ok || got != uint32(honey) {
						t.Fatalf("%s honey projection=%d/%v, want %d/true", name, got, ok, honey)
					}
				}
			}
		}
	}

	for index, state := range []SourceState{
		sourceState("minecraft:bee_nest", intState("direction", -1), intState("honey_level", 0)),
		sourceState("minecraft:bee_nest", intState("direction", 4), intState("honey_level", 0)),
		sourceState("minecraft:bee_nest", intState("direction", 0), intState("honey_level", -1)),
		sourceState("minecraft:bee_nest", intState("direction", 0), intState("honey_level", 6)),
		sourceState("minecraft:bee_nest", byteState("direction", 0), intState("honey_level", 0)),
		sourceState("minecraft:bee_nest", intState("direction", 0), byteState("honey_level", 0)),
		sourceState("minecraft:bee_nest", intState("minecraft:direction", 0), intState("honey_level", 0)),
		sourceState("minecraft:bee_nest", intState("direction", 0), intState("minecraft:honey_level", 0)),
		sourceState("minecraft:bee_nest", intState("direction", 0)),
		sourceState("minecraft:bee_nest", intState("direction", 0), intState("honey_level", 0), intState("extra", 0)),
	} {
		if _, err := classifyRecord(state); err == nil {
			t.Errorf("invalid bee selector fixture %d was accepted", index)
		}
	}

	unrelated, err := classifyRecord(sourceState(
		"minecraft:honey_block",
		intState("direction", 0),
		intState("honey_level", 5),
	))
	if err != nil {
		t.Fatalf("classify unrelated honey block: %v", err)
	}
	if unrelated.ModelFamily == ModelFamilyCube && unrelated.ModelState.Mask == wantMask {
		t.Fatal("unrelated honey block entered the exact bee-housing family")
	}
}

func TestShelfClassificationRequiresExactOrderIndependentTypedSelectors(t *testing.T) {
	const wantMask = uint8(1<<(ModelStateOrientation-1) | 1<<(ModelStateGrowth-1) | 1<<(ModelStateFlags-1))
	directions := []string{"south", "west", "north", "east"}
	for direction, name := range directions {
		for powered := byte(0); powered < 2; powered++ {
			for shelfType := int32(0); shelfType < 4; shelfType++ {
				for _, properties := range [][]StateProperty{
					{stringState("minecraft:cardinal_direction", name), byteState("powered_bit", powered), intState("powered_shelf_type", shelfType)},
					{intState("powered_shelf_type", shelfType), stringState("minecraft:cardinal_direction", name), byteState("powered_bit", powered)},
				} {
					record, err := classifyRecord(sourceState("minecraft:oak_shelf", properties...))
					if err != nil {
						t.Fatalf("direction=%s powered=%d type=%d: %v", name, powered, shelfType, err)
					}
					if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
						t.Fatalf("direction=%s powered=%d type=%d family/role=%v/%v", name, powered, shelfType, record.ModelFamily, record.ContributorRole)
					}
					if record.ModelState.Mask != wantMask {
						t.Fatalf("direction=%s powered=%d type=%d mask=%#x, want %#x", name, powered, shelfType, record.ModelState.Mask, wantMask)
					}
					if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != uint32(direction) {
						t.Fatalf("direction projection=%d/%v, want %d/true", got, ok, direction)
					}
					if got, ok := record.ModelState.Get(ModelStateGrowth); !ok || got != uint32(shelfType) {
						t.Fatalf("shelf-type projection=%d/%v, want %d/true", got, ok, shelfType)
					}
					wantFlags := uint32(0)
					if powered != 0 {
						wantFlags = modelFlagPowered
					}
					if got, ok := record.ModelState.Get(ModelStateFlags); !ok || got != wantFlags {
						t.Fatalf("powered projection=%d/%v, want %d/true", got, ok, wantFlags)
					}
				}
			}
		}
	}

	for index, state := range []SourceState{
		sourceState("minecraft:oak_shelf", stringState("minecraft:cardinal_direction", "up"), byteState("powered_bit", 0), intState("powered_shelf_type", 0)),
		sourceState("minecraft:oak_shelf", stringState("minecraft:cardinal_direction", "south"), byteState("powered_bit", 2), intState("powered_shelf_type", 0)),
		sourceState("minecraft:oak_shelf", stringState("minecraft:cardinal_direction", "south"), byteState("powered_bit", 0), intState("powered_shelf_type", -1)),
		sourceState("minecraft:oak_shelf", stringState("minecraft:cardinal_direction", "south"), byteState("powered_bit", 0), intState("powered_shelf_type", 4)),
		sourceState("minecraft:oak_shelf", stringState("cardinal_direction", "south"), byteState("powered_bit", 0), intState("powered_shelf_type", 0)),
		sourceState("minecraft:oak_shelf", stringState("minecraft:cardinal_direction", "south"), intState("powered_bit", 0), intState("powered_shelf_type", 0)),
		sourceState("minecraft:oak_shelf", stringState("minecraft:cardinal_direction", "south"), byteState("powered_bit", 0), byteState("powered_shelf_type", 0)),
		sourceState("minecraft:oak_shelf", stringState("minecraft:cardinal_direction", "south"), byteState("powered_bit", 0)),
		sourceState("minecraft:oak_shelf", stringState("minecraft:cardinal_direction", "south"), byteState("powered_bit", 0), intState("powered_shelf_type", 0), intState("extra", 0)),
	} {
		if _, err := classifyRecord(state); err == nil {
			t.Errorf("invalid shelf selector fixture %d was accepted", index)
		}
	}

	unrelated, err := classifyRecord(sourceState(
		"minecraft:bookshelf",
		stringState("minecraft:cardinal_direction", "south"),
		byteState("powered_bit", 0),
		intState("powered_shelf_type", 0),
	))
	if err != nil {
		t.Fatalf("classify unrelated bookshelf: %v", err)
	}
	if unrelated.ModelFamily == ModelFamilyCuboid && unrelated.ModelState.Mask == wantMask {
		t.Fatal("unrelated bookshelf entered exact shelf family")
	}
}

func exactShelfRecords(t *testing.T) []Record {
	t.Helper()
	families := []struct {
		name string
		base uint32
	}{
		{"minecraft:acacia_shelf", 383},
		{"minecraft:bamboo_shelf", 6513},
		{"minecraft:birch_shelf", 302},
		{"minecraft:cherry_shelf", 14007},
		{"minecraft:crimson_shelf", 13882},
		{"minecraft:dark_oak_shelf", 9131},
		{"minecraft:jungle_shelf", 6045},
		{"minecraft:mangrove_shelf", 5280},
		{"minecraft:oak_shelf", 6897},
		{"minecraft:pale_oak_shelf", 11080},
		{"minecraft:spruce_shelf", 5162},
		{"minecraft:warped_shelf", 5313},
	}
	directions := []struct {
		name  string
		shape uint16
		box   CollisionBox
	}{
		{"south", 18, CollisionBox{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 31_250_000}},
		{"west", 19, CollisionBox{MinX: 68_750_000, MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}},
		{"north", 20, CollisionBox{MinZ: 68_750_000, MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}},
		{"east", 21, CollisionBox{MaxX: 31_250_000, MaxY: 100_000_000, MaxZ: 100_000_000}},
	}
	records := make([]Record, 0, len(families)*32)
	for _, family := range families {
		for direction, fixture := range directions {
			for powered := byte(0); powered < 2; powered++ {
				for shelfType := int32(0); shelfType < 4; shelfType++ {
					record, err := classifyRecord(sourceState(
						family.name,
						stringState("minecraft:cardinal_direction", fixture.name),
						byteState("powered_bit", powered),
						intState("powered_shelf_type", shelfType),
					))
					if err != nil {
						t.Fatalf("classify %s: %v", family.name, err)
					}
					offset := uint32(direction)*8 + uint32(powered)*4 + uint32(shelfType)
					record.SequentialID = family.base + offset
					record.NetworkHash = 100_000 + record.SequentialID
					record.CollisionSeed = CollisionSeed{
						ShapeID: fixture.shape, Confidence: CollisionConfidenceCollisionOnly,
						Boxes: []CollisionBox{fixture.box},
					}
					finalizeGeometryFacts(&record)
					records = append(records, record)
				}
			}
		}
	}
	return records
}

func TestShelfProductsRequireExactCanonicalIdsProjectionAndDirectionalCollision(t *testing.T) {
	records := exactShelfRecords(t)
	if err := validateSelectorCardinality(records); err != nil {
		t.Fatalf("valid shelf selector products: %v", err)
	}
	for _, record := range records {
		if record.Flags != 0 || record.FaceCoverage != 0 {
			t.Fatalf("state %d acquired full-face geometry: flags/coverage=%#x/%#x", record.SequentialID, record.Flags, record.FaceCoverage)
		}
	}

	mutations := []struct {
		name   string
		mutate func([]Record)
	}{
		{"missing state", func(records []Record) { records[31] = records[30] }},
		{"sequential ID", func(records []Record) { records[0].SequentialID++ }},
		{"canonical alias", func(records []Record) {
			records[0].StateJSON = []byte(`{"cardinal_direction":{"type":"string","value":"south"},"powered_bit":{"type":"byte","value":0},"powered_shelf_type":{"type":"int","value":0}}`)
		}},
		{"projection disagreement", func(records []Record) { records[0].ModelState.Set(ModelStateGrowth, 1) }},
		{"extra projection", func(records []Record) { records[0].ModelState.Set(ModelStateOpen, 0) }},
		{"flags", func(records []Record) { records[0].Flags = flagCubeGeometry }},
		{"face coverage", func(records []Record) { records[0].FaceCoverage = 1 }},
		{"shape ID", func(records []Record) { records[0].CollisionSeed.ShapeID++ }},
		{"confidence", func(records []Record) { records[0].CollisionSeed.Confidence = CollisionConfidenceReviewedVisibleBounds }},
		{"collision bounds", func(records []Record) { records[0].CollisionSeed.Boxes[0].MaxZ++ }},
	}
	for _, mutation := range mutations {
		t.Run(mutation.name, func(t *testing.T) {
			broken := cloneRecords(records)
			mutation.mutate(broken)
			if err := validateSelectorCardinality(broken); err == nil {
				t.Fatal("invalid shelf selector products were accepted")
			}
		})
	}
}

func TestShelfInventoryRejectsWholeMissingAndUnexpectedFamilies(t *testing.T) {
	records := exactShelfRecords(t)

	t.Run("whole expected family missing", func(t *testing.T) {
		broken := make([]Record, 0, len(records)-32)
		for _, record := range records {
			if record.Name != "minecraft:oak_shelf" {
				broken = append(broken, record)
			}
		}
		if err := validateSelectorCardinality(broken); err == nil {
			t.Fatal("shelf inventory missing the complete oak family was accepted")
		}
	})

	t.Run("unexpected family replaces expected family", func(t *testing.T) {
		broken := cloneRecords(records)
		for index := range broken {
			if broken[index].Name == "minecraft:oak_shelf" {
				broken[index].Name = "minecraft:unexpected_shelf"
			}
		}
		if err := validateSelectorCardinality(broken); err == nil {
			t.Fatal("shelf inventory with an unexpected replacement family was accepted")
		}
	})
}

func TestBeeHousingSelectorProductRequiresExactCanonicalIdsAndUnitCollision(t *testing.T) {
	records := make([]Record, 0, 48)
	for _, family := range []struct {
		name string
		base uint32
	}{
		{"minecraft:bee_nest", 10_395},
		{"minecraft:beehive", 12_495},
	} {
		for honey := int32(0); honey < 6; honey++ {
			for direction := int32(0); direction < 4; direction++ {
				record, err := classifyRecord(sourceState(
					family.name,
					intState("honey_level", honey),
					intState("direction", direction),
				))
				if err != nil {
					t.Fatalf("classify %s: %v", family.name, err)
				}
				offset := uint32(honey)*4 + uint32(direction)
				record.SequentialID = family.base + offset
				record.NetworkHash = record.SequentialID + 1
				record.CollisionSeed = CollisionSeed{
					ShapeID:    1,
					Confidence: CollisionConfidenceCollisionOnly,
					Boxes:      []CollisionBox{{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}},
				}
				finalizeGeometryFacts(&record)
				records = append(records, record)
			}
		}
	}
	if err := validateSelectorCardinality(records); err != nil {
		t.Fatalf("valid bee selector products: %v", err)
	}
	for _, record := range records {
		if record.Flags != flagCubeGeometry|flagOccludesFullFace || record.FaceCoverage != 0x3f {
			t.Fatalf("state %d flags/coverage=%#x/%#x", record.SequentialID, record.Flags, record.FaceCoverage)
		}
	}

	for _, mutation := range []struct {
		name   string
		mutate func([]Record)
	}{
		{"missing state", func(records []Record) { records[23] = records[22] }},
		{"sequential ID", func(records []Record) { records[0].SequentialID++ }},
		{"canonical alias", func(records []Record) {
			records[0].StateJSON = []byte(`{"direction":{"type":"int","value":0},"minecraft:honey_level":{"type":"int","value":0}}`)
		}},
		{"projection disagreement", func(records []Record) { records[0].ModelState.Set(ModelStateGrowth, 1) }},
		{"flags", func(records []Record) { records[0].Flags = 0 }},
		{"unit bounds", func(records []Record) { records[0].CollisionSeed.Boxes[0].MaxY-- }},
		{"shape ID", func(records []Record) { records[0].CollisionSeed.ShapeID = 2 }},
		{"confidence", func(records []Record) { records[0].CollisionSeed.Confidence = CollisionConfidenceReviewedVisibleBounds }},
	} {
		t.Run(mutation.name, func(t *testing.T) {
			broken := append([]Record(nil), records...)
			for index := range broken {
				broken[index].CollisionSeed.Boxes = append([]CollisionBox(nil), records[index].CollisionSeed.Boxes...)
			}
			mutation.mutate(broken)
			if err := validateSelectorCardinality(broken); err == nil {
				t.Fatal("invalid bee selector products were accepted")
			}
		})
	}
}

func TestChiseledBookshelfSelectorProductRequiresCanonicalIdsAndUnitCollision(t *testing.T) {
	records := make([]Record, 0, 256)
	for books := int32(0); books < 64; books++ {
		for direction := int32(0); direction < 4; direction++ {
			record, err := classifyRecord(sourceState(
				"minecraft:chiseled_bookshelf",
				intState("books_stored", books),
				intState("direction", direction),
			))
			if err != nil {
				t.Fatalf("classify: %v", err)
			}
			record.SequentialID = 1605 + uint32(books)*4 + uint32(direction)
			record.NetworkHash = record.SequentialID + 1
			record.Flags = flagCubeGeometry | flagOccludesFullFace
			record.CollisionSeed = CollisionSeed{
				ShapeID:    1,
				Confidence: CollisionConfidenceCollisionOnly,
				Boxes:      []CollisionBox{{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}},
			}
			finalizeGeometryFacts(&record)
			records = append(records, record)
		}
	}
	if err := validateSelectorCardinality(records); err != nil {
		t.Fatalf("valid selector product: %v", err)
	}
	for _, record := range records {
		if record.FaceCoverage != 0x3f {
			t.Fatalf("state %d face coverage=%#x", record.SequentialID, record.FaceCoverage)
		}
	}

	aliased := append([]Record(nil), records...)
	for index := range aliased {
		state := strings.ReplaceAll(string(aliased[index].StateJSON), `"books_stored"`, `"minecraft:books_stored"`)
		state = strings.ReplaceAll(state, `"direction"`, `"minecraft:direction"`)
		aliased[index].StateJSON = []byte(state)
	}
	if err := validateSelectorCardinality(aliased); err == nil {
		t.Error("complete product with namespace-prefixed selector aliases was accepted")
	}

	for _, mutation := range []struct {
		name   string
		mutate func([]Record)
	}{
		{"sequential ID", func(records []Record) { records[0].SequentialID++ }},
		{"flags", func(records []Record) { records[0].Flags = 0 }},
		{"unit bounds", func(records []Record) { records[0].CollisionSeed.Boxes[0].MaxY-- }},
		{"shape ID", func(records []Record) { records[0].CollisionSeed.ShapeID = 2 }},
		{"confidence", func(records []Record) {
			records[0].CollisionSeed.Confidence = CollisionConfidenceReviewedVisibleBounds
		}},
	} {
		t.Run(mutation.name, func(t *testing.T) {
			broken := append([]Record(nil), records...)
			broken[0].CollisionSeed.Boxes = append([]CollisionBox(nil), records[0].CollisionSeed.Boxes...)
			mutation.mutate(broken)
			if err := validateSelectorCardinality(broken); err == nil {
				t.Fatal("invalid chiseled bookshelf product was accepted")
			}
		})
	}
}

func TestResinClumpClassificationRequiresExactTypedSelector(t *testing.T) {
	const wantMask = uint8(1 << (ModelStateConnections - 1))
	for mask := int32(0); mask < 64; mask++ {
		record, err := classifyRecord(sourceState(
			"minecraft:resin_clump",
			intState("multi_face_direction_bits", mask),
		))
		if err != nil {
			t.Fatalf("classify mask %d: %v", mask, err)
		}
		if record.ModelFamily != ModelFamilyResinClump {
			t.Fatalf("mask %d family=%v", mask, record.ModelFamily)
		}
		if record.ModelState.Mask != wantMask {
			t.Fatalf("mask %d model-state mask=%#x, want %#x", mask, record.ModelState.Mask, wantMask)
		}
		if got, ok := record.ModelState.Get(ModelStateConnections); !ok || got != uint32(mask) {
			t.Fatalf("mask %d connections=%d/%v", mask, got, ok)
		}
	}

	invalid := []SourceState{
		sourceState("minecraft:resin_clump"),
		sourceState("minecraft:resin_clump", intState("multi_face_direction_bits", -1)),
		sourceState("minecraft:resin_clump", intState("multi_face_direction_bits", 64)),
		sourceState("minecraft:resin_clump", byteState("multi_face_direction_bits", 1)),
		sourceState("minecraft:resin_clump", StateProperty{Name: "multi_face_direction_bits", Value: TypedScalar{Kind: ScalarString, String: "1"}}),
		sourceState("minecraft:resin_clump", intState("minecraft:multi_face_direction_bits", 1)),
		sourceState("minecraft:resin_clump", intState("direction_bits", 1)),
		sourceState("minecraft:resin_clump", intState("multi_face_direction_bits", 1), intState("extra", 0)),
	}
	for index, state := range invalid {
		if _, err := classifyRecord(state); err == nil {
			t.Errorf("invalid selector fixture %d was accepted", index)
		}
	}

	unrelated, err := classifyRecord(sourceState(
		"minecraft:resin_block",
		intState("multi_face_direction_bits", 1),
	))
	if err != nil {
		t.Fatalf("classify unrelated resin block: %v", err)
	}
	if unrelated.ModelFamily == ModelFamilyResinClump {
		t.Fatal("non-exact resin name entered resin-clump family")
	}
}

func TestCactusClassificationRequiresExactTypedAge(t *testing.T) {
	const wantMask = uint8(1 << (ModelStateGrowth - 1))
	for age := int32(0); age < 16; age++ {
		record, err := classifyRecord(sourceState("minecraft:cactus", intState("age", age)))
		if err != nil {
			t.Fatalf("classify age %d: %v", age, err)
		}
		if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			t.Fatalf("age %d family/role=%v/%v", age, record.ModelFamily, record.ContributorRole)
		}
		if record.ModelState.Mask != wantMask {
			t.Fatalf("age %d model-state mask=%#x, want %#x", age, record.ModelState.Mask, wantMask)
		}
		if got, ok := record.ModelState.Get(ModelStateGrowth); !ok || got != uint32(age) {
			t.Fatalf("age %d growth=%d/%v", age, got, ok)
		}
	}

	invalid := []SourceState{
		sourceState("minecraft:cactus"),
		sourceState("minecraft:cactus", intState("age", -1)),
		sourceState("minecraft:cactus", intState("age", 16)),
		sourceState("minecraft:cactus", byteState("age", 1)),
		sourceState("minecraft:cactus", StateProperty{Name: "age", Value: TypedScalar{Kind: ScalarString, String: "1"}}),
		sourceState("minecraft:cactus", intState("minecraft:age", 1)),
		sourceState("minecraft:cactus", intState("growth", 1)),
		sourceState("minecraft:cactus", intState("age", 1), intState("extra", 0)),
		sourceState("minecraft:cactus", intState("age", 1), intState("age", 1)),
	}
	for index, state := range invalid {
		if _, err := classifyRecord(state); err == nil {
			t.Errorf("invalid cactus selector fixture %d was accepted", index)
		}
	}

	unrelated, err := classifyRecord(sourceState("minecraft:chorus_flower", intState("age", 1)))
	if err != nil {
		t.Fatalf("classify unrelated age-bearing block: %v", err)
	}
	if unrelated.ModelFamily == ModelFamilyCuboid {
		t.Fatal("non-exact cactus name entered cactus cuboid family")
	}
}

func exactCactusRecords(t *testing.T) []Record {
	t.Helper()
	records := make([]Record, 0, 16)
	for age := int32(0); age < 16; age++ {
		record, err := classifyRecord(sourceState("minecraft:cactus", intState("age", age)))
		if err != nil {
			t.Fatalf("classify age %d: %v", age, err)
		}
		record.SequentialID = 13606 + uint32(age)
		record.NetworkHash = 130_000 + uint32(age)
		record.Flags = 0
		record.FaceCoverage = 0
		record.CollisionSeed = CollisionSeed{
			ShapeID:    84,
			Confidence: CollisionConfidenceCollisionOnly,
			Boxes: []CollisionBox{{
				MinX: 6_250_000, MaxX: 93_750_000,
				MinY: 0, MaxY: 100_000_000,
				MinZ: 6_250_000, MaxZ: 93_750_000,
			}},
		}
		record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
		records = append(records, record)
	}
	return records
}

func TestCactusProductRequiresExactIdsStateProjectionAndCollision(t *testing.T) {
	records := exactCactusRecords(t)
	if err := validateSelectorCardinality(records); err != nil {
		t.Fatalf("valid cactus product: %v", err)
	}
	forward, err := encode(records)
	if err != nil {
		t.Fatalf("encode forward: %v", err)
	}
	reversed := cloneRecords(records)
	slices.Reverse(reversed)
	backward, err := encode(reversed)
	if err != nil {
		t.Fatalf("encode reversed: %v", err)
	}
	if !bytes.Equal(forward, backward) {
		t.Fatal("cactus BREG encoding depends on source order")
	}

	mutations := []struct {
		name   string
		mutate func([]Record)
	}{
		{"missing state", func(records []Record) { records[0].StateJSON = []byte(`{}`) }},
		{"aliased key", func(records []Record) {
			records[0].StateJSON = []byte(`{"minecraft:age":{"type":"int","value":0}}`)
		}},
		{"wrong canonical type", func(records []Record) {
			records[0].StateJSON = []byte(`{"age":{"type":"byte","value":0}}`)
		}},
		{"out of range", func(records []Record) {
			records[0].StateJSON = []byte(`{"age":{"type":"int","value":16}}`)
		}},
		{"extra canonical key", func(records []Record) {
			records[0].StateJSON = []byte(`{"age":{"type":"int","value":0},"extra":{"type":"int","value":0}}`)
		}},
		{"model-state disagreement", func(records []Record) { records[0].ModelState.Set(ModelStateGrowth, 1) }},
		{"extra model-state field", func(records []Record) { records[0].ModelState.Set(ModelStateOrientation, 0) }},
		{"wrong role", func(records []Record) { records[0].ContributorRole = ContributorLiquidAdditional }},
		{"wrong family", func(records []Record) { records[0].ModelFamily = ModelFamilyCrop }},
		{"wrong ID", func(records []Record) { records[0].SequentialID++ }},
		{"flags", func(records []Record) { records[0].Flags = flagCubeGeometry }},
		{"face coverage", func(records []Record) { records[0].FaceCoverage = 1 }},
		{"shape ID", func(records []Record) { records[0].CollisionSeed.ShapeID = 1 }},
		{"confidence", func(records []Record) { records[0].CollisionSeed.Confidence = CollisionConfidenceReviewedVisibleBounds }},
		{"collision bounds", func(records []Record) { records[0].CollisionSeed.Boxes[0].MinX++ }},
		{"duplicate selector", func(records []Record) {
			records[1].StateJSON = append([]byte(nil), records[0].StateJSON...)
			records[1].ModelState = records[0].ModelState
		}},
	}
	for _, mutation := range mutations {
		t.Run(mutation.name, func(t *testing.T) {
			broken := cloneRecords(records)
			mutation.mutate(broken)
			if err := validateSelectorCardinality(broken); err == nil {
				t.Fatal("invalid cactus product was accepted")
			}
		})
	}
	if err := validateSelectorCardinality(records[:15]); err == nil {
		t.Fatal("incomplete cactus product was accepted")
	}
}

func TestCakeClassificationRequiresExactTypedBiteCounter(t *testing.T) {
	const wantMask = uint8(1 << (ModelStateGrowth - 1))
	for bite := int32(0); bite < 7; bite++ {
		record, err := classifyRecord(sourceState("minecraft:cake", intState("bite_counter", bite)))
		if err != nil {
			t.Fatalf("classify bite %d: %v", bite, err)
		}
		if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			t.Fatalf("bite %d family/role=%v/%v", bite, record.ModelFamily, record.ContributorRole)
		}
		if record.ModelState.Mask != wantMask {
			t.Fatalf("bite %d model-state mask=%#x, want %#x", bite, record.ModelState.Mask, wantMask)
		}
		if got, ok := record.ModelState.Get(ModelStateGrowth); !ok || got != uint32(bite) {
			t.Fatalf("bite %d growth=%d/%v", bite, got, ok)
		}
	}

	invalid := []SourceState{
		sourceState("minecraft:cake"),
		sourceState("minecraft:cake", intState("bite_counter", -1)),
		sourceState("minecraft:cake", intState("bite_counter", 7)),
		sourceState("minecraft:cake", byteState("bite_counter", 1)),
		sourceState("minecraft:cake", StateProperty{Name: "bite_counter", Value: TypedScalar{Kind: ScalarString, String: "1"}}),
		sourceState("minecraft:cake", intState("minecraft:bite_counter", 1)),
		sourceState("minecraft:cake", intState("bites", 1)),
		sourceState("minecraft:cake", intState("bite_counter", 1), intState("extra", 0)),
		sourceState("minecraft:cake", intState("bite_counter", 1), intState("bite_counter", 1)),
	}
	for index, state := range invalid {
		if _, err := classifyRecord(state); err == nil {
			t.Errorf("invalid cake selector fixture %d was accepted", index)
		}
	}

	unrelated, err := classifyRecord(sourceState("minecraft:candle_cake", intState("bite_counter", 1)))
	if err != nil {
		t.Fatalf("classify unrelated cake: %v", err)
	}
	if unrelated.ModelFamily == ModelFamilyCuboid {
		t.Fatal("non-exact cake name entered cake cuboid family")
	}
}

func exactCakeRecords(t *testing.T) []Record {
	t.Helper()
	mins := [...]int32{6_250_000, 18_750_000, 31_250_000, 43_750_000, 56_250_000, 68_750_000, 81_250_000}
	records := make([]Record, 0, len(mins))
	for bite, minX := range mins {
		record, err := classifyRecord(sourceState("minecraft:cake", intState("bite_counter", int32(bite))))
		if err != nil {
			t.Fatalf("classify bite %d: %v", bite, err)
		}
		record.SequentialID = 14055 + uint32(bite)
		record.NetworkHash = 140_000 + uint32(bite)
		record.Flags = 0
		record.FaceCoverage = 0
		record.CollisionSeed = CollisionSeed{
			ShapeID:    uint16(89 + bite),
			Confidence: CollisionConfidenceCollisionOnly,
			Boxes: []CollisionBox{{
				MinX: minX, MaxX: 93_750_000,
				MinY: 0, MaxY: 50_000_000,
				MinZ: 6_250_000, MaxZ: 93_750_000,
			}},
		}
		record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
		records = append(records, record)
	}
	return records
}

func TestCakeProductRequiresExactIdsStateProjectionAndCollision(t *testing.T) {
	records := exactCakeRecords(t)
	if err := validateSelectorCardinality(records); err != nil {
		t.Fatalf("valid cake product: %v", err)
	}
	forward, err := encode(records)
	if err != nil {
		t.Fatalf("encode forward: %v", err)
	}
	reversed := cloneRecords(records)
	slices.Reverse(reversed)
	backward, err := encode(reversed)
	if err != nil {
		t.Fatalf("encode reversed: %v", err)
	}
	if !bytes.Equal(forward, backward) {
		t.Fatal("cake BREG encoding depends on source order")
	}

	mutations := []struct {
		name   string
		mutate func([]Record)
	}{
		{"missing state", func(records []Record) { records[0].StateJSON = []byte(`{}`) }},
		{"aliased key", func(records []Record) {
			records[0].StateJSON = []byte(`{"minecraft:bite_counter":{"type":"int","value":0}}`)
		}},
		{"wrong canonical type", func(records []Record) {
			records[0].StateJSON = []byte(`{"bite_counter":{"type":"byte","value":0}}`)
		}},
		{"out of range", func(records []Record) {
			records[0].StateJSON = []byte(`{"bite_counter":{"type":"int","value":7}}`)
		}},
		{"extra wrapper key", func(records []Record) {
			records[0].StateJSON = []byte(`{"bite_counter":{"type":"int","value":0,"extra":0}}`)
		}},
		{"duplicate outer key", func(records []Record) {
			records[0].StateJSON = []byte(`{"bite_counter":{"type":"int","value":0},"bite_counter":{"type":"int","value":0}}`)
		}},
		{"duplicate wrapper key", func(records []Record) {
			records[0].StateJSON = []byte(`{"bite_counter":{"type":"int","type":"int","value":0}}`)
		}},
		{"extra canonical key", func(records []Record) {
			records[0].StateJSON = []byte(`{"bite_counter":{"type":"int","value":0},"extra":{"type":"int","value":0}}`)
		}},
		{"model-state disagreement", func(records []Record) { records[0].ModelState.Set(ModelStateGrowth, 1) }},
		{"extra model-state field", func(records []Record) { records[0].ModelState.Set(ModelStateOrientation, 0) }},
		{"wrong role", func(records []Record) { records[0].ContributorRole = ContributorLiquidAdditional }},
		{"wrong family", func(records []Record) { records[0].ModelFamily = ModelFamilyCrop }},
		{"wrong ID", func(records []Record) { records[0].SequentialID++ }},
		{"flags", func(records []Record) { records[0].Flags = flagCubeGeometry }},
		{"face coverage", func(records []Record) { records[0].FaceCoverage = 1 }},
		{"shape ID", func(records []Record) { records[0].CollisionSeed.ShapeID++ }},
		{"confidence", func(records []Record) { records[0].CollisionSeed.Confidence = CollisionConfidenceReviewedVisibleBounds }},
		{"collision bounds", func(records []Record) { records[0].CollisionSeed.Boxes[0].MinX++ }},
		{"duplicate selector", func(records []Record) {
			records[1].StateJSON = append([]byte(nil), records[0].StateJSON...)
			records[1].ModelState = records[0].ModelState
		}},
	}
	for _, mutation := range mutations {
		t.Run(mutation.name, func(t *testing.T) {
			broken := cloneRecords(records)
			mutation.mutate(broken)
			if err := validateSelectorCardinality(broken); err == nil {
				t.Fatal("invalid cake product was accepted")
			}
		})
	}
	if err := validateSelectorCardinality(records[:6]); err == nil {
		t.Fatal("incomplete cake product was accepted")
	}
}

func TestFarmlandClassificationRequiresExactTypedMoisture(t *testing.T) {
	const wantMask = uint8(1 << (ModelStateGrowth - 1))
	for amount := int32(0); amount < 8; amount++ {
		record, err := classifyRecord(sourceState("minecraft:farmland", intState("moisturized_amount", amount)))
		if err != nil {
			t.Fatalf("classify amount %d: %v", amount, err)
		}
		if record.ModelFamily != ModelFamilyCuboid || record.ContributorRole != ContributorPrimary {
			t.Fatalf("amount %d family/role=%v/%v", amount, record.ModelFamily, record.ContributorRole)
		}
		if record.ModelState.Mask != wantMask {
			t.Fatalf("amount %d model-state mask=%#x, want %#x", amount, record.ModelState.Mask, wantMask)
		}
		if got, ok := record.ModelState.Get(ModelStateGrowth); !ok || got != uint32(amount) {
			t.Fatalf("amount %d growth=%d/%v", amount, got, ok)
		}
	}

	invalid := []SourceState{
		sourceState("minecraft:farmland"),
		sourceState("minecraft:farmland", intState("moisturized_amount", -1)),
		sourceState("minecraft:farmland", intState("moisturized_amount", 8)),
		sourceState("minecraft:farmland", byteState("moisturized_amount", 1)),
		sourceState("minecraft:farmland", StateProperty{Name: "moisturized_amount", Value: TypedScalar{Kind: ScalarString, String: "1"}}),
		sourceState("minecraft:farmland", intState("minecraft:moisturized_amount", 1)),
		sourceState("minecraft:farmland", intState("moisture", 1)),
		sourceState("minecraft:farmland", intState("moisturized_amount", 1), intState("extra", 0)),
		sourceState("minecraft:farmland", intState("moisturized_amount", 1), intState("moisturized_amount", 1)),
	}
	for index, state := range invalid {
		if _, err := classifyRecord(state); err == nil {
			t.Errorf("invalid farmland selector fixture %d was accepted", index)
		}
	}

	unrelated, err := classifyRecord(sourceState("minecraft:dirt_with_roots", intState("moisturized_amount", 1)))
	if err != nil {
		t.Fatalf("classify unrelated block: %v", err)
	}
	if unrelated.ModelFamily == ModelFamilyCuboid {
		t.Fatal("non-exact farmland name entered farmland cuboid family")
	}
}

func exactFarmlandRecords(t *testing.T) []Record {
	t.Helper()
	records := make([]Record, 0, 8)
	for amount := int32(0); amount < 8; amount++ {
		record, err := classifyRecord(sourceState("minecraft:farmland", intState("moisturized_amount", amount)))
		if err != nil {
			t.Fatalf("classify amount %d: %v", amount, err)
		}
		record.SequentialID = 6122 + uint32(amount)
		record.NetworkHash = 360_492_383 + uint32(amount)*61_474_823
		record.Flags = 0
		record.FaceCoverage = 0
		record.CollisionSeed = CollisionSeed{
			ShapeID:    43,
			Confidence: CollisionConfidenceCollisionOnly,
			Boxes: []CollisionBox{{
				MinX: 0, MaxX: 100_000_000,
				MinY: 0, MaxY: 93_750_000,
				MinZ: 0, MaxZ: 100_000_000,
			}},
		}
		record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
		records = append(records, record)
	}
	return records
}

func TestFarmlandProductRequiresExactIdsStateProjectionAndCollision(t *testing.T) {
	records := exactFarmlandRecords(t)
	if err := validateSelectorCardinality(records); err != nil {
		t.Fatalf("valid farmland product: %v", err)
	}
	forward, err := encode(records)
	if err != nil {
		t.Fatalf("encode forward: %v", err)
	}
	reversed := cloneRecords(records)
	slices.Reverse(reversed)
	backward, err := encode(reversed)
	if err != nil {
		t.Fatalf("encode reversed: %v", err)
	}
	if !bytes.Equal(forward, backward) {
		t.Fatal("farmland BREG encoding depends on source order")
	}

	mutations := []struct {
		name   string
		mutate func([]Record)
	}{
		{"missing state", func(records []Record) { records[0].StateJSON = []byte(`{}`) }},
		{"aliased key", func(records []Record) {
			records[0].StateJSON = []byte(`{"minecraft:moisturized_amount":{"type":"int","value":0}}`)
		}},
		{"wrong canonical type", func(records []Record) {
			records[0].StateJSON = []byte(`{"moisturized_amount":{"type":"byte","value":0}}`)
		}},
		{"out of range", func(records []Record) {
			records[0].StateJSON = []byte(`{"moisturized_amount":{"type":"int","value":8}}`)
		}},
		{"extra wrapper key", func(records []Record) {
			records[0].StateJSON = []byte(`{"moisturized_amount":{"type":"int","value":0,"extra":0}}`)
		}},
		{"duplicate outer key", func(records []Record) {
			records[0].StateJSON = []byte(`{"moisturized_amount":{"type":"int","value":0},"moisturized_amount":{"type":"int","value":0}}`)
		}},
		{"duplicate wrapper key", func(records []Record) {
			records[0].StateJSON = []byte(`{"moisturized_amount":{"type":"int","type":"int","value":0}}`)
		}},
		{"extra canonical key", func(records []Record) {
			records[0].StateJSON = []byte(`{"moisturized_amount":{"type":"int","value":0},"extra":{"type":"int","value":0}}`)
		}},
		{"model-state disagreement", func(records []Record) { records[0].ModelState.Set(ModelStateGrowth, 1) }},
		{"extra model-state field", func(records []Record) { records[0].ModelState.Set(ModelStateOrientation, 0) }},
		{"wrong role", func(records []Record) { records[0].ContributorRole = ContributorLiquidAdditional }},
		{"wrong family", func(records []Record) { records[0].ModelFamily = ModelFamilyCrop }},
		{"wrong ID", func(records []Record) { records[0].SequentialID++ }},
		{"flags", func(records []Record) { records[0].Flags = flagCubeGeometry }},
		{"face coverage", func(records []Record) { records[0].FaceCoverage = 1 }},
		{"shape ID", func(records []Record) { records[0].CollisionSeed.ShapeID++ }},
		{"confidence", func(records []Record) { records[0].CollisionSeed.Confidence = CollisionConfidenceReviewedVisibleBounds }},
		{"collision bounds", func(records []Record) { records[0].CollisionSeed.Boxes[0].MaxY++ }},
		{"duplicate selector", func(records []Record) {
			records[1].StateJSON = append([]byte(nil), records[0].StateJSON...)
			records[1].ModelState = records[0].ModelState
		}},
	}
	for _, mutation := range mutations {
		t.Run(mutation.name, func(t *testing.T) {
			broken := cloneRecords(records)
			mutation.mutate(broken)
			if err := validateSelectorCardinality(broken); err == nil {
				t.Fatal("invalid farmland product was accepted")
			}
		})
	}
	if err := validateSelectorCardinality(records[:7]); err == nil {
		t.Fatal("incomplete farmland product was accepted")
	}
}

func exactResinClumpRecords(t *testing.T) []Record {
	t.Helper()
	records := make([]Record, 0, 64)
	for mask := int32(0); mask < 64; mask++ {
		record, err := classifyRecord(sourceState(
			"minecraft:resin_clump",
			intState("multi_face_direction_bits", mask),
		))
		if err != nil {
			t.Fatalf("classify mask %d: %v", mask, err)
		}
		record.SequentialID = 2930 + uint32(mask)
		record.NetworkHash = 80_000 + uint32(mask)
		record.Flags = 0
		record.FaceCoverage = 0
		record.CollisionSeed = CollisionSeed{
			ShapeID:    0,
			Confidence: CollisionConfidenceCollisionOnly,
		}
		records = append(records, record)
	}
	return records
}

func TestResinClumpProductRequiresExactIdsStateAndEmptyCollision(t *testing.T) {
	records := exactResinClumpRecords(t)
	if err := validateSelectorCardinality(records); err != nil {
		t.Fatalf("valid resin-clump product: %v", err)
	}
	forward, err := encode(records)
	if err != nil {
		t.Fatalf("encode forward: %v", err)
	}
	reversed := append([]Record(nil), records...)
	slices.Reverse(reversed)
	backward, err := encode(reversed)
	if err != nil {
		t.Fatalf("encode reversed: %v", err)
	}
	if !bytes.Equal(forward, backward) {
		t.Fatal("resin-clump BREG encoding depends on source order")
	}

	mutations := []struct {
		name   string
		mutate func([]Record)
	}{
		{"missing state", func(records []Record) { records[0].StateJSON = []byte(`{}`) }},
		{"aliased key", func(records []Record) {
			records[0].StateJSON = []byte(`{"minecraft:multi_face_direction_bits":{"type":"int","value":0}}`)
		}},
		{"wrong canonical type", func(records []Record) {
			records[0].StateJSON = []byte(`{"multi_face_direction_bits":{"type":"byte","value":0}}`)
		}},
		{"extra canonical key", func(records []Record) {
			records[0].StateJSON = []byte(`{"extra":{"type":"int","value":0},"multi_face_direction_bits":{"type":"int","value":0}}`)
		}},
		{"model-state disagreement", func(records []Record) {
			records[0].ModelState.Set(ModelStateConnections, 1)
		}},
		{"extra model-state field", func(records []Record) {
			records[0].ModelState.Set(ModelStateOrientation, 0)
		}},
		{"wrong role", func(records []Record) { records[0].ContributorRole = ContributorLiquidAdditional }},
		{"wrong family", func(records []Record) { records[0].ModelFamily = ModelFamilyGlowLichen }},
		{"wrong ID", func(records []Record) { records[0].SequentialID++ }},
		{"flags", func(records []Record) { records[0].Flags = flagCubeGeometry }},
		{"face coverage", func(records []Record) { records[0].FaceCoverage = 1 }},
		{"shape ID", func(records []Record) { records[0].CollisionSeed.ShapeID = 1 }},
		{"confidence", func(records []Record) { records[0].CollisionSeed.Confidence = CollisionConfidenceReviewedVisibleBounds }},
		{"collision box", func(records []Record) {
			records[0].CollisionSeed.Boxes = []CollisionBox{{MaxX: 1, MaxY: 1, MaxZ: 1}}
		}},
	}
	for _, mutation := range mutations {
		t.Run(mutation.name, func(t *testing.T) {
			broken := append([]Record(nil), records...)
			mutation.mutate(broken)
			if err := validateSelectorCardinality(broken); err == nil {
				t.Fatal("invalid resin-clump product was accepted")
			}
		})
	}

	if err := validateSelectorCardinality(records[:63]); err == nil {
		t.Fatal("incomplete resin-clump product was accepted")
	}
	duplicate := append([]Record(nil), records...)
	duplicate[63] = duplicate[62]
	duplicate[63].SequentialID = 2993
	duplicate[63].NetworkHash++
	if err := validateSelectorCardinality(duplicate); err == nil {
		t.Fatal("duplicate resin-clump selector was accepted")
	}
}

func exactSelectorAliasCubeRecords(t *testing.T) []Record {
	t.Helper()
	unit := CollisionSeed{
		ShapeID:    1,
		Confidence: CollisionConfidenceCollisionOnly,
		Boxes:      []CollisionBox{{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}},
	}
	records := make([]Record, 0, 38)
	appendAxisProduct := func(name string, base uint32, deprecated bool) {
		t.Helper()
		for axisIndex, axis := range []string{"y", "x", "z"} {
			count := 1
			if deprecated {
				count = 4
			}
			for value := 0; value < count; value++ {
				properties := []StateProperty{stringState("pillar_axis", axis)}
				if deprecated {
					properties = []StateProperty{intState("deprecated", int32(value)), stringState("pillar_axis", axis)}
				}
				record, err := classifyRecord(sourceState(name, properties...))
				if err != nil {
					t.Fatalf("classify %s/%s/%d: %v", name, axis, value, err)
				}
				record.SequentialID = base + uint32(axisIndex*count+value)
				record.NetworkHash = 90_000 + record.SequentialID
				record.CollisionSeed = unit
				if (deprecated && value == 0) || (!deprecated && axisIndex == 0) {
					record.Flags = flagCubeGeometry | flagOccludesFullFace
				}
				finalizeGeometryFacts(&record)
				records = append(records, record)
			}
		}
	}
	appendAxisProduct("minecraft:hay_block", 2907, true)
	appendAxisProduct("minecraft:bone_block", 6465, true)
	appendAxisProduct("minecraft:quartz_block", 5442, false)
	appendAxisProduct("minecraft:smooth_quartz", 7081, false)
	appendAxisProduct("minecraft:chiseled_quartz_block", 14685, false)
	appendAxisProduct("minecraft:purpur_block", 15344, false)
	for exploded := byte(0); exploded < 2; exploded++ {
		record, err := classifyRecord(sourceState("minecraft:tnt", byteState("explode_bit", exploded)))
		if err != nil {
			t.Fatalf("classify TNT %d: %v", exploded, err)
		}
		record.SequentialID = 13112 + uint32(exploded)
		record.NetworkHash = 90_000 + record.SequentialID
		record.CollisionSeed = unit
		if exploded == 0 {
			record.Flags = flagCubeGeometry | flagOccludesFullFace
		}
		finalizeGeometryFacts(&record)
		records = append(records, record)
	}
	return records
}

func cloneRecords(records []Record) []Record {
	cloned := append([]Record(nil), records...)
	for index := range cloned {
		cloned[index].StateJSON = append([]byte(nil), records[index].StateJSON...)
		cloned[index].CollisionSeed.Boxes = append([]CollisionBox(nil), records[index].CollisionSeed.Boxes...)
	}
	return cloned
}

func TestReviewedSelectorAliasCubePromotionIsExactAtomicAndDeterministic(t *testing.T) {
	records := exactSelectorAliasCubeRecords(t)
	before := cloneRecords(records)
	if err := promoteReviewedSelectorAliasCubes(records); err != nil {
		t.Fatalf("promote exact products: %v", err)
	}
	wantChanged := map[uint32]bool{
		2908: true, 2909: true, 2910: true, 2912: true, 2913: true, 2914: true, 2916: true, 2917: true, 2918: true,
		6466: true, 6467: true, 6468: true, 6470: true, 6471: true, 6472: true, 6474: true, 6475: true, 6476: true,
		5443: true, 5444: true, 7082: true, 7083: true, 14686: true, 14687: true, 15345: true, 15346: true, 13113: true,
	}
	changed := 0
	for index, record := range records {
		if record.Flags != flagCubeGeometry|flagOccludesFullFace || record.FaceCoverage != 0x3f {
			t.Fatalf("state %d solid facts=%#x/%#x", record.SequentialID, record.Flags, record.FaceCoverage)
		}
		wasChanged := before[index].Flags != record.Flags || before[index].FaceCoverage != record.FaceCoverage
		if wasChanged != wantChanged[record.SequentialID] {
			t.Fatalf("state %d changed=%v, want %v", record.SequentialID, wasChanged, wantChanged[record.SequentialID])
		}
		if wasChanged {
			changed++
		}
	}
	if changed != 27 {
		t.Fatalf("promoted %d states, want 27", changed)
	}
	forward, err := encode(records)
	if err != nil {
		t.Fatalf("encode forward: %v", err)
	}
	reversed := cloneRecords(before)
	slices.Reverse(reversed)
	if err := promoteReviewedSelectorAliasCubes(reversed); err != nil {
		t.Fatalf("promote reversed: %v", err)
	}
	backward, err := encode(reversed)
	if err != nil {
		t.Fatalf("encode reversed: %v", err)
	}
	if !bytes.Equal(forward, backward) {
		t.Fatal("selector-alias cube BREG encoding depends on source order")
	}
}

func TestReviewedSelectorAliasCubePromotionRejectsMalformedProductsWithoutMutation(t *testing.T) {
	valid := exactSelectorAliasCubeRecords(t)
	mutations := []struct {
		name   string
		mutate func([]Record)
	}{
		{"missing state", func(records []Record) { records[0].Name = "minecraft:unrelated" }},
		{"entire product absent", func(records []Record) {
			for index := range records {
				if records[index].Name == "minecraft:tnt" {
					records[index].Name = "minecraft:unrelated_tnt"
				}
			}
		}},
		{"wrong ID", func(records []Record) { records[0].SequentialID++ }},
		{"wrong canonical type", func(records []Record) {
			records[0].StateJSON = []byte(`{"deprecated":{"type":"byte","value":0},"pillar_axis":{"type":"string","value":"y"}}`)
		}},
		{"unknown axis", func(records []Record) {
			records[0].StateJSON = []byte(`{"deprecated":{"type":"int","value":0},"pillar_axis":{"type":"string","value":"q"}}`)
		}},
		{"extra canonical key", func(records []Record) {
			records[0].StateJSON = []byte(`{"deprecated":{"type":"int","value":0},"extra":{"type":"int","value":0},"pillar_axis":{"type":"string","value":"y"}}`)
		}},
		{"model-state disagreement", func(records []Record) { records[0].ModelState.Set(ModelStateOrientation, 2) }},
		{"extra model-state field", func(records []Record) { records[0].ModelState.Set(ModelStateGrowth, 0) }},
		{"wrong role", func(records []Record) { records[0].ContributorRole = ContributorLiquidAdditional }},
		{"wrong family", func(records []Record) { records[0].ModelFamily = ModelFamilyUnknown }},
		{"unexpected source flags", func(records []Record) { records[1].Flags = flagCubeGeometry }},
		{"shape ID", func(records []Record) { records[0].CollisionSeed.ShapeID = 2 }},
		{"confidence", func(records []Record) { records[0].CollisionSeed.Confidence = CollisionConfidenceReviewedVisibleBounds }},
		{"unit bounds", func(records []Record) { records[0].CollisionSeed.Boxes[0].MaxY-- }},
		{"duplicate selector", func(records []Record) {
			records[1].StateJSON = append([]byte(nil), records[0].StateJSON...)
			records[1].ModelState = records[0].ModelState
		}},
	}
	for _, mutation := range mutations {
		t.Run(mutation.name, func(t *testing.T) {
			broken := cloneRecords(valid)
			mutation.mutate(broken)
			before := cloneRecords(broken)
			if err := promoteReviewedSelectorAliasCubes(broken); err == nil {
				t.Fatal("malformed selector-alias product was accepted")
			}
			if !reflect.DeepEqual(broken, before) {
				t.Fatal("failed promotion mutated records")
			}
		})
	}
}

func TestReviewedSelectorAliasCubePromotionIgnoresUnreviewedCubeLikeRecords(t *testing.T) {
	record, err := classifyRecord(sourceState("minecraft:white_glazed_terracotta", intState("facing_direction", 2)))
	if err != nil {
		t.Fatal(err)
	}
	record.CollisionSeed = CollisionSeed{ShapeID: 1, Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}}}
	finalizeGeometryFacts(&record)
	records := []Record{record}
	before := cloneRecords(records)
	if err := promoteReviewedSelectorAliasCubes(records); err != nil {
		t.Fatalf("unrelated record: %v", err)
	}
	if !reflect.DeepEqual(records, before) {
		t.Fatal("unreviewed cube-like record was promoted")
	}
}

func TestMultifaceFamiliesPreserveEveryProtocol1001DirectionMaskSeparately(t *testing.T) {
	for _, fixture := range []struct {
		name       string
		wantFamily ModelFamily
	}{
		{"minecraft:glow_lichen", ModelFamily(33)},
		{"minecraft:sculk_vein", ModelFamily(34)},
	} {
		for mask := uint32(0); mask < 64; mask++ {
			record, err := classifyRecord(sourceState(
				fixture.name,
				intState("multi_face_direction_bits", int32(mask)),
			))
			if err != nil {
				t.Fatalf("%s mask %d: classify: %v", fixture.name, mask, err)
			}
			if record.ModelFamily != fixture.wantFamily {
				t.Errorf("%s mask %d: family=%v, want %v", fixture.name, mask, record.ModelFamily, fixture.wantFamily)
			}
			connections, ok := record.ModelState.Get(ModelStateConnections)
			if !ok || connections != mask {
				t.Errorf("%s mask %d: connections=(%d, %t)", fixture.name, mask, connections, ok)
			}
			if record.Flags&(flagCubeGeometry|flagOccludesFullFace) != 0 || record.FaceCoverage != 0 {
				t.Errorf("%s mask %d: multiface acquired full-block geometry/occlusion: flags=%#x coverage=%#x", fixture.name, mask, record.Flags, record.FaceCoverage)
			}
		}
	}
}

func TestStairSelectorUsesCanonicalLogicalFacingAndUpsideDown(t *testing.T) {
	wantFacing := map[int32]uint32{0: 0, 1: 1, 2: 2, 3: 3} // south, west, north, east
	for raw, want := range wantFacing {
		for upside := byte(0); upside < 2; upside++ {
			record, err := classifyRecord(sourceState(
				"minecraft:oak_stairs",
				intState("weirdo_direction", raw),
				byteState("upside_down_bit", upside),
			))
			if err != nil {
				t.Fatalf("classify direction=%d upside=%d: %v", raw, upside, err)
			}
			if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != want {
				t.Fatalf("direction=%d orientation=%d/%v, want %d/true", raw, got, ok, want)
			}
			if got, ok := record.ModelState.Get(ModelStateHalf); !ok || got != uint32(upside) {
				t.Fatalf("direction=%d upside=%d half=%d/%v", raw, upside, got, ok)
			}
			if record.FaceCoverage != 0 || record.Flags&(flagCubeGeometry|flagOccludesFullFace) != 0 {
				t.Fatalf("direction=%d upside=%d stair was promoted to full coverage: %#x/%#x", raw, upside, record.FaceCoverage, record.Flags)
			}
		}
	}
}

func TestFlowerBedClassificationPreservesGrowthAndOrientation(t *testing.T) {
	for _, name := range []string{"minecraft:wildflowers", "minecraft:pink_petals"} {
		state := sourceState(name,
			intState("growth", 2),
			StateProperty{Name: "minecraft:cardinal_direction", Value: TypedScalar{Kind: ScalarString, String: "east"}},
		)
		record, err := classifyRecord(state)
		if err != nil {
			t.Fatalf("classify %s: %v", name, err)
		}
		if record.ModelFamily != ModelFamilyFlowerBed {
			t.Fatalf("%s family=%v, want FlowerBed", name, record.ModelFamily)
		}
		if got, ok := record.ModelState.Get(ModelStateGrowth); !ok || got != 2 {
			t.Fatalf("%s growth=%d/%v, want 2/true", name, got, ok)
		}
		if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != 3 {
			t.Fatalf("%s orientation=%d/%v, want east (3/true)", name, got, ok)
		}
	}
}

func TestFlowerBedCanonicalStateCoverage(t *testing.T) {
	directions := []string{"south", "west", "north", "east"}
	for _, name := range []string{"minecraft:wildflowers", "minecraft:pink_petals"} {
		records := make([]Record, 0, 32)
		states := make(map[string]struct{}, 32)
		selectors := make(map[[2]uint32]struct{}, 32)
		for growth := int32(0); growth < 8; growth++ {
			for _, direction := range directions {
				record, err := classifyRecord(sourceState(name,
					intState("growth", growth),
					StateProperty{Name: "minecraft:cardinal_direction", Value: TypedScalar{Kind: ScalarString, String: direction}},
				))
				if err != nil {
					t.Fatalf("classify %s growth=%d direction=%s: %v", name, growth, direction, err)
				}
				if record.ModelFamily == ModelFamilyCross || record.ModelFamily == ModelFamilyUnknown {
					t.Fatalf("%s growth=%d direction=%s family=%v, want dedicated family", name, growth, direction, record.ModelFamily)
				}
				if record.ModelFamily != ModelFamilyFlowerBed {
					t.Fatalf("%s growth=%d direction=%s family=%v, want FlowerBed", name, growth, direction, record.ModelFamily)
				}
				gotGrowth, hasGrowth := record.ModelState.Get(ModelStateGrowth)
				orientation, hasOrientation := record.ModelState.Get(ModelStateOrientation)
				if !hasGrowth || !hasOrientation {
					t.Fatalf("%s growth=%d direction=%s selector mask=%#x", name, growth, direction, record.ModelState.Mask)
				}
				records = append(records, record)
				states[string(record.StateJSON)] = struct{}{}
				selectors[[2]uint32{gotGrowth, orientation}] = struct{}{}
			}
		}
		if len(records) != 32 || len(states) != 32 || len(selectors) != 32 {
			t.Fatalf("%s records/states/selectors=%d/%d/%d, want 32/32/32", name, len(records), len(states), len(selectors))
		}
	}
}

func TestEncodeBREG1003Canonical(t *testing.T) {
	record, err := classifyRecord(sourceState("minecraft:stone"))
	if err != nil {
		t.Fatalf("classify record: %v", err)
	}
	record.SequentialID = 0
	record.NetworkHash = 0x11223344
	record.Flags = flagCubeGeometry | flagOccludesFullFace
	record.CollisionSeed = CollisionSeed{
		ShapeID:    7,
		Confidence: CollisionConfidenceCollisionOnly,
		Boxes:      []CollisionBox{{MinX: 0, MinY: 0, MinZ: 0, MaxX: 4096, MaxY: 4096, MaxZ: 4096}},
	}
	record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine | ProvenanceValentine

	metadata := RegistryMetadata{
		Protocol:           1001,
		CanonicalNames:     1,
		CanonicalStates:    1,
		ValentineNames:     1,
		ValentineStates:    1,
		ValentineGapNames:  0,
		ValentineGapStates: 0,
	}
	first, err := encodeWithMetadata(metadata, []Record{record})
	if err != nil {
		t.Fatalf("encode: %v", err)
	}
	second, err := encodeWithMetadata(metadata, []Record{record})
	if err != nil {
		t.Fatalf("encode repeat: %v", err)
	}
	if !bytes.Equal(first, second) {
		t.Fatal("BREG1003 encoding is not deterministic")
	}
	if got := string(first[:8]); got != "BREG1003" {
		t.Fatalf("magic = %q", got)
	}
	if got := binary.LittleEndian.Uint32(first[8:12]); got != 1001 {
		t.Fatalf("protocol = %d", got)
	}
	record.CollisionSeed.Boxes[0].MaxX = collisionLocalHaloMax + 1
	if _, err := encodeWithMetadata(metadata, []Record{record}); err == nil || !strings.Contains(err.Error(), "one-block query halo") {
		t.Fatalf("out-of-halo BREG error = %v", err)
	}
}

func TestEncodeBREG1003AcceptsAquaticFamily(t *testing.T) {
	record := testRecord(0, 1)
	record.ModelFamily = ModelFamilyAquatic
	record.Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
	if _, err := encode([]Record{record}); err != nil {
		t.Fatalf("encode aquatic family: %v", err)
	}
}

func TestEncodeBREG1003RejectsValentineProvenanceMetadataMismatch(t *testing.T) {
	records := []Record{testRecord(0, 1), testRecord(1, 2)}
	records[0].Name = "minecraft:first"
	records[1].Name = "minecraft:second"
	for i := range records {
		records[i].Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine | ProvenanceValentine
	}
	metadata := RegistryMetadata{Protocol: 1001, CanonicalNames: 2, CanonicalStates: 2, ValentineNames: 1, ValentineStates: 2, ValentineGapNames: 1}
	if _, err := encodeWithMetadata(metadata, records); err == nil || !strings.Contains(err.Error(), "Valentine provenance name count") {
		t.Fatalf("name mismatch error = %v", err)
	}
	metadata.ValentineNames = 2
	metadata.ValentineGapNames = 0
	metadata.ValentineStates = 1
	metadata.ValentineGapStates = 1
	if _, err := encodeWithMetadata(metadata, records); err == nil || !strings.Contains(err.Error(), "Valentine provenance state count") {
		t.Fatalf("state mismatch error = %v", err)
	}
}

func TestDecodeNBTStatesStopsAtExactBufferEnd(t *testing.T) {
	first, err := nbt.Marshal(nbtState{Name: "minecraft:air", Properties: map[string]any{}, Version: 1})
	if err != nil {
		t.Fatalf("marshal first NBT state: %v", err)
	}
	second, err := nbt.Marshal(nbtState{Name: "minecraft:test", Properties: map[string]any{"open": uint8(1)}, Version: 1})
	if err != nil {
		t.Fatalf("marshal second NBT state: %v", err)
	}
	states, err := decodeNBTStates(append(first, second...))
	if err != nil {
		t.Fatalf("decode exact NBT stream: %v", err)
	}
	if len(states) != 2 || states[1].Name != "minecraft:test" {
		t.Fatalf("decoded states = %#v", states)
	}
}

func TestCollisionSeedPreservesPinnedDecimalCoordinates(t *testing.T) {
	seed, err := collisionSeed(35, map[string][][]float64{
		"35": {{0.025, -0.0625, 0.1, 0.95000005, 1.0, 0.9}},
	})
	if err != nil {
		t.Fatalf("collision seed: %v", err)
	}
	want := CollisionBox{MinX: 2_500_000, MinY: -6_250_000, MinZ: 10_000_000, MaxX: 95_000_005, MaxY: 100_000_000, MaxZ: 90_000_000}
	if len(seed.Boxes) != 1 || seed.Boxes[0] != want {
		t.Fatalf("collision boxes = %#v, want %#v", seed.Boxes, want)
	}
}

func TestCollisionSeedRejectsBoxesOutsideOneBlockQueryHalo(t *testing.T) {
	for name, box := range map[string][]float64{
		"x below local minimum": {-1.00000001, 0, 0, 1, 1, 1},
		"y below local minimum": {0, -1.00000001, 0, 1, 1, 1},
		"z below local minimum": {0, 0, -1.00000001, 1, 1, 1},
		"x above local maximum": {0, 0, 0, 2.00000001, 1, 1},
		"y above local maximum": {0, 0, 0, 1, 2.00000001, 1},
		"z above local maximum": {0, 0, 0, 1, 1, 2.00000001},
	} {
		t.Run(name, func(t *testing.T) {
			_, err := collisionSeed(1, map[string][][]float64{"1": {box}})
			if err == nil || !strings.Contains(err.Error(), "one-block query halo") {
				t.Fatalf("collisionSeed error = %v", err)
			}
		})
	}
}

func TestCollisionShapeVariantOrdering(t *testing.T) {
	ids := []uint16{8, 3, 5}
	for occurrence, want := range ids {
		got, err := collisionShapeID(ids, occurrence, len(ids))
		if err != nil || got != want {
			t.Fatalf("variant %d = %d, %v; want %d", occurrence, got, err, want)
		}
	}
	if got, err := collisionShapeID([]uint16{7}, 2, 4); err != nil || got != 7 {
		t.Fatalf("shared shape = %d, %v", got, err)
	}
	if _, err := collisionShapeID([]uint16{1, 2}, 0, 3); err == nil || !strings.Contains(err.Error(), "variant cardinality") {
		t.Fatalf("cardinality error = %v", err)
	}
}

func TestValidateRealProvenance(t *testing.T) {
	records := make([]Record, 4)
	for i := range records {
		records[i].Provenance = ProvenancePMMP | ProvenanceDragonfly | ProvenancePrismarine
		if i < 3 {
			records[i].Provenance |= ProvenanceValentine
		}
	}
	audit := ValentineAudit{CanonicalStates: 4, ValentineStates: 3, Joined: 3, Missing: 1}
	if err := validateRealProvenance(records, audit); err != nil {
		t.Fatalf("valid provenance: %v", err)
	}
	bad := append([]Record(nil), records...)
	bad[0].Provenance &^= ProvenancePrismarine
	if err := validateRealProvenance(bad, audit); err == nil || !strings.Contains(err.Error(), "canonical provenance") {
		t.Fatalf("canonical provenance error = %v", err)
	}
	bad = append([]Record(nil), records...)
	bad[3].Provenance |= ProvenanceValentine
	if err := validateRealProvenance(bad, audit); err == nil || !strings.Contains(err.Error(), "Valentine provenance") {
		t.Fatalf("Valentine provenance error = %v", err)
	}
}

func TestNonCubeFamiliesOverrideLegacySolidFlags(t *testing.T) {
	nonUnit := CollisionSeed{ShapeID: 2, Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 100_000_000, MaxY: 90_000_000, MaxZ: 100_000_000}}}
	for _, family := range []ModelFamily{ModelFamilyStatue, ModelFamilyCuboid} {
		record := Record{ModelFamily: family, Flags: flagCubeGeometry | flagOccludesFullFace, CollisionSeed: nonUnit}
		finalizeGeometryFacts(&record)
		if record.Flags != 0 || record.FaceCoverage != 0 || record.ModelFamily != family {
			t.Fatalf("family %v retained false cube facts: %+v", family, record)
		}
	}
	unit := CollisionSeed{ShapeID: 1, Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}}}
	record := Record{ModelFamily: ModelFamilyCube, CollisionSeed: unit}
	finalizeGeometryFacts(&record)
	if record.FaceCoverage != 0x3f {
		t.Fatalf("reviewed split cube coverage = %#x", record.FaceCoverage)
	}
}

func TestChiseledBookshelfPromotesReviewedUnitCollisionToSolidFaceFacts(t *testing.T) {
	unit := CollisionSeed{ShapeID: 1, Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 100_000_000, MaxY: 100_000_000, MaxZ: 100_000_000}}}
	record := Record{ModelFamily: ModelFamilyChiseledBookshelf, CollisionSeed: unit}
	finalizeGeometryFacts(&record)
	if record.Flags != flagCubeGeometry|flagOccludesFullFace || record.FaceCoverage != 0x3f {
		t.Fatalf("unit bookshelf facts = flags %#x coverage %#x", record.Flags, record.FaceCoverage)
	}

	record = Record{ModelFamily: ModelFamilyChiseledBookshelf, Flags: flagCubeGeometry | flagOccludesFullFace, FaceCoverage: 0x3f, CollisionSeed: CollisionSeed{ShapeID: 2, Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 100_000_000, MaxY: 99_999_999, MaxZ: 100_000_000}}}}
	finalizeGeometryFacts(&record)
	if record.Flags != 0 || record.FaceCoverage != 0 {
		t.Fatalf("non-unit bookshelf retained solid facts = flags %#x coverage %#x", record.Flags, record.FaceCoverage)
	}
}

func TestRequiredFamilySelectorFlags(t *testing.T) {
	tests := []struct {
		state       SourceState
		orientation uint32
		flags       uint32
	}{
		{sourceState("minecraft:golden_rail", intState("rail_direction", 5), byteState("rail_data_bit", 1)), 5, modelFlagPowered},
		{sourceState("minecraft:stone_button", intState("facing_direction", 3), byteState("button_pressed_bit", 1)), 3, modelFlagPressed},
		{sourceState("minecraft:lever", StateProperty{Name: "lever_direction", Value: TypedScalar{Kind: ScalarString, String: "up_east_west"}}), 6, 0},
		{sourceState("minecraft:oak_hanging_sign", intState("facing_direction", 3), intState("ground_sign_direction", 7), byteState("attached_bit", 1), byteState("hanging", 1)), 55, modelFlagAttached | modelFlagHanging},
		{sourceState("minecraft:red_bed", intState("direction", 2), byteState("head_piece_bit", 1), byteState("occupied_bit", 1)), 2, modelFlagHead | modelFlagOccupied},
		{sourceState("minecraft:oak_fence_gate", intState("direction", 1), byteState("in_wall_bit", 1)), 1, modelFlagInWall},
		{sourceState("minecraft:oak_door", intState("direction", 3), byteState("upper_block_bit", 1)), 3, modelFlagUpper},
		{sourceState("minecraft:test", StateProperty{Name: "cardinal_direction", Value: TypedScalar{Kind: ScalarString, String: "east"}}), 3, 0},
		{sourceState("minecraft:torch", StateProperty{Name: "torch_facing_direction", Value: TypedScalar{Kind: ScalarString, String: "south"}}), 4, 0},
	}
	for _, test := range tests {
		record, err := classifyRecord(test.state)
		if err != nil {
			t.Fatalf("classify %s: %v", test.state.Name, err)
		}
		if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != test.orientation {
			t.Errorf("%s orientation = %d/%v, want %d", test.state.Name, got, ok, test.orientation)
		}
		if test.flags != 0 {
			if got, ok := record.ModelState.Get(ModelStateFlags); !ok || got != test.flags {
				t.Errorf("%s flags = %#x/%v, want %#x", test.state.Name, got, ok, test.flags)
			}
		}
	}
}

func TestSignSelectorsAreModeDependentAndOrderIndependent(t *testing.T) {
	const signMask = uint8(1 << (ModelStateOrientation - 1))
	const hangingMask = signMask | uint8(1<<(ModelStateFlags-1))

	for rotation := uint32(0); rotation < 16; rotation++ {
		record, err := classifyRecord(sourceState(
			"minecraft:standing_sign",
			intState("ground_sign_direction", int32(rotation)),
		))
		if err != nil {
			t.Fatalf("classify standing rotation %d: %v", rotation, err)
		}
		if record.ModelState.Mask != signMask {
			t.Fatalf("standing rotation %d mask = %#x, want %#x", rotation, record.ModelState.Mask, signMask)
		}
		if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != rotation {
			t.Fatalf("standing rotation %d selector = %d/%v", rotation, got, ok)
		}
	}

	for facing := uint32(0); facing < 6; facing++ {
		record, err := classifyRecord(sourceState(
			"minecraft:wall_sign",
			intState("facing_direction", int32(facing)),
		))
		if err != nil {
			t.Fatalf("classify wall facing %d: %v", facing, err)
		}
		if record.ModelState.Mask != signMask {
			t.Fatalf("wall facing %d mask = %#x, want %#x", facing, record.ModelState.Mask, signMask)
		}
		if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != facing {
			t.Fatalf("wall facing %d selector = %d/%v", facing, got, ok)
		}
	}

	for attached := uint32(0); attached < 2; attached++ {
		for hanging := uint32(0); hanging < 2; hanging++ {
			for rotation := uint32(0); rotation < 16; rotation++ {
				for facing := uint32(0); facing < 6; facing++ {
					properties := []StateProperty{
						byteState("hanging", byte(hanging)),
						intState("ground_sign_direction", int32(rotation)),
						byteState("attached_bit", byte(attached)),
						intState("facing_direction", int32(facing)),
					}
					record, err := classifyRecord(sourceState("minecraft:oak_hanging_sign", properties...))
					if err != nil {
						t.Fatalf("classify hanging selector %d/%d/%d/%d: %v", attached, hanging, rotation, facing, err)
					}
					if record.ModelState.Mask != hangingMask {
						t.Fatalf("hanging selector %d/%d/%d/%d mask = %#x, want %#x", attached, hanging, rotation, facing, record.ModelState.Mask, hangingMask)
					}
					wantOrientation := rotation | (facing << 4)
					if got, ok := record.ModelState.Get(ModelStateOrientation); !ok || got != wantOrientation {
						t.Fatalf("hanging selector %d/%d/%d/%d orientation = %d/%v, want %d", attached, hanging, rotation, facing, got, ok, wantOrientation)
					}
					wantFlags := attached*modelFlagAttached | hanging*modelFlagHanging
					if got, ok := record.ModelState.Get(ModelStateFlags); !ok || got != wantFlags {
						t.Fatalf("hanging selector %d/%d/%d/%d flags = %#x/%v, want %#x", attached, hanging, rotation, facing, got, ok, wantFlags)
					}

					slices.Reverse(properties)
					reversed, err := classifyRecord(sourceState("minecraft:oak_hanging_sign", properties...))
					if err != nil {
						t.Fatalf("classify reversed hanging selector: %v", err)
					}
					if reversed.ModelState != record.ModelState {
						t.Fatalf("property order changed hanging selector: got %+v want %+v", reversed.ModelState, record.ModelState)
					}
				}
			}
		}
	}
}

func TestPressurePlateRedstoneSignalBecomesTypedPressedFlag(t *testing.T) {
	for signal := int32(0); signal <= 15; signal++ {
		state := sourceState(
			"minecraft:wooden_pressure_plate",
			intState("redstone_signal", signal),
		)
		record, err := classifyRecord(state)
		if err != nil {
			t.Fatalf("classify signal %d: %v", signal, err)
		}
		if record.ModelFamily != ModelFamilyPressurePlate {
			t.Fatalf("signal %d family = %v", signal, record.ModelFamily)
		}
		want := uint32(0)
		if signal != 0 {
			want = modelFlagPressed
		}
		if got, ok := record.ModelState.Get(ModelStateFlags); !ok || got != want {
			t.Errorf("signal %d flags = %#x/%v, want %#x/present", signal, got, ok, want)
		}
	}
}

func TestRedstoneSignalSelectorIsPressurePlateScoped(t *testing.T) {
	record, err := classifyRecord(sourceState("minecraft:test", intState("redstone_signal", 15)))
	if err != nil {
		t.Fatalf("classify unrelated redstone state: %v", err)
	}
	if got, ok := record.ModelState.Get(ModelStateFlags); ok {
		t.Fatalf("unrelated redstone signal leaked typed flags %#x", got)
	}
}

func TestPressurePlateRejectsRedstoneSignalOutsideTypedDomain(t *testing.T) {
	for _, signal := range []int32{-1, 16} {
		_, err := classifyRecord(sourceState(
			"minecraft:wooden_pressure_plate",
			intState("redstone_signal", signal),
		))
		if err == nil {
			t.Fatalf("signal %d unexpectedly classified", signal)
		}
	}
}

func TestClassifyRecordCanonicalPropertyOrder(t *testing.T) {
	properties := []StateProperty{
		intState("direction", 1),
		intState("facing_direction", 2),
		byteState("upper_block_bit", 1),
	}
	forward, err := classifyRecord(sourceState("minecraft:test", properties...))
	if err != nil {
		t.Fatalf("classify forward: %v", err)
	}
	slices.Reverse(properties)
	reverse, err := classifyRecord(sourceState("minecraft:test", properties...))
	if err != nil {
		t.Fatalf("classify reverse: %v", err)
	}
	if forward.ModelState != reverse.ModelState || !bytes.Equal(forward.StateJSON, reverse.StateJSON) {
		t.Fatalf("classification depends on property order: %+v vs %+v", forward.ModelState, reverse.ModelState)
	}
}
