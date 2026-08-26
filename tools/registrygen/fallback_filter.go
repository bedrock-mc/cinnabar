package main

import (
	"encoding/binary"
	"fmt"
	"os"
	"path/filepath"
)

const (
	fallbackHeaderBytes = 16
	fallbackEntryBytes  = 25
)

func writeFilteredFallback(inputPath, outputPath, bregPath string) error {
	input, err := os.ReadFile(inputPath)
	if err != nil {
		return fmt.Errorf("read fallback inventory: %w", err)
	}
	breg, err := os.ReadFile(bregPath)
	if err != nil {
		return fmt.Errorf("read fallback BREG: %w", err)
	}
	records, err := readBREG1003LightIdentities(breg)
	if err != nil {
		return err
	}
	selected, err := selectedFallbackHashes(records)
	if err != nil {
		return err
	}
	filtered, err := filterFallbackInventory(input, selected)
	if err != nil {
		return err
	}
	if err := os.MkdirAll(filepath.Dir(outputPath), 0o755); err != nil {
		return fmt.Errorf("create fallback output directory: %w", err)
	}
	if err := os.WriteFile(outputPath, filtered, 0o644); err != nil {
		return fmt.Errorf("write fallback inventory: %w", err)
	}
	return nil
}

// parseFallbackInventoryHeader validates the exact CVFB1001 table envelope
// shared by every fallback-inventory mode and returns its entry count. A zero
// count fails closed because the real producer never emits an empty table.
func parseFallbackInventoryHeader(input []byte) (int, error) {
	if len(input) < fallbackHeaderBytes || string(input[:8]) != "CVFB1001" || binary.LittleEndian.Uint32(input[8:12]) != 1 {
		return 0, fmt.Errorf("fallback inventory has an invalid header")
	}
	count := int(binary.LittleEndian.Uint32(input[12:16]))
	if count == 0 {
		return 0, fmt.Errorf("fallback inventory declares zero entries")
	}
	if len(input) != fallbackHeaderBytes+count*fallbackEntryBytes {
		return 0, fmt.Errorf("fallback inventory length does not match %d entries", count)
	}
	return count, nil
}

func selectedFallbackHashes(records []bregLightIdentity) (map[uint32]struct{}, error) {
	owners := make(map[uint32]uint32, len(records))
	selected := make(map[uint32]struct{}, retailReservedCount)
	for _, record := range records {
		if owner, exists := owners[record.NetworkHash]; exists {
			return nil, fmt.Errorf("fallback BREG hash %#x is shared by records %d and %d", record.NetworkHash, owner, record.SequentialID)
		}
		owners[record.NetworkHash] = record.SequentialID
		if !isRetailReservedSequentialID(record.SequentialID) {
			continue
		}
		if record.Name != retailReservedName || string(record.StateJSON) != string(reservedStateJSON(record.SequentialID)) {
			return nil, fmt.Errorf("fallback BREG record %d is not projected", record.SequentialID)
		}
		selected[record.NetworkHash] = struct{}{}
	}
	if len(selected) != retailReservedCount {
		return nil, fmt.Errorf("fallback BREG contains %d selected hashes, want %d", len(selected), retailReservedCount)
	}
	return selected, nil
}

func filterFallbackInventory(input []byte, excluded map[uint32]struct{}) ([]byte, error) {
	count, err := parseFallbackInventoryHeader(input)
	if err != nil {
		return nil, err
	}
	output := append([]byte(nil), input[:fallbackHeaderBytes]...)
	removed := 0
	for index := 0; index < count; index++ {
		start := fallbackHeaderBytes + index*fallbackEntryBytes
		entry := input[start : start+fallbackEntryBytes]
		hash := binary.LittleEndian.Uint32(entry[:4])
		if _, drop := excluded[hash]; drop {
			removed++
			continue
		}
		output = append(output, entry...)
	}
	binary.LittleEndian.PutUint32(output[12:16], uint32(count-removed))
	return output, nil
}
