package main

import (
	"bytes"
	"encoding/json"
	"fmt"
	"os"
)

const (
	retailReservedName  = "cinnabar:reserved"
	retailReservedCount = 383
)

type sequentialIDRange struct {
	first uint32
	last  uint32
}

type coverageManifest struct {
	Schema          uint32              `json:"schema"`
	Protocol        uint32              `json:"protocol"`
	CanonicalStates uint32              `json:"canonical_states"`
	CoveredStates   uint32              `json:"covered_states"`
	GapRanges       []sequentialIDRange `json:"gap_ranges"`
}

func (span *sequentialIDRange) UnmarshalJSON(data []byte) error {
	var fields struct {
		First uint32 `json:"first"`
		Last  uint32 `json:"last"`
	}
	if err := json.Unmarshal(data, &fields); err != nil {
		return err
	}
	span.first, span.last = fields.First, fields.Last
	return nil
}

var retailReservedRanges = [...]sequentialIDRange{
	{1, 1}, {382, 382}, {1258, 1258}, {1266, 1266}, {1273, 1273}, {1528, 1528},
	{2712, 2712}, {3339, 3340}, {3778, 3778}, {3784, 3784}, {4152, 4157},
	{5373, 5388}, {5526, 5529}, {6105, 6105}, {6316, 6316}, {6416, 6416},
	{6770, 6775}, {6847, 6847}, {6858, 6859}, {7295, 7295}, {7300, 7303},
	{7306, 7306}, {7332, 7332}, {7362, 7365}, {7989, 7989}, {8713, 8874},
	{9257, 9257}, {10018, 10036}, {10394, 10394}, {11699, 11699}, {12519, 12519},
	{12726, 12726}, {12827, 12827}, {12891, 12891}, {12944, 12944}, {12953, 12958},
	{13526, 13526}, {13819, 13819}, {13839, 13839}, {13967, 13967}, {14573, 14573},
	{14586, 14586}, {14637, 14646}, {14648, 14648}, {15027, 15027}, {15115, 15115},
	{15231, 15320}, {15374, 15379}, {15867, 15867}, {16121, 16124}, {16834, 16839},
	{16842, 16842},
}

func isRetailReservedSequentialID(id uint32) bool {
	for _, span := range retailReservedRanges {
		if id < span.first {
			return false
		}
		if id <= span.last {
			return true
		}
	}
	return false
}

func reservedStateJSON(id uint32) []byte {
	return []byte(fmt.Sprintf(`{"reserved_id":{"type":"int","value":%d}}`, id))
}

func projectRetailRegistry(source []Record) ([]Record, error) {
	projected := make([]Record, len(source))
	selected := 0
	for index, record := range source {
		projected[index] = record
		if !isRetailReservedSequentialID(record.SequentialID) {
			continue
		}
		selected++
		projected[index].Flags = 0
		projected[index].Name = retailReservedName
		projected[index].StateJSON = reservedStateJSON(record.SequentialID)
		projected[index].ModelFamily = ModelFamilyUnknown
		projected[index].ContributorRole = ContributorPrimary
		projected[index].ModelState = ModelState{}
		projected[index].FaceCoverage = 0
		projected[index].CollisionSeed = CollisionSeed{}
	}
	if len(source) == physicsRecordCount && selected != retailReservedCount {
		return nil, fmt.Errorf("retail projection selected %d records, want %d", selected, retailReservedCount)
	}
	return projected, nil
}

func validateRetailProjection(source, projected []Record) error {
	if len(source) != len(projected) {
		return fmt.Errorf("projection record count %d does not match source %d", len(projected), len(source))
	}
	selected := 0
	for index := range source {
		a, b := source[index], projected[index]
		if a.SequentialID != b.SequentialID || a.NetworkHash != b.NetworkHash || a.Provenance != b.Provenance {
			return fmt.Errorf("projection changed identity or provenance at index %d", index)
		}
		if !isRetailReservedSequentialID(a.SequentialID) {
			if !recordsEqual(a, b) {
				return fmt.Errorf("projection changed unselected record %d", a.SequentialID)
			}
			continue
		}
		selected++
		if b.Flags != 0 || b.Name != retailReservedName || !bytes.Equal(b.StateJSON, reservedStateJSON(b.SequentialID)) ||
			b.ModelFamily != ModelFamilyUnknown || b.ContributorRole != ContributorPrimary || b.ModelState != (ModelState{}) ||
			b.FaceCoverage != 0 || b.CollisionSeed.ShapeID != 0 || b.CollisionSeed.Confidence != CollisionConfidenceNone || len(b.CollisionSeed.Boxes) != 0 {
			return fmt.Errorf("projected record %d is not neutral", b.SequentialID)
		}
	}
	if len(source) == physicsRecordCount && selected != retailReservedCount {
		return fmt.Errorf("projection selected %d records, want %d", selected, retailReservedCount)
	}
	return nil
}

func recordsEqual(a, b Record) bool {
	return a.SequentialID == b.SequentialID && a.NetworkHash == b.NetworkHash && a.Flags == b.Flags &&
		a.Name == b.Name && bytes.Equal(a.StateJSON, b.StateJSON) && a.ModelFamily == b.ModelFamily &&
		a.ContributorRole == b.ContributorRole && a.ModelState == b.ModelState && a.FaceCoverage == b.FaceCoverage &&
		a.CollisionSeed.ShapeID == b.CollisionSeed.ShapeID && a.CollisionSeed.Confidence == b.CollisionSeed.Confidence &&
		collisionBoxesEqual(a.CollisionSeed.Boxes, b.CollisionSeed.Boxes) && a.Provenance == b.Provenance
}

func collisionBoxesEqual(a, b []CollisionBox) bool {
	if len(a) != len(b) {
		return false
	}
	for index := range a {
		if a[index] != b[index] {
			return false
		}
	}
	return true
}

func metadataForRecords(records []Record) RegistryMetadata {
	allNames := make(map[string]struct{}, len(records))
	valentineNames := make(map[string]struct{}, len(records))
	gapNames := make(map[string]struct{}, len(records))
	metadata := RegistryMetadata{Protocol: registryProtocol, CanonicalStates: uint32(len(records))}
	for _, record := range records {
		allNames[record.Name] = struct{}{}
		if record.Provenance&ProvenanceValentine != 0 {
			metadata.ValentineStates++
			valentineNames[record.Name] = struct{}{}
		} else {
			metadata.ValentineGapStates++
			gapNames[record.Name] = struct{}{}
		}
	}
	metadata.CanonicalNames = uint32(len(allNames))
	metadata.ValentineNames = uint32(len(valentineNames))
	metadata.ValentineGapNames = uint32(len(gapNames))
	return metadata
}

func applyCoverageManifest(records []Record, path string) (ValentineAudit, error) {
	data, err := os.ReadFile(path)
	if err != nil {
		return ValentineAudit{}, fmt.Errorf("read coverage manifest: %w", err)
	}
	if len(data) > 1<<20 {
		return ValentineAudit{}, fmt.Errorf("coverage manifest exceeds 1 MiB")
	}
	var manifest coverageManifest
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return ValentineAudit{}, fmt.Errorf("decode coverage manifest: %w", err)
	}
	if manifest.Schema != 1 || manifest.Protocol != registryProtocol || manifest.CanonicalStates != uint32(len(records)) || manifest.CoveredStates != 15_845 {
		return ValentineAudit{}, fmt.Errorf("coverage manifest header is not schema 1 protocol %d with %d/%d states", registryProtocol, len(records), 15_845)
	}
	gaps := make([]bool, len(records))
	gapCount := 0
	var previous uint32
	for index, span := range manifest.GapRanges {
		if span.first > span.last || span.last >= uint32(len(records)) || (index != 0 && span.first <= previous) {
			return ValentineAudit{}, fmt.Errorf("coverage gap range %d is invalid or out of order", index)
		}
		for id := span.first; id <= span.last; id++ {
			gaps[id] = true
			gapCount++
		}
		previous = span.last
	}
	if gapCount != 1_068 || len(records)-gapCount != int(manifest.CoveredStates) {
		return ValentineAudit{}, fmt.Errorf("coverage manifest has %d gaps and %d covered states", gapCount, len(records)-gapCount)
	}
	coveredNames := make(map[string]struct{}, len(records))
	gapNames := make(map[string]struct{}, gapCount)
	for index := range records {
		if records[index].SequentialID != uint32(index) {
			return ValentineAudit{}, fmt.Errorf("coverage record %d has sequential ID %d", index, records[index].SequentialID)
		}
		if gaps[index] {
			gapNames[records[index].Name] = struct{}{}
			continue
		}
		records[index].Provenance |= ProvenanceValentine
		coveredNames[records[index].Name] = struct{}{}
	}
	audit := ValentineAudit{
		CanonicalNames:  uniqueRecordNameCount(records),
		CanonicalStates: len(records),
		ValentineNames:  len(coveredNames),
		ValentineStates: len(records) - gapCount,
		GapNames:        len(gapNames),
		GapStates:       gapCount,
		Joined:          len(records) - gapCount,
		Missing:         gapCount,
	}
	if audit.CanonicalNames != 1_356 || audit.ValentineNames != 1_321 || audit.GapNames != 35 {
		return ValentineAudit{}, fmt.Errorf("coverage name counts are canonical=%d covered=%d gaps=%d", audit.CanonicalNames, audit.ValentineNames, audit.GapNames)
	}
	return audit, nil
}

func uniqueRecordNameCount(records []Record) int {
	names := make(map[string]struct{}, len(records))
	for _, record := range records {
		names[record.Name] = struct{}{}
	}
	return len(names)
}

func neutralizeReservedLightProperties(properties []byte) error {
	if len(properties) != physicsRecordCount {
		return fmt.Errorf("light property count %d does not match %d", len(properties), physicsRecordCount)
	}
	for id := range properties {
		if isRetailReservedSequentialID(uint32(id)) {
			properties[id] = 0
		}
	}
	return nil
}

func neutralizeReservedPhysics(records []PhysicsRecord) error {
	selected := 0
	for index := range records {
		if !isRetailReservedSequentialID(records[index].SequentialID) {
			continue
		}
		selected++
		records[index].Boxes = nil
		records[index].FrictionQ1E8 = defaultSpeedQ1E8
		records[index].HorizontalSpeedQ1E8 = defaultSpeedQ1E8
		records[index].VerticalSpeedQ1E8 = defaultSpeedQ1E8
		records[index].FluidHeightQ1E8 = 0
		records[index].Flags = physicsFlagPassable
		records[index].SurfaceResponse = SurfaceNone
	}
	if len(records) == physicsRecordCount && selected != retailReservedCount {
		return fmt.Errorf("physics projection selected %d records, want %d", selected, retailReservedCount)
	}
	return nil
}
