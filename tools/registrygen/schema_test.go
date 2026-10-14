package main

import (
	"bytes"
	"encoding/hex"
	"os"
	"os/exec"
	"strings"
	"testing"
)

func TestSharedRegistrySchemaIsCurrent(t *testing.T) {
	python, err := exec.LookPath("python3")
	if err != nil {
		python, err = exec.LookPath("python")
	}
	if err != nil {
		t.Fatal("Python 3 is required to check the shared registry schema")
	}
	if output, err := exec.Command(python, "generate_schema.py", "--check").CombinedOutput(); err != nil {
		t.Fatalf("regenerate the registry declarations: %v\n%s", err, output)
	}
}

func TestRegistryEncoderMatchesCrossLanguageFixture(t *testing.T) {
	metadata := RegistryMetadata{Protocol: 2193, CanonicalNames: 2, CanonicalStates: 2, ValentineNames: 2, ValentineStates: 2}
	records := []Record{
		{
			BeaconTint: BeaconTintRed, SequentialID: 7, NetworkHash: 0x12345678, Flags: flagCubeGeometry | flagLeafModel,
			Name: "fixture:leaf", StateJSON: []byte(`{"growth":{"type":"int","value":3}}`),
			ModelFamily: ModelFamilyLeaves, ContributorRole: ContributorPrimary,
			ModelState:   ModelState{Mask: 0xff, Values: [8]uint32{1, 2, 3, 4, 5, 6, 7, 8}},
			FaceCoverage: 0x3f, Provenance: allProvenance,
			CollisionSeed: CollisionSeed{ShapeID: 0x1234, Confidence: CollisionConfidenceReviewedVisibleBounds,
				Boxes: []CollisionBox{{MinX: -1, MinY: 0, MinZ: 1, MaxX: 100_000_000, MaxY: 99_999_999, MaxZ: 50_000_000}}},
		},
		{
			BeaconTint: BeaconTintNoTinting, SequentialID: 8, NetworkHash: 0x87654321, Flags: flagCubeGeometry,
			Name: "fixture:resin", StateJSON: []byte(`{}`), ModelFamily: ModelFamilyResinClump,
			ContributorRole: ContributorLiquidAdditional, Provenance: ProvenanceValentine,
			ModelState: ModelState{Mask: 0x80, Values: [8]uint32{0, 0, 0, 0, 0, 0, 0, 0xfeed}},
		},
	}
	got, err := encodeWithMetadata(metadata, records)
	if err != nil {
		t.Fatal(err)
	}
	text, err := os.ReadFile("../../crates/assets/tests/fixtures/registry-wire.hex")
	if err != nil {
		t.Fatal(err)
	}
	want, err := hex.DecodeString(strings.Join(strings.Fields(string(text)), ""))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("registry wire contract changed:\n got %x\nwant %x", got, want)
	}
}

// TestActiveBlockFactsWireRoundTrip checks nonzero compiled masks against the real pinned palette.
func TestActiveBlockFactsWireRoundTrip(t *testing.T) {
	data, err := os.ReadFile("../../crates/assets/data/block-registry-v2193.bin")
	if err != nil {
		t.Fatal(err)
	}
	metadata, records, err := decodeBREGRecords(data, v2193BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	found := false
	for _, record := range records {
		found = found || record.Traits != 0
	}
	if !found {
		t.Fatal("active palette has no compiled block traits")
	}
	encoded, err := encodeWithMetadata(metadata, records)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(data, encoded) {
		t.Fatal("compiled block facts changed during a wire round trip")
	}
}
