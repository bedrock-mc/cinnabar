package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"

	"github.com/df-mc/dragonfly/server/world"
	"github.com/hashimthearab/rust-mcbe/tools/registrygen/internal/targetpin"
)

// The protocol-2193 physics projection reuses the reviewed protocol-1001
// physics pipeline: the checked-in v2193 block registry carries the complete
// fact payloads of the protocol-1001 record each state was projected from,
// including verbatim collision seeds. Legacy and connection-reduced states keep
// their protocol-1001 name and its pinned PMMP friction row; reviewed twins
// read the row of the twin whose facts they carry. Reserved identities keep the
// exact protocol-1001 neutral treatment: treat-as-air passables with default
// factors and no fluid.

const (
	v2193PhysicsOutputPath    = "crates/assets/data/block-physics-v2193.bin"
	v2193PhysicsBREGInputPath = "crates/assets/data/block-registry-v2193.bin"
	v2193PhysicsReservedCount = 662 - v2193EducationStateCount
)

func writeV2193PhysicsProjection(bregPath, pmmpRoot, prismarineRoot, outputPath, shaOutputPath, manifestPath string) error {
	if bregPath == "" || outputPath == "" {
		return errors.New("v2193 physics projection requires a binding BREG and an output path")
	}
	data, err := os.ReadFile(bregPath)
	if err != nil {
		return fmt.Errorf("read v2193 physics-binding BREG: %w", err)
	}
	if len(data) > 128<<20 {
		return errors.New("v2193 physics-binding BREG exceeds 128 MiB")
	}
	blockHash, err := targetpin.BlockHash()
	if err != nil {
		return err
	}
	if actual := fmt.Sprintf("%x", sha256.Sum256(data)); actual != blockHash {
		return fmt.Errorf("pinned protocol-2193 block registry SHA-256 %s does not match %s", actual, blockHash)
	}
	if pmmpRoot == "" || prismarineRoot == "" {
		return errors.New("v2193 physics projection requires the pinned PMMP and Prismarine sources")
	}
	_, records, err := decodeBREGRecords(data, v2193BlockProtocol)
	if err != nil {
		return err
	}
	sources, err := loadPinnedPhysicsSources(pmmpRoot, prismarineRoot, world.DefaultBlockRegistry)
	if err != nil {
		return err
	}
	physics, err := projectV2193PhysicsRecords(records, sources)
	if err != nil {
		return err
	}
	if manifestPath != "" {
		payload, err := os.ReadFile(manifestPath)
		if err != nil {
			return fmt.Errorf("read v2193 physics projection manifest: %w", err)
		}
		if err := crossCheckV2193PhysicsManifest(payload); err != nil {
			return err
		}
	}
	encoded, err := encodePhysicsRegistryForProtocol(data, physics, v2193BlockStateCount, v2193BlockProtocol)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(outputPath), 0o755); err != nil {
		return fmt.Errorf("create v2193 physics output directory: %w", err)
	}
	if err := os.WriteFile(outputPath, encoded, 0o644); err != nil {
		return fmt.Errorf("write v2193 physics output: %w", err)
	}
	if shaOutputPath == "" {
		shaOutputPath = strings.TrimSuffix(outputPath, filepath.Ext(outputPath)) + ".sha256"
	}
	digest := sha256.Sum256(encoded)
	if err := os.WriteFile(shaOutputPath, []byte(fmt.Sprintf("%x\n", digest)), 0o644); err != nil {
		return fmt.Errorf("write v2193 physics checksum: %w", err)
	}
	return nil
}

// projectV2193PhysicsRecords derives one physics record per checked-in v2193
// block state through the shared reviewed pipeline: collision boxes from the
// registry's verbatim seeds, pinned PMMP friction normalized to Q1E8 by name,
// default speed factors, fluid heights from liquid depth, and the same
// reviewed override families cross-checked against the supplied states with
// production coverage required. It fails closed naming every non-reserved name
// that lacks a PMMP row, so future class-2 additions cannot silently inherit
// guessed movement facts.
func projectV2193PhysicsRecords(records []Record, sources PhysicsSourceCatalog) ([]PhysicsRecord, error) {
	if len(records) != v2193BlockStateCount {
		return nil, fmt.Errorf("v2193 physics record count %d does not match %d", len(records), v2193BlockStateCount)
	}
	build := make([]Record, 0, len(records))
	missing := make(map[string]struct{})
	reservedCount := 0
	for index, record := range records {
		if record.SequentialID != uint32(index) {
			return nil, fmt.Errorf("v2193 physics record %d has sequential ID %d", index, record.SequentialID)
		}
		if record.Name == retailReservedName {
			reservedCount++
			continue
		}
		if twin, ok := v2193TwinFor(record.Name); ok {
			record.Name = twin.twin
		}
		if _, ok := sources.PMMP[record.Name]; !ok {
			missing[record.Name] = struct{}{}
		}
		build = append(build, record)
	}
	if len(missing) > 0 {
		names := make([]string, 0, len(missing))
		for name := range missing {
			names = append(names, name)
		}
		sort.Strings(names)
		return nil, fmt.Errorf("v2193 physics projection fails closed: %d non-reserved names have no pinned PMMP friction row (future class-2 additions require reviewed rows before this projection): %s", len(names), strings.Join(names, ", "))
	}
	if reservedCount != v2193PhysicsReservedCount {
		return nil, fmt.Errorf("v2193 physics projection found %d cinnabar:reserved states, want exactly %d", reservedCount, v2193PhysicsReservedCount)
	}
	built, err := buildPhysicsRecords(build, sources)
	if err != nil {
		return nil, err
	}
	physics := make([]PhysicsRecord, len(records))
	cursor := 0
	for index, record := range records {
		if record.Name == retailReservedName {
			physics[index] = PhysicsRecord{
				SequentialID:        record.SequentialID,
				NetworkHash:         record.NetworkHash,
				FrictionQ1E8:        defaultSpeedQ1E8,
				HorizontalSpeedQ1E8: defaultSpeedQ1E8,
				VerticalSpeedQ1E8:   defaultSpeedQ1E8,
				Flags:               physicsFlagPassable,
			}
			continue
		}
		physics[index] = built[cursor]
		cursor++
	}
	return physics, nil
}

func crossCheckV2193PhysicsManifest(payload []byte) error {
	var manifest v2193BlockProjectionManifest
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return fmt.Errorf("decode v2193 physics projection manifest: %w", err)
	}
	if manifest.Projection.DeniedCount != v2193PhysicsReservedCount {
		return fmt.Errorf("v2193 projection manifest denies %d states, want exactly %d", manifest.Projection.DeniedCount, v2193PhysicsReservedCount)
	}
	if manifest.Projection.EducationStates != v2193EducationStateCount {
		return fmt.Errorf("v2193 projection manifest has %d Education states, want %d", manifest.Projection.EducationStates, v2193EducationStateCount)
	}
	blockHash, err := targetpin.BlockHash()
	if err != nil {
		return err
	}
	if manifest.Output.SHA256 != blockHash {
		return fmt.Errorf("v2193 projection manifest binds BREG SHA-256 %q, want %q", manifest.Output.SHA256, blockHash)
	}
	return nil
}
