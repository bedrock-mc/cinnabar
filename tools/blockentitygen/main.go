package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"reflect"
	"sort"
	"strings"

	_ "github.com/df-mc/dragonfly/server/block"
	"github.com/df-mc/dragonfly/server/world"
)

const (
	sourceSchema             = 1
	rendererSchema           = 1
	artifactSchema           = 1
	reportSchema             = 1
	maxRendererManifestBytes = 1 << 20

	protocolVersion = 1001
	gameVersion     = "1.26.30"

	dragonflyModule   = "github.com/df-mc/dragonfly"
	dragonflyVersion  = "v0.10.15-0.20260709170650-b85c56ffea6b"
	dragonflyRevision = "b85c56ffea6b306798a935f14cc941c76618be52"

	bdsServerVersion    = "1.26.32.2"
	bdsExecutableSize   = 207171400
	bdsExecutableSHA256 = "10c680f00faffecdfb3743c5a8a71d6c73f176d148173ca19a99b0c80e40a83f"

	// Filled from the canonical JSON projection of the exact pinned NBTer
	// registrations. Any registration, ID, source file pin, backing block, or
	// canonical state change must use the deliberate pin-bump workflow.
	dragonflyRegistrationSHA256 = "13a852419214598a6a644557d3cc3a587ce0ca3a61292571c841184fbe937bae"
)

type joinMode uint8

const (
	joinReviewed joinMode = iota
	joinStrictFinal
)

type dragonflyProvenance struct {
	Module             string `json:"module"`
	Version            string `json:"version"`
	Revision           string `json:"revision"`
	RegistrationSHA256 string `json:"registration_sha256"`
}

type bdsProvenance struct {
	ServerVersion    string `json:"server_version"`
	ProtocolVersion  int    `json:"protocol_version"`
	ExecutableBytes  int64  `json:"executable_bytes"`
	ExecutableSHA256 string `json:"executable_sha256"`
}

type backingBlock struct {
	Name   string            `json:"name"`
	States []json.RawMessage `json:"states"`
}

type sourceEntry struct {
	SourceKey        string         `json:"source_key"`
	NBTID            *string        `json:"nbt_id"`
	NBTAliases       []string       `json:"nbt_aliases"`
	SourceFile       string         `json:"source_file"`
	SourceFileSHA256 string         `json:"source_file_sha256"`
	BackingBlocks    []backingBlock `json:"backing_blocks"`
}

type sourceInventory struct {
	Schema           int                 `json:"schema"`
	ProtocolVersion  int                 `json:"protocol_version"`
	GameVersion      string              `json:"game_version"`
	BDSServerVersion string              `json:"bds_server_version"`
	Dragonfly        dragonflyProvenance `json:"dragonfly"`
	BDS              bdsProvenance       `json:"bds"`
	Entries          []sourceEntry       `json:"entries"`
}

type evidencePath struct {
	Supported  bool     `json:"supported"`
	WitnessIDs []string `json:"witness_ids"`
}

type nbtVariant struct {
	VariantID      string   `json:"variant_id"`
	RequiredFields []string `json:"required_fields"`
	WitnessIDs     []string `json:"witness_ids"`
}

type renderWitnesses struct {
	GPU    []string `json:"gpu"`
	NoDraw []string `json:"no_draw"`
}

type rendererEntry struct {
	SourceKey            string          `json:"source_key"`
	NBTID                *string         `json:"nbt_id"`
	NBTAliases           []string        `json:"nbt_aliases"`
	RequiredNBTVariants  []nbtVariant    `json:"required_nbt_variants"`
	ChunkNBT             evidencePath    `json:"chunk_nbt"`
	LiveUpdate           evidencePath    `json:"live_update"`
	RendererClass        string          `json:"renderer_class"`
	RendererStatus       string          `json:"renderer_status"`
	ImplementationSymbol *string         `json:"implementation_symbol"`
	GalleryBuilder       *string         `json:"gallery_builder"`
	Witnesses            renderWitnesses `json:"witnesses"`
}

type rendererManifest struct {
	Schema                      int             `json:"schema"`
	ProtocolVersion             int             `json:"protocol_version"`
	GameVersion                 string          `json:"game_version"`
	DragonflyModule             string          `json:"dragonfly_module"`
	DragonflyVersion            string          `json:"dragonfly_version"`
	DragonflyRevision           string          `json:"dragonfly_revision"`
	DragonflyRegistrationSHA256 string          `json:"dragonfly_registration_sha256"`
	BDSServerVersion            string          `json:"bds_server_version"`
	BDSExecutableSHA256         string          `json:"bds_executable_sha256"`
	Entries                     []rendererEntry `json:"entries"`
	digest                      string
	evidenceDigest              string
	sourceContractDigest        string
	rendererContractDigest      string
}

type entrySourceProvenance struct {
	DragonflyModule             string `json:"dragonfly_module"`
	DragonflyVersion            string `json:"dragonfly_version"`
	DragonflyRevision           string `json:"dragonfly_revision"`
	DragonflyRegistrationSHA256 string `json:"dragonfly_registration_sha256"`
	DragonflySourceFile         string `json:"dragonfly_source_file"`
	DragonflySourceFileSHA256   string `json:"dragonfly_source_file_sha256"`
	BDSServerVersion            string `json:"bds_server_version"`
	BDSExecutableSHA256         string `json:"bds_executable_sha256"`
}

type inventoryEntry struct {
	SourceKey            string                `json:"source_key"`
	NBTID                *string               `json:"nbt_id"`
	NBTAliases           []string              `json:"nbt_aliases"`
	BackingBlocks        []backingBlock        `json:"backing_blocks"`
	Source               entrySourceProvenance `json:"source"`
	RequiredNBTVariants  []nbtVariant          `json:"required_nbt_variants"`
	ChunkNBT             evidencePath          `json:"chunk_nbt"`
	LiveUpdate           evidencePath          `json:"live_update"`
	RendererClass        string                `json:"renderer_class"`
	RendererStatus       string                `json:"renderer_status"`
	ImplementationSymbol *string               `json:"implementation_symbol"`
	GalleryBuilder       *string               `json:"gallery_builder"`
	Witnesses            renderWitnesses       `json:"witnesses"`
}

type blockEntityInventory struct {
	Schema                     int                 `json:"schema"`
	ProtocolVersion            int                 `json:"protocol_version"`
	GameVersion                string              `json:"game_version"`
	Dragonfly                  dragonflyProvenance `json:"dragonfly"`
	BDS                        bdsProvenance       `json:"bds"`
	RendererManifestSHA256     string              `json:"renderer_manifest_sha256"`
	SourceContractSHA256       string              `json:"source_contract_sha256"`
	RendererContractSHA256     string              `json:"renderer_contract_sha256"`
	EvidenceCatalogSHA256      string              `json:"evidence_catalog_sha256"`
	CanonicalBlockStateCounted bool                `json:"canonical_block_state_counted"`
	Entries                    []inventoryEntry    `json:"entries"`
}

type coverageReport struct {
	Schema                      int      `json:"schema"`
	ProtocolVersion             int      `json:"protocol_version"`
	DragonflyRegistrationSHA256 string   `json:"dragonfly_registration_sha256"`
	BDSExecutableSHA256         string   `json:"bds_executable_sha256"`
	RendererManifestSHA256      string   `json:"renderer_manifest_sha256"`
	SourceContractSHA256        string   `json:"source_contract_sha256"`
	RendererContractSHA256      string   `json:"renderer_contract_sha256"`
	EvidenceCatalogSHA256       string   `json:"evidence_catalog_sha256"`
	SourceCount                 int      `json:"source_count"`
	ManifestCount               int      `json:"manifest_count"`
	JoinedCount                 int      `json:"joined_count"`
	ExplicitNBTIDCount          int      `json:"explicit_nbt_id_count"`
	IDLessProducerCount         int      `json:"id_less_producer_count"`
	ImplementedRendererCount    int      `json:"implemented_renderer_count"`
	DeferredRendererCount       int      `json:"deferred_renderer_count"`
	UnsupportedRendererCount    int      `json:"unsupported_renderer_count"`
	ProvenRendererCount         int      `json:"proven_renderer_count"`
	FinalGatePassed             bool     `json:"final_gate_passed"`
	FinalBlockers               []string `json:"final_blockers"`
}

type sourceFilePin struct {
	File   string
	SHA256 string
}

var sourceFilePins = map[string]sourceFilePin{
	"Banner":            {"server/block/banner.go", "2b4bf86772098f20d90e89eeb128aa72c652bce2e9a7256e008d30501e43f189"},
	"Barrel":            {"server/block/barrel.go", "7ca8f5bbc338dff857fa0453a1d5c3336e4e58ad51dd45ae66ef7c011abd4edf"},
	"Beacon":            {"server/block/beacon.go", "7bd35f40857c70421ceace84e84271815e8ca5f856b49a1a32c416f935b804f3"},
	"Bed":               {"server/block/bed.go", "d7b1d4f8e1ce9f34d85ec41712601646e9f3670b771d98668034f5bb1143c070"},
	"BlastFurnace":      {"server/block/blast_furnace.go", "63b3ef85c523bc5a9d2b1a141aea9dbbb61c2abc007daff56dcf7ec3ed2a517d"},
	"BrewingStand":      {"server/block/brewing_stand.go", "d8a9787b921e74f58ad8338b607a98b6fbb53cff1a5e364b451d77f912f857a5"},
	"Campfire":          {"server/block/campfire.go", "a3a65405da1a9774aaf77a90f043a42a2054947b311a2b39d4372ab1b666dcf5"},
	"Chest":             {"server/block/chest.go", "d8188c53c5cb683924e66740ec29248ef5d0e4535ff660dec045ab156de32d07"},
	"CopperGolemStatue": {"server/block/copper_golem_statue.go", "3117769f076371d179aa0b0a38d57e26fc19e1578715a565adab786805528371"},
	"DecoratedPot":      {"server/block/decorated_pot.go", "0c71eb53a868da9b711e6de937a820ae509ce87996af336d7d111e4b687783af"},
	"EnchantTable":      {"server/block/enchanting_table.go", "3a533b9abedd2d9961b531df2773aeec0fece882f8d2832c888b1db0e78668dd"},
	"EnderChest":        {"server/block/ender_chest.go", "b0bb24c036e9ba9106540c422337b741ad972fee88ae5b702a39841f5685bc45"},
	"Furnace":           {"server/block/furnace.go", "c0e630e27380708a48adcfe2d4089ee1646e1444b799de58d97fd89bfbb745ea"},
	"GlowItemFrame":     {"server/block/item_frame.go", "40f2adad61de22fdfcce31e5c308844955a55c818a9399f80f947b7d8120dd9c"},
	"Hopper":            {"server/block/hopper.go", "796dd81ff5f86336c645eaaf98ec1eddb164de41abae458b6505e22e9869977e"},
	"ItemFrame":         {"server/block/item_frame.go", "40f2adad61de22fdfcce31e5c308844955a55c818a9399f80f947b7d8120dd9c"},
	"Jukebox":           {"server/block/jukebox.go", "3099c37944cb662f7a54c23eeb5f3dca88680e7f7571c9b12a6475fa064c6c0c"},
	"Lectern":           {"server/block/lectern.go", "538892bd4b81bb2ec342cbe9f6b4d8446ff9d2009f2e63b9eed84f288a58b472"},
	"Note":              {"server/block/note.go", "3d1f88ab57b1b63418b9c8a727d12a0dbd9077b90b0f3e88f568d06944e0ef7d"},
	"Sign":              {"server/block/sign.go", "53f6782a16f0b0f58364f1561c30f5f3e9350d8b186ca940d9a957aecd3781b2"},
	"Skull":             {"server/block/skull.go", "2227703b3bd3d5b8b6a69f9127d9ef10db0701bc344347559a0457ba7f604a64"},
	"Smoker":            {"server/block/smoker.go", "2382fa2a36f6ef1b6c5d3389bbc1831e2bdb9aae7a1f64f5dd2b01400eb5e4a6"},
}

var requiredNBTVariants = map[string]map[string][]string{
	"Banner": {
		"blank": {"id", "Base", "Type", "Patterns"}, "patterned": {"id", "Base", "Type", "Patterns"}, "illager": {"id", "Base", "Type", "Patterns"},
	},
	"Barrel":       {"empty": {"id", "Items"}, "inventory_named": {"id", "Items", "CustomName"}},
	"Beacon":       {"inactive": {"id", "Levels"}, "powered_effects": {"id", "Levels", "Primary", "Secondary"}},
	"Bed":          {"all_colours": {"id", "color"}},
	"BlastFurnace": {"idle": {"id", "BurnTime", "CookTime", "BurnDuration", "StoredXPInt", "Items"}, "active_recipe": {"id", "BurnTime", "CookTime", "BurnDuration", "StoredXPInt", "Items"}},
	"BrewingStand": {"empty": {"id", "Items", "CookTime", "FuelTotal", "FuelAmount"}, "brewing": {"id", "Items", "CookTime", "FuelTotal", "FuelAmount"}},
	"Campfire":     {"empty": {"id"}, "cooking_four_slots": {"id", "Item1", "ItemTime1", "Item2", "ItemTime2", "Item3", "ItemTime3", "Item4", "ItemTime4"}},
	"Chest": {
		"single_empty": {"id", "Items"}, "single_inventory_named": {"id", "Items", "CustomName"}, "paired": {"id", "Items", "pairx", "pairz"},
	},
	"CopperGolemStatue": {"all_poses": {"id", "Pose"}},
	"DecoratedPot":      {"brick_sides_empty": {"id", "sherds"}, "mixed_sherds_with_item": {"id", "sherds", "item"}},
	"EnchantTable":      {"default": {"id"}},
	"EnderChest":        {"default": {"id"}},
	"Furnace":           {"idle": {"id", "BurnTime", "CookTime", "BurnDuration", "StoredXPInt", "Items"}, "active_recipe": {"id", "BurnTime", "CookTime", "BurnDuration", "StoredXPInt", "Items"}},
	"GlowItemFrame":     {"empty": {"id", "ItemDropChance", "ItemRotation"}, "displayed_item_all_rotations": {"id", "ItemDropChance", "ItemRotation", "Item"}},
	"Hopper":            {"empty": {"id", "Items", "TransferCooldown"}, "inventory_named_cooldown": {"id", "Items", "TransferCooldown", "CustomName"}},
	"ItemFrame":         {"empty": {"id", "ItemDropChance", "ItemRotation"}, "displayed_item_all_rotations": {"id", "ItemDropChance", "ItemRotation", "Item"}},
	"Jukebox":           {"empty": {"id"}, "record": {"id", "RecordItem"}},
	"Lectern":           {"empty": {"id", "hasBook", "page"}, "book_all_pages": {"id", "hasBook", "page", "book", "totalPages"}},
	"Note":              {"all_pitches_unpowered": {"note", "powered"}, "all_pitches_powered": {"note", "powered"}},
	"Sign":              {"plain_front_back": {"id", "IsWaxed", "FrontText", "BackText"}, "coloured_glowing_waxed": {"id", "IsWaxed", "FrontText", "BackText"}},
	"Skull":             {"standing_types_rotations": {"id", "SkullType", "Rotation"}, "wall_types": {"id", "SkullType", "Rotation"}},
	"Smoker":            {"idle": {"id", "BurnTime", "CookTime", "BurnDuration", "StoredXPInt", "Items"}, "active_recipe": {"id", "BurnTime", "CookTime", "BurnDuration", "StoredXPInt", "Items"}},
}

func main() {
	manifestPath := flag.String("renderer-manifest", "assets/block-entity-renderers-v1001.json", "reviewed renderer manifest")
	evidenceCatalogPath := flag.String("evidence-catalog", "docs/evidence/block-entity-render-evidence-v1001.json", "hash-bound renderer evidence catalog")
	outputPath := flag.String("output", "crates/assets/data/block-entities-v1001.json", "generated inventory")
	reportPath := flag.String("report", "docs/block-entity-coverage-v1001-report.json", "deterministic coverage report")
	strictFinal := flag.Bool("strict-final", false, "require implemented renderers and complete variant/GPU or no-draw evidence")
	verifyBDS := flag.String("verify-bds", "", "optional path to the pinned BDS executable")
	flag.Parse()

	if *verifyBDS != "" {
		if err := verifyBDSExecutable(*verifyBDS); err != nil {
			fatal(err)
		}
	}
	source, err := collectPinnedInventory()
	if err != nil {
		fatal(err)
	}
	manifest, err := readRendererManifest(*manifestPath)
	if err != nil {
		fatal(err)
	}
	catalog, err := readEvidenceCatalog(*evidenceCatalogPath)
	if err != nil {
		fatal(err)
	}
	sourceContractDigest, err := sourceContractSHA256(source)
	if err != nil {
		fatal(fmt.Errorf("hash source contract: %w", err))
	}
	rendererContractDigest, err := rendererContractSHA256(manifest)
	if err != nil {
		fatal(fmt.Errorf("hash renderer contract: %w", err))
	}
	manifest, err = joinEvidence(manifest, catalog, evidenceIdentities{
		SourceContractSHA256: sourceContractDigest, RendererContractSHA256: rendererContractDigest,
		Targets: map[string]evidenceTargetIdentity{},
	})
	if err != nil {
		fatal(err)
	}
	mode := joinReviewed
	if *strictFinal {
		mode = joinStrictFinal
	}
	artifact, report, joinErr := joinInventory(source, manifest, mode)
	if joinErr != nil {
		fatal(joinErr)
	}
	inventoryJSON, reportJSON, err := encodeArtifacts(artifact, report)
	if err != nil {
		fatal(err)
	}
	if err := writeAtomic(*outputPath, inventoryJSON); err != nil {
		fatal(err)
	}
	if err := writeAtomic(*reportPath, reportJSON); err != nil {
		fatal(err)
	}
	fmt.Printf("block entities: source=%d joined=%d proven=%d deferred=%d final=%t\n", report.SourceCount, report.JoinedCount, report.ProvenRendererCount, report.DeferredRendererCount, report.FinalGatePassed)
}

func collectPinnedInventory() (sourceInventory, error) {
	moduleRoot, err := resolveDragonflyModuleRoot()
	if err != nil {
		return sourceInventory{}, err
	}
	return collectPinnedInventoryFromModuleRoot(moduleRoot)
}

func collectPinnedInventoryFromModuleRoot(moduleRoot string) (sourceInventory, error) {
	if err := verifyPinnedSourceFiles(moduleRoot); err != nil {
		return sourceInventory{}, err
	}
	return collectPinnedInventoryFromRegistry()
}

func collectPinnedInventoryFromRegistry() (sourceInventory, error) {
	world.DefaultBlockRegistry.Finalize()
	grouped := map[string]map[string]map[string]json.RawMessage{}
	ids := map[string]*string{}
	for _, value := range world.DefaultBlockRegistry.Blocks() {
		typeOf := reflect.TypeOf(value)
		for typeOf.Kind() == reflect.Pointer {
			typeOf = typeOf.Elem()
		}
		// unknownBlock intentionally preserves arbitrary NBT for unimplemented
		// palette states. It is a codec fallback, not a registered block-entity
		// producer, so only concrete server/block implementations are audited.
		if typeOf.PkgPath() != dragonflyModule+"/server/block" {
			continue
		}
		nbter, ok := value.(world.NBTer)
		if !ok {
			continue
		}
		blockName, properties := value.EncodeBlock()
		state, err := json.Marshal(normalizeProperties(properties))
		if err != nil {
			return sourceInventory{}, fmt.Errorf("encode %s state: %w", blockName, err)
		}
		encoded := nbter.EncodeNBT()
		sourceKey := typeOf.Name()
		var nbtID *string
		if rawID, present := encoded["id"]; present {
			id, ok := rawID.(string)
			if !ok || id == "" {
				return sourceInventory{}, fmt.Errorf("%s emitted invalid NBT id %#v", sourceKey, rawID)
			}
			sourceKey = id
			idCopy := id
			nbtID = &idCopy
		}
		pin, ok := sourceFilePins[sourceKey]
		if !ok {
			return sourceInventory{}, fmt.Errorf("unreviewed Dragonfly NBT producer %q", sourceKey)
		}
		if old, present := ids[sourceKey]; present && !equalOptionalString(old, nbtID) {
			return sourceInventory{}, fmt.Errorf("ambiguous NBT identity for %q", sourceKey)
		}
		ids[sourceKey] = nbtID
		if grouped[sourceKey] == nil {
			grouped[sourceKey] = map[string]map[string]json.RawMessage{}
		}
		if grouped[sourceKey][blockName] == nil {
			grouped[sourceKey][blockName] = map[string]json.RawMessage{}
		}
		grouped[sourceKey][blockName][string(state)] = state
		_ = pin
	}
	if len(grouped) != len(sourceFilePins) {
		return sourceInventory{}, fmt.Errorf("Dragonfly NBT producer count %d does not match reviewed %d", len(grouped), len(sourceFilePins))
	}

	keys := make([]string, 0, len(grouped))
	for key := range grouped {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	entries := make([]sourceEntry, 0, len(keys))
	for _, key := range keys {
		pin := sourceFilePins[key]
		blockNames := make([]string, 0, len(grouped[key]))
		for name := range grouped[key] {
			blockNames = append(blockNames, name)
		}
		sort.Strings(blockNames)
		blocks := make([]backingBlock, 0, len(blockNames))
		for _, name := range blockNames {
			stateKeys := make([]string, 0, len(grouped[key][name]))
			for state := range grouped[key][name] {
				stateKeys = append(stateKeys, state)
			}
			sort.Strings(stateKeys)
			states := make([]json.RawMessage, 0, len(stateKeys))
			for _, state := range stateKeys {
				states = append(states, grouped[key][name][state])
			}
			blocks = append(blocks, backingBlock{Name: name, States: states})
		}
		entries = append(entries, sourceEntry{
			SourceKey: key, NBTID: ids[key], NBTAliases: []string{}, SourceFile: pin.File,
			SourceFileSHA256: pin.SHA256, BackingBlocks: blocks,
		})
	}
	rawEntries, err := json.Marshal(entries)
	if err != nil {
		return sourceInventory{}, err
	}
	digest := sha256.Sum256(rawEntries)
	registrationHash := hex.EncodeToString(digest[:])
	if registrationHash != dragonflyRegistrationSHA256 {
		return sourceInventory{}, fmt.Errorf("Dragonfly registration hash drift: got %s, want %s", registrationHash, dragonflyRegistrationSHA256)
	}
	return sourceInventory{
		Schema: sourceSchema, ProtocolVersion: protocolVersion, GameVersion: gameVersion,
		BDSServerVersion: bdsServerVersion,
		Dragonfly:        dragonflyProvenance{Module: dragonflyModule, Version: dragonflyVersion, Revision: dragonflyRevision, RegistrationSHA256: registrationHash},
		BDS:              bdsProvenance{ServerVersion: bdsServerVersion, ProtocolVersion: protocolVersion, ExecutableBytes: bdsExecutableSize, ExecutableSHA256: bdsExecutableSHA256},
		Entries:          entries,
	}, nil
}

func resolveDragonflyModuleRoot() (string, error) {
	command := exec.Command("go", "list", "-m", "-f", "{{.Path}}\n{{.Version}}\n{{.Dir}}", dragonflyModule)
	command.Env = append(os.Environ(), "GOWORK=off")
	raw, err := command.CombinedOutput()
	if err != nil {
		return "", fmt.Errorf("resolve pinned Dragonfly module: %w: %s", err, strings.TrimSpace(string(raw)))
	}
	parts := strings.Split(strings.TrimSpace(string(raw)), "\n")
	if len(parts) != 3 || strings.TrimSpace(parts[0]) != dragonflyModule || strings.TrimSpace(parts[1]) != dragonflyVersion {
		return "", errors.New("resolved Dragonfly module path/version drift")
	}
	root := strings.TrimSpace(parts[2])
	if root == "" {
		return "", errors.New("resolved empty Dragonfly module root")
	}
	return root, nil
}

func verifyPinnedSourceFiles(moduleRoot string) error {
	root, err := filepath.Abs(moduleRoot)
	if err != nil {
		return fmt.Errorf("resolve Dragonfly module root: %w", err)
	}
	keys := make([]string, 0, len(sourceFilePins))
	for key := range sourceFilePins {
		keys = append(keys, key)
	}
	sort.Strings(keys)
	for _, key := range keys {
		pin := sourceFilePins[key]
		path := filepath.Join(root, filepath.FromSlash(pin.File))
		relative, err := filepath.Rel(root, path)
		if err != nil || relative == ".." || strings.HasPrefix(relative, ".."+string(filepath.Separator)) {
			return fmt.Errorf("invalid Dragonfly source path for %s", key)
		}
		digest, err := streamFileSHA256(path)
		if err != nil {
			return fmt.Errorf("hash Dragonfly source for %s: %w", key, err)
		}
		if digest != pin.SHA256 {
			return fmt.Errorf("Dragonfly source hash drift for %s: got %s, want %s", key, digest, pin.SHA256)
		}
	}
	return nil
}

func streamFileSHA256(path string) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	digest := sha256.New()
	if _, err := io.Copy(digest, file); err != nil {
		return "", err
	}
	return hex.EncodeToString(digest.Sum(nil)), nil
}

func normalizeProperties(properties map[string]any) map[string]any {
	if properties == nil {
		return map[string]any{}
	}
	return properties
}

func decodeRendererManifest(raw []byte) (rendererManifest, error) {
	if len(raw) > maxRendererManifestBytes {
		return rendererManifest{}, fmt.Errorf("renderer manifest size %d exceeds %d bytes", len(raw), maxRendererManifestBytes)
	}
	var manifest rendererManifest
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return rendererManifest{}, fmt.Errorf("decode renderer manifest: %w", err)
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return rendererManifest{}, errors.New("renderer manifest has trailing JSON content")
	}
	canonical, err := marshalCanonical(manifest)
	if err != nil {
		return rendererManifest{}, err
	}
	digest := sha256.Sum256(canonical)
	manifest.digest = hex.EncodeToString(digest[:])
	return manifest, nil
}

func readRendererManifest(path string) (rendererManifest, error) {
	file, err := os.Open(path)
	if err != nil {
		return rendererManifest{}, fmt.Errorf("open renderer manifest: %w", err)
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return rendererManifest{}, fmt.Errorf("stat renderer manifest: %w", err)
	}
	if info.Size() > maxRendererManifestBytes {
		return rendererManifest{}, fmt.Errorf("renderer manifest size %d exceeds %d bytes", info.Size(), maxRendererManifestBytes)
	}
	raw, err := io.ReadAll(io.LimitReader(file, maxRendererManifestBytes+1))
	if err != nil {
		return rendererManifest{}, fmt.Errorf("read renderer manifest: %w", err)
	}
	return decodeRendererManifest(raw)
}

func joinInventory(source sourceInventory, manifest rendererManifest, mode joinMode) (blockEntityInventory, coverageReport, error) {
	report := coverageReport{
		Schema: reportSchema, ProtocolVersion: protocolVersion,
		DragonflyRegistrationSHA256: source.Dragonfly.RegistrationSHA256,
		BDSExecutableSHA256:         source.BDS.ExecutableSHA256,
		RendererManifestSHA256:      manifest.digest,
		SourceContractSHA256:        manifest.sourceContractDigest,
		RendererContractSHA256:      manifest.rendererContractDigest,
		EvidenceCatalogSHA256:       manifest.evidenceDigest,
		SourceCount:                 len(source.Entries), ManifestCount: len(manifest.Entries),
		FinalBlockers: []string{},
	}
	if err := validatePins(source, manifest); err != nil {
		return blockEntityInventory{}, report, err
	}
	sourceByKey := make(map[string]sourceEntry, len(source.Entries))
	canonicalIDs := map[string]string{}
	for _, entry := range source.Entries {
		if _, exists := sourceByKey[entry.SourceKey]; exists {
			return blockEntityInventory{}, report, fmt.Errorf("duplicate source key %q", entry.SourceKey)
		}
		sourceByKey[entry.SourceKey] = entry
		if entry.NBTID == nil {
			report.IDLessProducerCount++
		} else {
			report.ExplicitNBTIDCount++
			if owner, exists := canonicalIDs[*entry.NBTID]; exists {
				return blockEntityInventory{}, report, fmt.Errorf("ambiguous canonical NBT id %q for %q and %q", *entry.NBTID, owner, entry.SourceKey)
			}
			canonicalIDs[*entry.NBTID] = entry.SourceKey
		}
	}
	manifestByKey := make(map[string]rendererEntry, len(manifest.Entries))
	for _, entry := range manifest.Entries {
		if _, exists := manifestByKey[entry.SourceKey]; exists {
			return blockEntityInventory{}, report, fmt.Errorf("duplicate renderer source key %q", entry.SourceKey)
		}
		manifestByKey[entry.SourceKey] = entry
		if _, exists := sourceByKey[entry.SourceKey]; !exists {
			return blockEntityInventory{}, report, fmt.Errorf("manifest-only source key %q", entry.SourceKey)
		}
	}
	for key := range sourceByKey {
		if _, exists := manifestByKey[key]; !exists {
			return blockEntityInventory{}, report, fmt.Errorf("missing source key %q from renderer manifest", key)
		}
	}
	aliasOwners := canonicalIDs
	for _, entry := range manifest.Entries {
		for _, alias := range entry.NBTAliases {
			if alias == "" {
				return blockEntityInventory{}, report, fmt.Errorf("empty NBT alias for %q", entry.SourceKey)
			}
			if owner, exists := aliasOwners[alias]; exists {
				return blockEntityInventory{}, report, fmt.Errorf("ambiguous NBT alias %q for %q and %q", alias, owner, entry.SourceKey)
			}
			aliasOwners[alias] = entry.SourceKey
		}
	}

	entries := make([]inventoryEntry, 0, len(source.Entries))
	for _, sourceEntry := range source.Entries {
		renderer := manifestByKey[sourceEntry.SourceKey]
		if !equalOptionalString(sourceEntry.NBTID, renderer.NBTID) {
			return blockEntityInventory{}, report, fmt.Errorf("NBT id drift for %q", sourceEntry.SourceKey)
		}
		if err := validateReviewedEntry(renderer); err != nil {
			return blockEntityInventory{}, report, fmt.Errorf("%s: %w", renderer.SourceKey, err)
		}
		switch renderer.RendererStatus {
		case "implemented":
			report.ImplementedRendererCount++
		case "deferred":
			report.DeferredRendererCount++
		case "unsupported":
			report.UnsupportedRendererCount++
		}
		entryBlockers := strictFinalBlockers(renderer)
		if len(entryBlockers) == 0 {
			report.ProvenRendererCount++
		} else {
			for _, blocker := range entryBlockers {
				report.FinalBlockers = append(report.FinalBlockers, renderer.SourceKey+": "+blocker)
			}
		}
		entries = append(entries, inventoryEntry{
			SourceKey: sourceEntry.SourceKey, NBTID: sourceEntry.NBTID,
			NBTAliases: renderer.NBTAliases, BackingBlocks: sourceEntry.BackingBlocks,
			Source: entrySourceProvenance{
				DragonflyModule: dragonflyModule, DragonflyVersion: dragonflyVersion,
				DragonflyRevision: dragonflyRevision, DragonflyRegistrationSHA256: source.Dragonfly.RegistrationSHA256,
				DragonflySourceFile: sourceEntry.SourceFile, DragonflySourceFileSHA256: sourceEntry.SourceFileSHA256,
				BDSServerVersion: bdsServerVersion, BDSExecutableSHA256: bdsExecutableSHA256,
			},
			RequiredNBTVariants: renderer.RequiredNBTVariants, ChunkNBT: renderer.ChunkNBT,
			LiveUpdate: renderer.LiveUpdate, RendererClass: renderer.RendererClass,
			RendererStatus: renderer.RendererStatus, ImplementationSymbol: renderer.ImplementationSymbol,
			GalleryBuilder: renderer.GalleryBuilder, Witnesses: renderer.Witnesses,
		})
	}
	report.JoinedCount = len(entries)
	report.FinalGatePassed = report.ProvenRendererCount == len(entries) && len(report.FinalBlockers) == 0
	artifact := blockEntityInventory{
		Schema: artifactSchema, ProtocolVersion: protocolVersion, GameVersion: gameVersion,
		Dragonfly: source.Dragonfly, BDS: source.BDS, RendererManifestSHA256: manifest.digest,
		SourceContractSHA256: manifest.sourceContractDigest, RendererContractSHA256: manifest.rendererContractDigest,
		EvidenceCatalogSHA256:      manifest.evidenceDigest,
		CanonicalBlockStateCounted: false, Entries: entries,
	}
	if mode == joinStrictFinal && !report.FinalGatePassed {
		return artifact, report, fmt.Errorf("strict-final gate failed: %s", strings.Join(report.FinalBlockers, "; "))
	}
	return artifact, report, nil
}

func validatePins(source sourceInventory, manifest rendererManifest) error {
	if source.Schema != sourceSchema || source.ProtocolVersion != protocolVersion || source.GameVersion != gameVersion {
		return errors.New("source schema/protocol/game pin drift")
	}
	if source.Dragonfly.Module != dragonflyModule || source.Dragonfly.Version != dragonflyVersion || source.Dragonfly.Revision != dragonflyRevision || source.Dragonfly.RegistrationSHA256 != dragonflyRegistrationSHA256 {
		return errors.New("source Dragonfly pin drift")
	}
	if source.BDS.ServerVersion != bdsServerVersion || source.BDS.ProtocolVersion != protocolVersion || source.BDS.ExecutableBytes != bdsExecutableSize || source.BDS.ExecutableSHA256 != bdsExecutableSHA256 {
		return errors.New("source BDS pin drift")
	}
	if manifest.Schema != rendererSchema || manifest.ProtocolVersion != protocolVersion || manifest.GameVersion != gameVersion {
		return errors.New("renderer manifest schema/protocol/game pin drift")
	}
	if manifest.DragonflyModule != dragonflyModule || manifest.DragonflyVersion != dragonflyVersion || manifest.DragonflyRevision != dragonflyRevision {
		return errors.New("renderer manifest Dragonfly version drift")
	}
	if manifest.DragonflyRegistrationSHA256 != source.Dragonfly.RegistrationSHA256 {
		return errors.New("Dragonfly registration hash drift")
	}
	if manifest.BDSServerVersion != bdsServerVersion || manifest.BDSExecutableSHA256 != source.BDS.ExecutableSHA256 {
		return errors.New("BDS executable hash drift")
	}
	return nil
}

func validateReviewedEntry(entry rendererEntry) error {
	if entry.SourceKey == "" {
		return errors.New("empty source key")
	}
	classes := map[string]bool{"static_block_model": true, "custom_geometry": true, "text_overlay": true, "animated": true, "sourced_logical_invisible": true}
	if !classes[entry.RendererClass] {
		return fmt.Errorf("invalid renderer class %q", entry.RendererClass)
	}
	if entry.RendererStatus != "implemented" && entry.RendererStatus != "deferred" && entry.RendererStatus != "unsupported" {
		return fmt.Errorf("invalid renderer status %q", entry.RendererStatus)
	}
	if !entry.ChunkNBT.Supported || len(entry.ChunkNBT.WitnessIDs) == 0 {
		return errors.New("missing chunk-NBT support or witness")
	}
	if err := validateIdentifiers(entry.ChunkNBT.WitnessIDs, "chunk-NBT witness"); err != nil {
		return err
	}
	if !entry.LiveUpdate.Supported || len(entry.LiveUpdate.WitnessIDs) == 0 {
		return errors.New("missing live-update support or witness")
	}
	if err := validateIdentifiers(entry.LiveUpdate.WitnessIDs, "live-update witness"); err != nil {
		return err
	}
	if err := validateIdentifiers(entry.NBTAliases, "NBT alias"); err != nil {
		return err
	}
	if len(entry.RequiredNBTVariants) == 0 {
		return errors.New("missing required NBT variants")
	}
	expectedVariants, ok := requiredNBTVariants[entry.SourceKey]
	if !ok || len(entry.RequiredNBTVariants) != len(expectedVariants) {
		return errors.New("required NBT variant set mismatch")
	}
	seen := make(map[string]bool, len(entry.RequiredNBTVariants))
	for _, variant := range entry.RequiredNBTVariants {
		if !isTrimmedNonEmpty(variant.VariantID) {
			return errors.New("invalid required NBT variant identifier")
		}
		if seen[variant.VariantID] {
			return fmt.Errorf("duplicate required NBT variant %q", variant.VariantID)
		}
		expectedFields, expected := expectedVariants[variant.VariantID]
		if !expected {
			return errors.New("required NBT variant set mismatch")
		}
		seen[variant.VariantID] = true
		if len(variant.RequiredFields) == 0 {
			return errors.New("invalid required NBT variant")
		}
		if err := validateIdentifiers(variant.RequiredFields, "required NBT field"); err != nil {
			return err
		}
		if !sameIdentifierSet(variant.RequiredFields, expectedFields) {
			return errors.New("required NBT field set mismatch")
		}
		if err := validateIdentifiers(variant.WitnessIDs, "NBT variant witness"); err != nil {
			return err
		}
	}
	for variantID := range expectedVariants {
		if !seen[variantID] {
			return errors.New("required NBT variant set mismatch")
		}
	}
	if entry.ImplementationSymbol != nil && !isTrimmedNonEmpty(*entry.ImplementationSymbol) {
		return errors.New("invalid implementation symbol")
	}
	if entry.GalleryBuilder != nil && !isTrimmedNonEmpty(*entry.GalleryBuilder) {
		return errors.New("invalid gallery builder")
	}
	if err := validateIdentifiers(entry.Witnesses.GPU, "GPU witness"); err != nil {
		return err
	}
	if err := validateIdentifiers(entry.Witnesses.NoDraw, "no-draw witness"); err != nil {
		return err
	}

	switch entry.RendererStatus {
	case "deferred", "unsupported":
		if entry.ImplementationSymbol != nil || entry.GalleryBuilder != nil || hasVariantWitnesses(entry.RequiredNBTVariants) || len(entry.Witnesses.GPU) != 0 || len(entry.Witnesses.NoDraw) != 0 {
			return fmt.Errorf("%s renderer claims implementation or evidence", entry.RendererStatus)
		}
	case "implemented":
		if entry.ImplementationSymbol == nil {
			return errors.New("missing implementation symbol")
		}
		if entry.GalleryBuilder == nil {
			return errors.New("missing gallery builder")
		}
		for _, variant := range entry.RequiredNBTVariants {
			if len(variant.WitnessIDs) == 0 {
				return fmt.Errorf("missing NBT variant witness %s", variant.VariantID)
			}
		}
		if entry.RendererClass == "sourced_logical_invisible" {
			if len(entry.Witnesses.GPU) != 0 {
				return errors.New("invisible renderer claims GPU evidence")
			}
			if len(entry.Witnesses.NoDraw) == 0 {
				return errors.New("missing GPU/no-draw witness")
			}
		} else {
			if len(entry.Witnesses.NoDraw) != 0 {
				return errors.New("drawable renderer claims no-draw evidence")
			}
			if len(entry.Witnesses.GPU) == 0 {
				return errors.New("missing GPU/no-draw witness")
			}
		}
	}
	return nil
}

func validateIdentifiers(values []string, label string) error {
	seen := make(map[string]bool, len(values))
	for _, value := range values {
		if !isTrimmedNonEmpty(value) {
			return fmt.Errorf("invalid %s", label)
		}
		if seen[value] {
			return fmt.Errorf("duplicate %s %q", label, value)
		}
		seen[value] = true
	}
	return nil
}

func sameIdentifierSet(actual, expected []string) bool {
	if len(actual) != len(expected) {
		return false
	}
	want := make(map[string]bool, len(expected))
	for _, value := range expected {
		want[value] = true
	}
	for _, value := range actual {
		if !want[value] {
			return false
		}
	}
	return true
}

func isTrimmedNonEmpty(value string) bool {
	return value != "" && value == strings.TrimSpace(value)
}

func hasVariantWitnesses(variants []nbtVariant) bool {
	for _, variant := range variants {
		if len(variant.WitnessIDs) != 0 {
			return true
		}
	}
	return false
}

func strictFinalBlockers(entry rendererEntry) []string {
	var blockers []string
	if entry.RendererStatus != "implemented" {
		blockers = append(blockers, "renderer status is "+entry.RendererStatus)
	}
	if entry.ImplementationSymbol == nil || *entry.ImplementationSymbol == "" {
		blockers = append(blockers, "missing implementation symbol")
	}
	if entry.GalleryBuilder == nil || *entry.GalleryBuilder == "" {
		blockers = append(blockers, "missing gallery builder")
	}
	for _, variant := range entry.RequiredNBTVariants {
		if len(variant.WitnessIDs) == 0 {
			blockers = append(blockers, "missing NBT variant witness "+variant.VariantID)
		}
	}
	if entry.RendererClass == "sourced_logical_invisible" {
		if len(entry.Witnesses.NoDraw) == 0 {
			blockers = append(blockers, "missing GPU/no-draw witness")
		}
	} else if len(entry.Witnesses.GPU) == 0 {
		blockers = append(blockers, "missing GPU/no-draw witness")
	}
	return blockers
}

func encodeArtifacts(inventory blockEntityInventory, report coverageReport) ([]byte, []byte, error) {
	inventoryJSON, err := marshalCanonical(inventory)
	if err != nil {
		return nil, nil, err
	}
	reportJSON, err := marshalCanonical(report)
	if err != nil {
		return nil, nil, err
	}
	return inventoryJSON, reportJSON, nil
}

func marshalCanonical(value any) ([]byte, error) {
	raw, err := json.MarshalIndent(value, "", "  ")
	if err != nil {
		return nil, err
	}
	return append(raw, '\n'), nil
}

func equalOptionalString(left, right *string) bool {
	if left == nil || right == nil {
		return left == nil && right == nil
	}
	return *left == *right
}

func verifyBDSExecutable(path string) error {
	if err := verifyFileSHA256(path, bdsExecutableSize, bdsExecutableSHA256); err != nil {
		return fmt.Errorf("verify BDS executable: %w", err)
	}
	return nil
}

func verifyFileSHA256(path string, expectedSize int64, expectedSHA256 string) error {
	file, err := os.Open(path)
	if err != nil {
		return fmt.Errorf("open pinned file: %w", err)
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return fmt.Errorf("stat pinned file: %w", err)
	}
	if info.Size() != expectedSize {
		return fmt.Errorf("pinned file size drift: got %d, want %d", info.Size(), expectedSize)
	}
	digest := sha256.New()
	written, err := io.Copy(digest, io.LimitReader(file, expectedSize+1))
	if err != nil {
		return fmt.Errorf("hash pinned file: %w", err)
	}
	if written != expectedSize {
		return fmt.Errorf("pinned file size drift while hashing: got %d, want %d", written, expectedSize)
	}
	if hex.EncodeToString(digest.Sum(nil)) != expectedSHA256 {
		return errors.New("pinned file hash drift")
	}
	return nil
}

func writeAtomic(path string, raw []byte) error {
	directory := filepath.Dir(path)
	if err := os.MkdirAll(directory, 0o755); err != nil {
		return err
	}
	temporary, err := os.CreateTemp(directory, ".blockentitygen-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	defer os.Remove(temporaryPath)
	if _, err := temporary.Write(raw); err != nil {
		temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	if err := os.Rename(temporaryPath, path); err != nil {
		return fmt.Errorf("publish %s: %w", path, err)
	}
	return nil
}

func fatal(err error) {
	fmt.Fprintln(os.Stderr, "blockentitygen:", err)
	os.Exit(1)
}
