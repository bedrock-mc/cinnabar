package main

import (
	"bytes"
	"encoding/binary"
	"testing"
)

func TestFilterFallbackInventoryPreservesUnselectedEntries(t *testing.T) {
	input := append([]byte("CVFB1001"), make([]byte, 8)...)
	binary.LittleEndian.PutUint32(input[8:12], 1)
	binary.LittleEndian.PutUint32(input[12:16], 3)
	for _, hash := range []uint32{10, 20, 30} {
		entry := make([]byte, fallbackEntryBytes)
		binary.LittleEndian.PutUint32(entry, hash)
		for index := 4; index < len(entry); index++ {
			entry[index] = byte(hash + uint32(index))
		}
		input = append(input, entry...)
	}
	filtered, err := filterFallbackInventory(input, map[uint32]struct{}{20: {}})
	if err != nil {
		t.Fatal(err)
	}
	if binary.LittleEndian.Uint32(filtered[12:16]) != 2 {
		t.Fatalf("filtered count = %d", binary.LittleEndian.Uint32(filtered[12:16]))
	}
	want := append(append([]byte(nil), input[:fallbackHeaderBytes+fallbackEntryBytes]...), input[fallbackHeaderBytes+2*fallbackEntryBytes:]...)
	binary.LittleEndian.PutUint32(want[12:16], 2)
	if !bytes.Equal(filtered, want) {
		t.Fatal("unselected entries were not preserved byte-for-byte")
	}
}

func TestSelectedFallbackHashesRejectCollisionWithUnselectedRecord(t *testing.T) {
	records := make([]bregLightIdentity, physicsRecordCount)
	for id := range records {
		records[id] = bregLightIdentity{SequentialID: uint32(id), NetworkHash: uint32(id + 100)}
		if isRetailReservedSequentialID(uint32(id)) {
			records[id].Name = retailReservedName
			records[id].StateJSON = reservedStateJSON(uint32(id))
		}
	}
	records[0].NetworkHash = records[1].NetworkHash
	if _, err := selectedFallbackHashes(records); err == nil {
		t.Fatal("selected/unselected network-hash collision was accepted")
	}
}
