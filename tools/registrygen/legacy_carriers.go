package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/binary"
	"encoding/json"
	"errors"
	"fmt"
	"math"
	"os"
	"path/filepath"
	"sort"

	"github.com/df-mc/dragonfly/server/world"
)

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

func encodeLightRegistry(breg []byte, records []Record, registry lightRegistry) ([]byte, error) {
	if registry == nil {
		return nil, errors.New("light registry is nil")
	}
	if len(records) > maxRecordCount {
		return nil, fmt.Errorf("too many light records: %d exceeds %d", len(records), maxRecordCount)
	}
	sorted := append([]Record(nil), records...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].SequentialID < sorted[j].SequentialID })
	for index, record := range sorted {
		if index != 0 && record.SequentialID == sorted[index-1].SequentialID {
			return nil, fmt.Errorf("duplicate sequential ID: %d", record.SequentialID)
		}
		if record.SequentialID != uint32(index) {
			return nil, fmt.Errorf("light registry sequential IDs are not contiguous at index %d: got %d", index, record.SequentialID)
		}
	}

	properties := make([]byte, len(sorted))
	for _, record := range sorted {
		emission := registry.LightBlock(record.SequentialID)
		filter := registry.FilteringBlock(record.SequentialID)
		if emission > 15 {
			return nil, fmt.Errorf("runtime ID %d emission %d exceeds one nibble", record.SequentialID, emission)
		}
		if filter > 15 {
			return nil, fmt.Errorf("runtime ID %d filter %d exceeds one nibble", record.SequentialID, filter)
		}
		properties[record.SequentialID] = emission | filter<<4
	}
	return encodeResolvedLightRegistry(breg, sorted, properties)
}

func encodeResolvedLightRegistry(breg []byte, sorted []Record, properties []byte) ([]byte, error) {
	if len(properties) != len(sorted) {
		return nil, fmt.Errorf("light property count %d does not match %d records", len(properties), len(sorted))
	}
	for index, record := range sorted {
		if record.SequentialID != uint32(index) {
			return nil, fmt.Errorf("resolved light records are not in sequential order at index %d: got %d", index, record.SequentialID)
		}
	}
	bregDigest := sha256.Sum256(breg)
	encoded := make([]byte, 0, len(lightRegistryHeader)+8+sha256.Size+len(sorted)+sha256.Size)
	encoded = append(encoded, lightRegistryHeader...)
	encoded = binary.LittleEndian.AppendUint32(encoded, registryProtocol)
	encoded = binary.LittleEndian.AppendUint32(encoded, uint32(len(sorted)))
	encoded = append(encoded, bregDigest[:]...)
	encoded = append(encoded, properties...)
	digest := sha256.Sum256(encoded)
	encoded = append(encoded, digest[:]...)
	return encoded, nil
}

func validateLightBindingBREG(breg []byte, records []Record) error {
	identities, err := readBREG1003IdentitiesForProtocol(breg, registryProtocol)
	if err != nil {
		return err
	}
	if len(identities) != len(records) {
		return fmt.Errorf("light binding BREG count %d does not match %d source states", len(identities), len(records))
	}
	sorted := append([]Record(nil), records...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].SequentialID < sorted[j].SequentialID })
	for index, identity := range identities {
		record := sorted[index]
		if identity.SequentialID != uint32(index) || record.SequentialID != uint32(index) ||
			identity.NetworkHash != record.NetworkHash || identity.Name != record.Name ||
			!bytes.Equal(identity.StateJSON, record.StateJSON) {
			return fmt.Errorf("light binding BREG identity mismatch at sequential index %d", index)
		}
	}
	return nil
}

func readBREG1003LightIdentities(data []byte) ([]bregLightIdentity, error) {
	return readBREG1003IdentitiesForProtocol(data, registryProtocol)
}

func readBREG1003IdentitiesForProtocol(data []byte, expectedProtocol uint32) ([]bregLightIdentity, error) {
	_, records, err := decodeBREGRecords(data, expectedProtocol)
	if err != nil {
		return nil, err
	}
	identities := make([]bregLightIdentity, len(records))
	for index, record := range records {
		identities[index] = bregLightIdentity{
			SequentialID: record.SequentialID, NetworkHash: record.NetworkHash,
			Name: record.Name, StateJSON: record.StateJSON,
		}
	}
	return identities, nil
}

func encodeAuthoritativeLightRegistry(breg []byte, sourceRecords, records []Record, registry world.BlockRegistry, pmmpRoot string) ([]byte, LightGenerationReport, error) {
	if pmmpRoot == "" {
		return nil, LightGenerationReport{}, errors.New("authoritative light generation requires the pinned PMMP source")
	}
	pmmpLights, err := readPMMPLightProperties(filepath.Join(pmmpRoot, "block_properties_table.json"))
	if err != nil {
		return nil, LightGenerationReport{}, fmt.Errorf("read PMMP light diagnostics: %w", err)
	}
	resolved, report, err := resolveAuthoritativeLightProperties(sourceRecords, registry, pmmpLights)
	if err != nil {
		return nil, LightGenerationReport{}, fmt.Errorf("resolve light metadata: %w", err)
	}
	if err := neutralizeReservedLightProperties(resolved); err != nil {
		return nil, LightGenerationReport{}, err
	}
	bindingDigest := sha256.Sum256(breg)
	report.BREGSHA256 = fmt.Sprintf("%x", bindingDigest)
	encoded, err := encodeResolvedLightRegistry(breg, records, resolved)
	if err != nil {
		return nil, LightGenerationReport{}, err
	}
	return encoded, report, nil
}

func resolveAuthoritativeLightProperties(records []Record, registry world.BlockRegistry, pmmp map[string]PMMPLightProperties) ([]byte, LightGenerationReport, error) {
	if registry == nil {
		return nil, LightGenerationReport{}, errors.New("block registry is nil")
	}
	sorted := append([]Record(nil), records...)
	sort.Slice(sorted, func(i, j int) bool { return sorted[i].SequentialID < sorted[j].SequentialID })
	properties := make([]byte, len(sorted))
	report := LightGenerationReport{DragonflyRevision: dragonflyVersion}
	fallbackNames := make(map[string]bool, len(pmmpLightFallbackIdentifiers))
	for _, name := range pmmpLightFallbackIdentifiers {
		fallbackNames[name] = false
	}
	for index, record := range sorted {
		if record.SequentialID != uint32(index) {
			return nil, LightGenerationReport{}, fmt.Errorf("light registry sequential IDs are not contiguous at index %d: got %d", index, record.SequentialID)
		}
		emission := registry.LightBlock(record.SequentialID)
		filter := registry.FilteringBlock(record.SequentialID)
		if emission > 15 || filter > 15 {
			return nil, LightGenerationReport{}, fmt.Errorf("runtime ID %d returned malformed light %d/%d", record.SequentialID, emission, filter)
		}
		_, eligible := fallbackNames[record.Name]
		// The exact Dragonfly per-RID accessors are primary for every state
		// except the two audited lamp identifiers. Block implementation coverage
		// is intentionally not an authority test: protocol-1001 includes valid
		// states represented internally by Dragonfly's unknownBlock type.
		if !eligible {
			report.DragonflyAccessorStates++
			properties[index] = emission | filter<<4
			continue
		}
		value, found := registry.BlockByRuntimeID(record.SequentialID)
		concrete := found
		if found {
			_, hash := value.Hash()
			concrete = hash != math.MaxUint64
		}
		exact, ok := pmmp[record.Name]
		if !ok {
			return nil, LightGenerationReport{}, fmt.Errorf("required exact PMMP light entry %s is missing", record.Name)
		}
		pmmpEmission, pmmpFilter, err := checkedPMMPLight(record.Name, exact)
		if err != nil {
			return nil, LightGenerationReport{}, err
		}
		if concrete {
			if emission != pmmpEmission || filter != pmmpFilter {
				return nil, LightGenerationReport{}, fmt.Errorf("concrete Dragonfly light %d/%d disagrees with exact PMMP cross-check %d/%d for %s", emission, filter, pmmpEmission, pmmpFilter, record.Name)
			}
			report.DragonflyAccessorStates++
		} else {
			emission, filter = pmmpEmission, pmmpFilter
			report.PMMPFallbackStates++
			report.PMMPFallbackSequentialIDs = append(report.PMMPFallbackSequentialIDs, record.SequentialID)
			fallbackNames[record.Name] = true
		}
		properties[index] = emission | filter<<4
	}
	for _, name := range pmmpLightFallbackIdentifiers {
		if !fallbackNames[name] {
			continue
		}
		report.PMMPFallbackIdentifiers = append(report.PMMPFallbackIdentifiers, name)
	}
	return properties, report, nil
}

func checkedPMMPLight(name string, properties PMMPLightProperties) (uint8, uint8, error) {
	if !isWholeNibble(properties.Brightness) {
		return 0, 0, fmt.Errorf("PMMP brightness %v for %s is not an exact nibble", properties.Brightness, name)
	}
	if math.IsNaN(properties.Opacity) || math.IsInf(properties.Opacity, 0) || properties.Opacity < 0 || properties.Opacity > 1 {
		return 0, 0, fmt.Errorf("PMMP opacity %v for %s is outside 0..1", properties.Opacity, name)
	}
	return uint8(properties.Brightness), uint8(math.Round(properties.Opacity * 15)), nil
}

func isWholeNibble(value float64) bool {
	return !math.IsNaN(value) && !math.IsInf(value, 0) && value >= 0 && value <= 15 && value == math.Trunc(value)
}

func readPMMPLightProperties(path string) (map[string]PMMPLightProperties, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}
	if len(data) > 16<<20 {
		return nil, fmt.Errorf("PMMP block properties exceed 16 MiB")
	}
	var properties map[string]PMMPLightProperties
	if err := json.Unmarshal(data, &properties); err != nil {
		return nil, err
	}
	return properties, nil
}

func encodeWithMetadata(metadata RegistryMetadata, records []Record) ([]byte, error) {
	if len(records) > maxRecordCount {
		return nil, fmt.Errorf("too many records: %d exceeds %d", len(records), maxRecordCount)
	}
	if metadata.Protocol != registryProtocol && metadata.Protocol != v2193BlockProtocol {
		return nil, fmt.Errorf("registry protocol %d is unsupported", metadata.Protocol)
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
		if record.BeaconTint > maxBeaconTint || (metadata.Protocol == registryProtocol && record.BeaconTint != BeaconTintUnresolved) {
			return nil, fmt.Errorf("invalid beacon tint %d for protocol %d", record.BeaconTint, metadata.Protocol)
		}
		if record.Traits & ^uint64(allBlockTraits) != 0 || record.Survival > maxSurvivalRule {
			return nil, fmt.Errorf("invalid compiled traits for sequential ID %d", record.SequentialID)
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
		for boxIndex, box := range record.CollisionSeed.Boxes {
			if err := validateCollisionBox(box); err != nil {
				return nil, fmt.Errorf("collision box %d for sequential ID %d: %w", boxIndex, record.SequentialID, err)
			}
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

	encoded := appendRegistryMetadata(nil, metadata)
	for _, record := range sorted {
		encoded = appendRecordHeader(encoded, record, metadata.Protocol == registryProtocol)
		for _, box := range record.CollisionSeed.Boxes {
			encoded = appendCollisionBox(encoded, box)
		}
		encoded = append(encoded, record.Name...)
		encoded = append(encoded, record.StateJSON...)
	}
	return encoded, nil
}

func slicesContainsKey[V any](values map[string]V, key string) bool {
	_, ok := values[key]
	return ok
}
