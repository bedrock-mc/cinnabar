package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"

	"github.com/hashimthearab/rust-mcbe/tools/registrygen/internal/targetpin"
)

// The active physics carrier combines shared state facts with client movement rules.

const (
	v2193PhysicsOutputPath    = "crates/assets/data/block-physics-v2193.bin"
	v2193PhysicsBREGInputPath = "crates/assets/data/block-registry-v2193.bin"
	v2193PhysicsReservedCount = 662 - v2193EducationStateCount
)

// writeV2193PhysicsProjection binds shared physics facts to the selected block carrier.
func writeV2193PhysicsProjection(bregPath, outputPath, shaOutputPath, manifestPath string) error {
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
	_, records, err := decodeBREGRecords(data, v2193BlockProtocol)
	if err != nil {
		return err
	}
	physics, err := projectV2193PhysicsRecords(records)
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

// projectV2193PhysicsRecords uses exact shared state friction and collisions, with local movement behavior.
func projectV2193PhysicsRecords(records []Record) ([]PhysicsRecord, error) {
	if err := validateSharedTarget(); err != nil {
		return nil, err
	}
	if len(records) != v2193BlockStateCount {
		return nil, fmt.Errorf("v2193 physics record count %d does not match %d", len(records), v2193BlockStateCount)
	}
	physics := make([]PhysicsRecord, len(records))
	counts := make(map[string]int)
	reserved := 0
	for i, record := range records {
		if record.SequentialID != uint32(i) {
			return nil, fmt.Errorf("v2193 physics record %d has sequential ID %d", i, record.SequentialID)
		}
		entry := PhysicsRecord{SequentialID: record.SequentialID, NetworkHash: record.NetworkHash, FrictionQ1E8: defaultFrictionQ1E8, HorizontalSpeedQ1E8: defaultSpeedQ1E8, VerticalSpeedQ1E8: defaultSpeedQ1E8}
		if record.Name == retailReservedName {
			reserved++
			entry.Flags = physicsFlagPassable
			physics[i] = entry
			continue
		}
		counts[record.Name]++
		properties, err := sharedBlockProperties(record)
		if err != nil {
			return nil, err
		}
		seed, err := sharedCollisionSeed(record, properties)
		if err != nil {
			return nil, err
		}
		entry.Boxes = seed.Boxes
		entry.FrictionQ1E8, err = fixedPhysicsScalar(float64(properties.Friction))
		if err != nil {
			return nil, fmt.Errorf("shared friction for %s: %w", record.Name, err)
		}
		if len(entry.Boxes) == 0 {
			entry.Flags |= physicsFlagPassable
		}
		if override, ok := reviewedPhysicsOverrideFor(record.Name); ok {
			if err := applyPhysicsOverride(record, override, &entry); err != nil {
				return nil, err
			}
		}
		physics[i] = entry
	}
	if reserved != v2193PhysicsReservedCount {
		return nil, fmt.Errorf("v2193 physics projection found %d cinnabar:reserved states, want exactly %d", reserved, v2193PhysicsReservedCount)
	}
	for _, override := range reviewedPhysicsOverrides {
		if counts[override.Name] != override.StateCount {
			return nil, fmt.Errorf("reviewed physics override %s has %d states, want %d", override.Name, counts[override.Name], override.StateCount)
		}
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
