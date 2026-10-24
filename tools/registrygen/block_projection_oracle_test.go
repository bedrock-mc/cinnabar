package main

import (
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"slices"
	"sort"

	shared "github.com/cinnabar-mc/protocolgen/generated/data"
)

type v2193BlockProjectionManifest struct {
	Schema      string `json:"schema"`
	GameVersion string `json:"game_version"`
	Protocol    uint32 `json:"protocol"`
	Source      struct {
		Module           string `json:"module"`
		SourceLockSHA256 string `json:"source_lock_sha256"`
		CloudburstRef    string `json:"cloudburst_ref"`
	} `json:"source"`
	Allowlist struct {
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"allowlist"`
	Projection struct {
		States              int      `json:"states"`
		LegacyExactStates   int      `json:"legacy_exact_states"`
		AuthoredStates      int      `json:"authored_states"`
		LegacyReducedStates int      `json:"legacy_reduced_states"`
		TwinStates          int      `json:"twin_states"`
		TwinNames           int      `json:"twin_names"`
		TwinFingerprint     string   `json:"twin_fingerprint"`
		UnresolvedRetail    []string `json:"unresolved_retail"`
		DeniedCount         int      `json:"denied_count"`
		DeniedFingerprint   string   `json:"denied_fingerprint"`
		EducationStates     int      `json:"education_states"`
	} `json:"projection"`
	Output struct {
		Format string `json:"format"`
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"output"`
	Light struct {
		Format string `json:"format"`
		Path   string `json:"path"`
		SHA256 string `json:"sha256"`
	} `json:"light"`
}

type v2193ProjectionStats struct {
	legacy, reduced, twins, additions, denied, education, authored int
	deniedFingerprint, additionFingerprint                         string
}

// Projection classes recorded per canonical source key.
const (
	v2193ClassDenied byte = iota
	v2193ClassLegacyExact
	v2193ClassAddition
	v2193ClassLegacyReduced
	v2193ClassTwin
	v2193ClassEducation
	v2193ClassAuthored
)

// v2193Projection records, per source index, the projected record, its class,
// and the protocol-1001 record index that supplied its facts (-1 if none).
type v2193Projection struct {
	records []Record
	classes []byte
	facts   []int
}

func projectV2193Blocks(source, legacy []Record, allowed map[string]struct{}) (v2193Projection, v2193ProjectionStats, error) {
	if len(source) != v2193BlockStateCount {
		return v2193Projection{}, v2193ProjectionStats{}, errors.New("v2193 block source count does not match")
	}
	legacyByKey := make(map[string]int, len(legacy))
	for index, record := range legacy {
		if record.Name == retailReservedName {
			continue
		}
		legacyByKey[canonicalRecordKey(record.Name, record.StateJSON)] = index
	}
	projection := v2193Projection{
		records: make([]Record, len(source)), classes: make([]byte, len(source)),
		facts: make([]int, len(source)),
	}
	projected := projection.records
	deniedHash := sha256.New()
	additionHash := sha256.New()
	stats := v2193ProjectionStats{}
	adopt := func(index int, identity Record, legacyIndex int, class byte) {
		facts := legacy[legacyIndex]
		facts.SequentialID, facts.NetworkHash = identity.SequentialID, identity.NetworkHash
		facts.Name, facts.StateJSON = identity.Name, append([]byte(nil), identity.StateJSON...)
		facts.CollisionSeed.Boxes = append([]CollisionBox(nil), facts.CollisionSeed.Boxes...)
		projected[index] = facts
		projection.classes[index], projection.facts[index] = class, legacyIndex
	}
	for index, identity := range source {
		projection.facts[index] = -1
		if record, matched, err := educationRenderRecord(identity); err != nil {
			return v2193Projection{}, v2193ProjectionStats{}, err
		} else if matched {
			projected[index] = record
			projection.classes[index] = v2193ClassEducation
			stats.education++
			continue
		}
		if _, retail := allowed[identity.Name]; retail && (identity.Name == "minecraft:shelf_mushroom" || identity.Name == "minecraft:straw_bed") {
			projected[index] = identity
			projection.classes[index] = v2193ClassAuthored
			stats.authored++
			continue
		}
		key := canonicalRecordKey(identity.Name, identity.StateJSON)
		if old, ok := legacyByKey[key]; ok {
			adopt(index, identity, old, v2193ClassLegacyExact)
			stats.legacy++
			continue
		}
		reduced, err := v2193ReducedState(identity.StateJSON)
		if err != nil {
			return v2193Projection{}, v2193ProjectionStats{}, fmt.Errorf("runtime ID %d: %w", identity.SequentialID, err)
		}
		if old, ok := legacyByKey[canonicalRecordKey(identity.Name, reduced)]; ok {
			adopt(index, identity, old, v2193ClassLegacyReduced)
			stats.reduced++
			continue
		}
		twin, twinned := v2193TwinFor(identity.Name)
		item := identity.Name
		if twinned {
			item = twin.item
		}
		_, retail := allowed[item]
		if !retail || slices.Contains(v2193UnresolvedRetail, identity.Name) {
			projected[index] = Record{
				SequentialID: identity.SequentialID, NetworkHash: identity.NetworkHash,
				Name: retailReservedName, StateJSON: reservedStateJSON(identity.SequentialID),
				ContributorRole: ContributorPrimary, Provenance: ProvenanceDragonfly,
			}
			stats.denied++
			writeV2193Fingerprint(deniedHash, identity, key)
			continue
		}
		if twinned {
			old, found := legacyByKey[canonicalRecordKey(twin.twin, reduced)]
			if !found {
				return v2193Projection{}, v2193ProjectionStats{}, fmt.Errorf("reviewed twin %s has no protocol-1001 state for %s %s", twin.twin, identity.Name, reduced)
			}
			adopt(index, identity, old, v2193ClassTwin)
			stats.twins++
			continue
		}
		projected[index] = identity
		projection.classes[index] = v2193ClassAddition
		stats.additions++
		writeV2193Fingerprint(additionHash, identity, key)
	}
	for index := range projected {
		if err := applyFamilyRouting(&projected[index]); err != nil {
			return v2193Projection{}, v2193ProjectionStats{}, err
		}
	}
	stats.deniedFingerprint = fmt.Sprintf("%x", deniedHash.Sum(nil))
	stats.additionFingerprint = fmt.Sprintf("%x", additionHash.Sum(nil))
	return projection, stats, nil
}

func writeV2193Fingerprint(hash io.Writer, identity Record, key string) {
	var raw [8]byte
	binary.LittleEndian.PutUint32(raw[:4], identity.SequentialID)
	binary.LittleEndian.PutUint32(raw[4:], identity.NetworkHash)
	hash.Write(raw[:])
	hash.Write([]byte(key))
	hash.Write([]byte{0})
}

func encodeResolvedLightRegistryForProtocol(protocol uint32, breg []byte, records []Record, properties []byte) ([]byte, error) {
	if len(records) != len(properties) {
		return nil, errors.New("light property count does not match BREG")
	}
	digest := sha256.Sum256(breg)
	encoded := append([]byte(lightRegistryHeader), make([]byte, 0)...)
	encoded = binary.LittleEndian.AppendUint32(encoded, protocol)
	encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(records)))
	encoded = append(encoded, digest[:]...)
	encoded = append(encoded, properties...)
	payloadDigest := sha256.Sum256(encoded)
	return append(encoded, payloadDigest[:]...), nil
}

// writeV2193BlockProjection combines shared facts with the client's reviewed rendering and retail admission policy.
func writeV2193BlockProjectionOracle(legacyBREGPath, allowlistPath, outputPath, lightOutputPath, manifestPath string) error {
	if lightOutputPath == "" {
		return errors.New("v2193 block projection requires -light-out")
	}
	source, err := v2193SourceRecords()
	if err != nil {
		return err
	}
	legacyBytes, err := os.ReadFile(legacyBREGPath)
	if err != nil {
		return fmt.Errorf("read legacy BREG: %w", err)
	}
	_, legacy, err := decodeBREGRecords(legacyBytes, registryProtocol)
	if err != nil {
		return err
	}
	allowed, err := parseV2193RetailItems(allowlistPath)
	if err != nil {
		return err
	}
	projection, stats, err := projectV2193Blocks(source, legacy, allowed)
	if err != nil {
		return err
	}
	if stats.additions != 0 {
		return fmt.Errorf("v2193 projection has %d retail states without reviewed facts (fingerprint %s)", stats.additions, stats.additionFingerprint)
	}
	if stats.education != v2193EducationStateCount() {
		return fmt.Errorf("Education construction palette contains %d states, want %d", stats.education, v2193EducationStateCount())
	}
	projected := projection.records
	if err := applyBeaconTints(projected, source); err != nil {
		return err
	}
	properties, err := applySharedBlockFacts(projected)
	if err != nil {
		return err
	}
	metadata := metadataForRecords(projected)
	metadata.Protocol = v2193BlockProtocol
	encoded, err := encodeWithMetadata(metadata, projected)
	if err != nil {
		return err
	}

	light, err := encodeResolvedLightRegistryForProtocol(v2193BlockProtocol, encoded, projected, properties)
	if err != nil {
		return err
	}

	twinNames := make([]string, 0, len(v2193Twins))
	for name := range v2193Twins {
		twinNames = append(twinNames, name)
	}
	sort.Strings(twinNames)
	twinHash := sha256.New()
	for _, name := range twinNames {
		fmt.Fprintf(twinHash, "%s\x00%s\x00%s\x00", name, v2193Twins[name].twin, v2193Twins[name].item)
	}
	manifest := v2193BlockProjectionManifest{Schema: v2193BlockManifestSchema, GameVersion: v2193GameVersion, Protocol: v2193BlockProtocol}
	manifest.Source.Module = shared.ModulePath
	manifest.Source.SourceLockSHA256, manifest.Source.CloudburstRef = shared.SourceLockSHA256, shared.CloudburstRef
	manifest.Allowlist.Path, manifest.Allowlist.SHA256 = v2193RetailItemsPath, v2193RetailItemsSHA256
	manifest.Projection.States, manifest.Projection.LegacyExactStates = len(projected), stats.legacy
	manifest.Projection.LegacyReducedStates, manifest.Projection.TwinStates = stats.reduced, stats.twins
	manifest.Projection.TwinNames, manifest.Projection.TwinFingerprint = len(twinNames), fmt.Sprintf("%x", twinHash.Sum(nil))
	manifest.Projection.UnresolvedRetail = append([]string{}, v2193UnresolvedRetail...)
	manifest.Projection.DeniedCount, manifest.Projection.DeniedFingerprint = stats.denied, stats.deniedFingerprint
	manifest.Projection.EducationStates = stats.education
	manifest.Projection.AuthoredStates = stats.authored
	manifest.Output.Format, manifest.Output.Path, manifest.Output.SHA256 = registryHeader, v2193BlockOutputPath, fmt.Sprintf("%x", sha256.Sum256(encoded))
	manifest.Light.Format, manifest.Light.Path, manifest.Light.SHA256 = lightRegistryHeader, v2193LightOutputPath, fmt.Sprintf("%x", sha256.Sum256(light))
	manifestBytes, err := json.MarshalIndent(manifest, "", "  ")
	if err != nil {
		return err
	}
	manifestBytes = append(manifestBytes, '\n')

	for _, path := range []string{outputPath, lightOutputPath, manifestPath} {
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			return err
		}
	}
	for _, output := range []struct {
		path string
		data []byte
	}{{outputPath, encoded}, {lightOutputPath, light}} {
		if err := os.WriteFile(output.path, output.data, 0o644); err != nil {
			return err
		}
		checksum := fmt.Sprintf("%x  %s\n", sha256.Sum256(output.data), filepath.Base(output.path))
		if err := os.WriteFile(output.path+".sha256", []byte(checksum), 0o644); err != nil {
			return err
		}
	}
	return os.WriteFile(manifestPath, manifestBytes, 0o644)
}

// v2193Twin names the protocol-1001 block whose identical state schema and
// shape supply facts for a retail 1.26.50 block, and the retail item that
// admits it.
type v2193Twin struct{ twin, item string }

// v2193Twins covers every retail 1.26.50 block without a protocol-1001 state.
// Model, collision, light and friction follow the twin; the identity and the
// resource-pack visual name stay the new block's.
var v2193Twins = map[string]v2193Twin{
	"minecraft:black_concrete_double_slab":      {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:black_concrete_slab"},
	"minecraft:black_concrete_slab":             {twin: "minecraft:smooth_stone_slab", item: "minecraft:black_concrete_slab"},
	"minecraft:black_concrete_stairs":           {twin: "minecraft:stone_stairs", item: "minecraft:black_concrete_stairs"},
	"minecraft:black_wool_double_slab":          {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:black_wool_slab"},
	"minecraft:black_wool_slab":                 {twin: "minecraft:smooth_stone_slab", item: "minecraft:black_wool_slab"},
	"minecraft:black_wool_stairs":               {twin: "minecraft:stone_stairs", item: "minecraft:black_wool_stairs"},
	"minecraft:blue_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:blue_concrete_slab"},
	"minecraft:blue_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:blue_concrete_slab"},
	"minecraft:blue_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:blue_concrete_stairs"},
	"minecraft:blue_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:blue_wool_slab"},
	"minecraft:blue_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:blue_wool_slab"},
	"minecraft:blue_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:blue_wool_stairs"},
	"minecraft:brown_concrete_double_slab":      {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:brown_concrete_slab"},
	"minecraft:brown_concrete_slab":             {twin: "minecraft:smooth_stone_slab", item: "minecraft:brown_concrete_slab"},
	"minecraft:brown_concrete_stairs":           {twin: "minecraft:stone_stairs", item: "minecraft:brown_concrete_stairs"},
	"minecraft:brown_wool_double_slab":          {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:brown_wool_slab"},
	"minecraft:brown_wool_slab":                 {twin: "minecraft:smooth_stone_slab", item: "minecraft:brown_wool_slab"},
	"minecraft:brown_wool_stairs":               {twin: "minecraft:stone_stairs", item: "minecraft:brown_wool_stairs"},
	"minecraft:cyan_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:cyan_concrete_slab"},
	"minecraft:cyan_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:cyan_concrete_slab"},
	"minecraft:cyan_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:cyan_concrete_stairs"},
	"minecraft:cyan_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:cyan_wool_slab"},
	"minecraft:cyan_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:cyan_wool_slab"},
	"minecraft:cyan_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:cyan_wool_stairs"},
	"minecraft:gray_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:gray_concrete_slab"},
	"minecraft:gray_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:gray_concrete_slab"},
	"minecraft:gray_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:gray_concrete_stairs"},
	"minecraft:gray_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:gray_wool_slab"},
	"minecraft:gray_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:gray_wool_slab"},
	"minecraft:gray_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:gray_wool_stairs"},
	"minecraft:green_concrete_double_slab":      {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:green_concrete_slab"},
	"minecraft:green_concrete_slab":             {twin: "minecraft:smooth_stone_slab", item: "minecraft:green_concrete_slab"},
	"minecraft:green_concrete_stairs":           {twin: "minecraft:stone_stairs", item: "minecraft:green_concrete_stairs"},
	"minecraft:green_wool_double_slab":          {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:green_wool_slab"},
	"minecraft:green_wool_slab":                 {twin: "minecraft:smooth_stone_slab", item: "minecraft:green_wool_slab"},
	"minecraft:green_wool_stairs":               {twin: "minecraft:stone_stairs", item: "minecraft:green_wool_stairs"},
	"minecraft:light_blue_concrete_double_slab": {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:light_blue_concrete_slab"},
	"minecraft:light_blue_concrete_slab":        {twin: "minecraft:smooth_stone_slab", item: "minecraft:light_blue_concrete_slab"},
	"minecraft:light_blue_concrete_stairs":      {twin: "minecraft:stone_stairs", item: "minecraft:light_blue_concrete_stairs"},
	"minecraft:light_blue_wool_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:light_blue_wool_slab"},
	"minecraft:light_blue_wool_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:light_blue_wool_slab"},
	"minecraft:light_blue_wool_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:light_blue_wool_stairs"},
	"minecraft:light_gray_concrete_double_slab": {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:light_gray_concrete_slab"},
	"minecraft:light_gray_concrete_slab":        {twin: "minecraft:smooth_stone_slab", item: "minecraft:light_gray_concrete_slab"},
	"minecraft:light_gray_concrete_stairs":      {twin: "minecraft:stone_stairs", item: "minecraft:light_gray_concrete_stairs"},
	"minecraft:light_gray_wool_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:light_gray_wool_slab"},
	"minecraft:light_gray_wool_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:light_gray_wool_slab"},
	"minecraft:light_gray_wool_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:light_gray_wool_stairs"},
	"minecraft:lime_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:lime_concrete_slab"},
	"minecraft:lime_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:lime_concrete_slab"},
	"minecraft:lime_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:lime_concrete_stairs"},
	"minecraft:lime_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:lime_wool_slab"},
	"minecraft:lime_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:lime_wool_slab"},
	"minecraft:lime_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:lime_wool_stairs"},
	"minecraft:magenta_concrete_double_slab":    {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:magenta_concrete_slab"},
	"minecraft:magenta_concrete_slab":           {twin: "minecraft:smooth_stone_slab", item: "minecraft:magenta_concrete_slab"},
	"minecraft:magenta_concrete_stairs":         {twin: "minecraft:stone_stairs", item: "minecraft:magenta_concrete_stairs"},
	"minecraft:magenta_wool_double_slab":        {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:magenta_wool_slab"},
	"minecraft:magenta_wool_slab":               {twin: "minecraft:smooth_stone_slab", item: "minecraft:magenta_wool_slab"},
	"minecraft:magenta_wool_stairs":             {twin: "minecraft:stone_stairs", item: "minecraft:magenta_wool_stairs"},
	"minecraft:orange_concrete_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:orange_concrete_slab"},
	"minecraft:orange_concrete_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:orange_concrete_slab"},
	"minecraft:orange_concrete_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:orange_concrete_stairs"},
	"minecraft:orange_poplar_leaves":            {twin: "minecraft:oak_leaves", item: "minecraft:orange_poplar_leaves"},
	"minecraft:orange_wool_double_slab":         {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:orange_wool_slab"},
	"minecraft:orange_wool_slab":                {twin: "minecraft:smooth_stone_slab", item: "minecraft:orange_wool_slab"},
	"minecraft:orange_wool_stairs":              {twin: "minecraft:stone_stairs", item: "minecraft:orange_wool_stairs"},
	"minecraft:pink_concrete_double_slab":       {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:pink_concrete_slab"},
	"minecraft:pink_concrete_slab":              {twin: "minecraft:smooth_stone_slab", item: "minecraft:pink_concrete_slab"},
	"minecraft:pink_concrete_stairs":            {twin: "minecraft:stone_stairs", item: "minecraft:pink_concrete_stairs"},
	"minecraft:pink_wool_double_slab":           {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:pink_wool_slab"},
	"minecraft:pink_wool_slab":                  {twin: "minecraft:smooth_stone_slab", item: "minecraft:pink_wool_slab"},
	"minecraft:pink_wool_stairs":                {twin: "minecraft:stone_stairs", item: "minecraft:pink_wool_stairs"},
	"minecraft:poplar_button":                   {twin: "minecraft:wooden_button", item: "minecraft:poplar_button"},
	"minecraft:poplar_door":                     {twin: "minecraft:wooden_door", item: "minecraft:poplar_door"},
	"minecraft:poplar_double_slab":              {twin: "minecraft:oak_double_slab", item: "minecraft:poplar_slab"},
	"minecraft:poplar_fence":                    {twin: "minecraft:oak_fence", item: "minecraft:poplar_fence"},
	"minecraft:poplar_fence_gate":               {twin: "minecraft:fence_gate", item: "minecraft:poplar_fence_gate"},
	"minecraft:poplar_hanging_sign":             {twin: "minecraft:oak_hanging_sign", item: "minecraft:poplar_hanging_sign"},
	"minecraft:poplar_log":                      {twin: "minecraft:oak_log", item: "minecraft:poplar_log"},
	"minecraft:poplar_planks":                   {twin: "minecraft:oak_planks", item: "minecraft:poplar_planks"},
	"minecraft:poplar_pressure_plate":           {twin: "minecraft:wooden_pressure_plate", item: "minecraft:poplar_pressure_plate"},
	"minecraft:poplar_sapling":                  {twin: "minecraft:oak_sapling", item: "minecraft:poplar_sapling"},
	"minecraft:poplar_shelf":                    {twin: "minecraft:oak_shelf", item: "minecraft:poplar_shelf"},
	"minecraft:poplar_slab":                     {twin: "minecraft:oak_slab", item: "minecraft:poplar_slab"},
	"minecraft:poplar_stairs":                   {twin: "minecraft:oak_stairs", item: "minecraft:poplar_stairs"},
	"minecraft:poplar_standing_sign":            {twin: "minecraft:standing_sign", item: "minecraft:poplar_sign"},
	"minecraft:poplar_trapdoor":                 {twin: "minecraft:trapdoor", item: "minecraft:poplar_trapdoor"},
	"minecraft:poplar_wall_sign":                {twin: "minecraft:wall_sign", item: "minecraft:poplar_sign"},
	"minecraft:poplar_wood":                     {twin: "minecraft:oak_wood", item: "minecraft:poplar_wood"},
	"minecraft:purple_concrete_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:purple_concrete_slab"},
	"minecraft:purple_concrete_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:purple_concrete_slab"},
	"minecraft:purple_concrete_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:purple_concrete_stairs"},
	"minecraft:purple_wool_double_slab":         {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:purple_wool_slab"},
	"minecraft:purple_wool_slab":                {twin: "minecraft:smooth_stone_slab", item: "minecraft:purple_wool_slab"},
	"minecraft:purple_wool_stairs":              {twin: "minecraft:stone_stairs", item: "minecraft:purple_wool_stairs"},
	"minecraft:red_concrete_double_slab":        {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:red_concrete_slab"},
	"minecraft:red_concrete_slab":               {twin: "minecraft:smooth_stone_slab", item: "minecraft:red_concrete_slab"},
	"minecraft:red_concrete_stairs":             {twin: "minecraft:stone_stairs", item: "minecraft:red_concrete_stairs"},
	"minecraft:red_poplar_leaves":               {twin: "minecraft:oak_leaves", item: "minecraft:red_poplar_leaves"},
	"minecraft:red_shrub":                       {twin: "minecraft:deadbush", item: "minecraft:red_shrub"},
	"minecraft:red_wool_double_slab":            {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:red_wool_slab"},
	"minecraft:red_wool_slab":                   {twin: "minecraft:smooth_stone_slab", item: "minecraft:red_wool_slab"},
	"minecraft:red_wool_stairs":                 {twin: "minecraft:stone_stairs", item: "minecraft:red_wool_stairs"},
	"minecraft:stripped_poplar_log":             {twin: "minecraft:stripped_oak_log", item: "minecraft:stripped_poplar_log"},
	"minecraft:stripped_poplar_wood":            {twin: "minecraft:stripped_oak_wood", item: "minecraft:stripped_poplar_wood"},
	"minecraft:white_concrete_double_slab":      {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:white_concrete_slab"},
	"minecraft:white_concrete_slab":             {twin: "minecraft:smooth_stone_slab", item: "minecraft:white_concrete_slab"},
	"minecraft:white_concrete_stairs":           {twin: "minecraft:stone_stairs", item: "minecraft:white_concrete_stairs"},
	"minecraft:white_wool_double_slab":          {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:white_wool_slab"},
	"minecraft:white_wool_slab":                 {twin: "minecraft:smooth_stone_slab", item: "minecraft:white_wool_slab"},
	"minecraft:white_wool_stairs":               {twin: "minecraft:stone_stairs", item: "minecraft:white_wool_stairs"},
	"minecraft:yellow_concrete_double_slab":     {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:yellow_concrete_slab"},
	"minecraft:yellow_concrete_slab":            {twin: "minecraft:smooth_stone_slab", item: "minecraft:yellow_concrete_slab"},
	"minecraft:yellow_concrete_stairs":          {twin: "minecraft:stone_stairs", item: "minecraft:yellow_concrete_stairs"},
	"minecraft:yellow_poplar_leaves":            {twin: "minecraft:oak_leaves", item: "minecraft:yellow_poplar_leaves"},
	"minecraft:yellow_wool_double_slab":         {twin: "minecraft:smooth_stone_double_slab", item: "minecraft:yellow_wool_slab"},
	"minecraft:yellow_wool_slab":                {twin: "minecraft:smooth_stone_slab", item: "minecraft:yellow_wool_slab"},
	"minecraft:yellow_wool_stairs":              {twin: "minecraft:stone_stairs", item: "minecraft:yellow_wool_stairs"},
}

// v2193UnresolvedRetail are retail 1.26.50 blocks with no schema-identical
// twin; they stay reserved until a reviewed fact source exists.
var v2193UnresolvedRetail = []string{}

func v2193TwinFor(name string) (v2193Twin, bool) {
	twin, ok := v2193Twins[name]
	return twin, ok
}
