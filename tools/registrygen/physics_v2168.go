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
)

// The protocol-2168 physics projection reuses the reviewed protocol-1001
// physics pipeline unchanged: the checked-in v2168 block registry carries the
// complete class-1 fact payloads, including verbatim collision seeds; every
// non-reserved name is a legacy-exact protocol-1001 name whose pinned PMMP
// friction row still applies by name; and denied states were already renamed
// to cinnabar:reserved during the block projection. Only the identity space,
// the reserved detection mechanism (name-based instead of sequential-ID
// ranges), and the stamped wire protocol differ. Reserved identities keep the
// exact protocol-1001 neutral treatment: treat-as-air passables with default
// factors and no fluid, labeled provisional until class-2 fact authority
// exists for those states.

const (
	v2168PhysicsOutputPath    = "crates/assets/data/block-physics-v2168.bin"
	v2168PhysicsBREGInputPath = "crates/assets/data/block-registry-v2168.bin"
	v2168PhysicsBREGSHA256    = v2168FoundationBlockSHA256
	v2168PhysicsReservedCount = 969
)

func writeV2168PhysicsProjection(bregPath, pmmpRoot, prismarineRoot, outputPath, shaOutputPath, manifestPath string) error {
	if bregPath == "" || outputPath == "" {
		return errors.New("v2168 physics projection requires a binding BREG and an output path")
	}
	data, err := os.ReadFile(bregPath)
	if err != nil {
		return fmt.Errorf("read v2168 physics-binding BREG: %w", err)
	}
	if len(data) > 128<<20 {
		return errors.New("v2168 physics-binding BREG exceeds 128 MiB")
	}
	if actual := fmt.Sprintf("%x", sha256.Sum256(data)); actual != v2168PhysicsBREGSHA256 {
		return fmt.Errorf("pinned protocol-2168 block registry SHA-256 %s does not match %s", actual, v2168PhysicsBREGSHA256)
	}
	if pmmpRoot == "" || prismarineRoot == "" {
		return errors.New("v2168 physics projection requires the pinned PMMP and Prismarine sources")
	}
	_, records, err := decodeBREGRecords(data, v2168BlockProtocol)
	if err != nil {
		return err
	}
	sources, err := loadPinnedPhysicsSources(pmmpRoot, prismarineRoot, world.DefaultBlockRegistry)
	if err != nil {
		return err
	}
	physics, err := projectV2168PhysicsRecords(records, sources)
	if err != nil {
		return err
	}
	if manifestPath != "" {
		payload, err := os.ReadFile(manifestPath)
		if err != nil {
			return fmt.Errorf("read v2168 physics projection manifest: %w", err)
		}
		if err := crossCheckV2168PhysicsManifest(payload); err != nil {
			return err
		}
	}
	encoded, err := encodePhysicsRegistryForProtocol(data, physics, v2168BlockStateCount, v2168BlockProtocol)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(outputPath), 0o755); err != nil {
		return fmt.Errorf("create v2168 physics output directory: %w", err)
	}
	if err := os.WriteFile(outputPath, encoded, 0o644); err != nil {
		return fmt.Errorf("write v2168 physics output: %w", err)
	}
	if shaOutputPath == "" {
		shaOutputPath = strings.TrimSuffix(outputPath, filepath.Ext(outputPath)) + ".sha256"
	}
	digest := sha256.Sum256(encoded)
	if err := os.WriteFile(shaOutputPath, []byte(fmt.Sprintf("%x\n", digest)), 0o644); err != nil {
		return fmt.Errorf("write v2168 physics checksum: %w", err)
	}
	return nil
}

// projectV2168PhysicsRecords derives one physics record per checked-in v2168
// block state through the shared reviewed pipeline: collision boxes from the
// registry's verbatim seeds, pinned PMMP friction normalized to Q1E8 by name,
// default speed factors, fluid heights from liquid depth, and the same
// reviewed override families cross-checked against the supplied states with
// production coverage required. It fails closed naming every non-reserved name
// that lacks a PMMP row, so future class-2 additions cannot silently inherit
// guessed movement facts.
func projectV2168PhysicsRecords(records []Record, sources PhysicsSourceCatalog) ([]PhysicsRecord, error) {
	if len(records) != v2168BlockStateCount {
		return nil, fmt.Errorf("v2168 physics record count %d does not match %d", len(records), v2168BlockStateCount)
	}
	build := make([]Record, 0, len(records))
	missing := make(map[string]struct{})
	reservedCount := 0
	for index, record := range records {
		if record.SequentialID != uint32(index) {
			return nil, fmt.Errorf("v2168 physics record %d has sequential ID %d", index, record.SequentialID)
		}
		if record.Name == retailReservedName {
			reservedCount++
			continue
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
		return nil, fmt.Errorf("v2168 physics projection fails closed: %d non-reserved names have no pinned PMMP friction row (future class-2 additions require reviewed rows before this projection): %s", len(names), strings.Join(names, ", "))
	}
	if reservedCount != v2168PhysicsReservedCount {
		return nil, fmt.Errorf("v2168 physics projection found %d cinnabar:reserved states, want exactly %d", reservedCount, v2168PhysicsReservedCount)
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

func crossCheckV2168PhysicsManifest(payload []byte) error {
	var manifest v2168BlockProjectionManifest
	decoder := json.NewDecoder(bytes.NewReader(payload))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&manifest); err != nil {
		return fmt.Errorf("decode v2168 physics projection manifest: %w", err)
	}
	if manifest.Projection.DeniedCount != v2168PhysicsReservedCount {
		return fmt.Errorf("v2168 projection manifest denies %d states, want exactly %d", manifest.Projection.DeniedCount, v2168PhysicsReservedCount)
	}
	if manifest.Output.SHA256 != v2168PhysicsBREGSHA256 {
		return fmt.Errorf("v2168 projection manifest binds BREG SHA-256 %q, want %q", manifest.Output.SHA256, v2168PhysicsBREGSHA256)
	}
	return nil
}
