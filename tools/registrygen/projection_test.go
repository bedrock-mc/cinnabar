package main

import (
	"bytes"
	"testing"
)

func TestRetailReservedManifestIsSortedDisjointAndExact(t *testing.T) {
	count := 0
	var previous uint32
	for index, span := range retailReservedRanges {
		if span.first > span.last {
			t.Fatalf("range %d is inverted: %d..%d", index, span.first, span.last)
		}
		if index != 0 && span.first <= previous {
			t.Fatalf("range %d starts at %d after %d", index, span.first, previous)
		}
		count += int(span.last-span.first) + 1
		previous = span.last
	}
	if count != retailReservedCount {
		t.Fatalf("manifest contains %d IDs, want %d", count, retailReservedCount)
	}
	for id := uint32(0); id < physicsRecordCount; id++ {
		found := isRetailReservedSequentialID(id)
		listed := false
		for _, span := range retailReservedRanges {
			listed = listed || id >= span.first && id <= span.last
		}
		if found != listed {
			t.Fatalf("membership mismatch for ID %d", id)
		}
	}
}

func TestRetailProjectionIsIdempotentAndPreservesIdentities(t *testing.T) {
	records := make([]Record, physicsRecordCount)
	for id := range records {
		records[id] = Record{
			SequentialID: uint32(id), NetworkHash: uint32(id*17 + 3), Flags: 0xff,
			Name: "minecraft:test", StateJSON: []byte(`{"test":{"type":"int","value":1}}`),
			ModelFamily: ModelFamilyCube, ContributorRole: ContributorLiquidAdditional,
			ModelState: ModelState{Mask: 1, Values: [8]uint32{42}}, FaceCoverage: 0x3f,
			CollisionSeed: CollisionSeed{ShapeID: 2, Confidence: CollisionConfidenceCollisionOnly, Boxes: []CollisionBox{{MaxX: 1}}},
			Provenance:    ProvenancePMMP | ProvenanceValentine,
		}
	}
	projected, err := projectRetailRegistry(records)
	if err != nil {
		t.Fatal(err)
	}
	if err := validateRetailProjection(records, projected); err != nil {
		t.Fatal(err)
	}
	again, err := projectRetailRegistry(projected)
	if err != nil {
		t.Fatal(err)
	}
	for index := range projected {
		if !recordsEqual(projected[index], again[index]) {
			t.Fatalf("projection is not idempotent at ID %d", index)
		}
		if projected[index].SequentialID != records[index].SequentialID || projected[index].NetworkHash != records[index].NetworkHash {
			t.Fatalf("identity changed at ID %d", index)
		}
		if !isRetailReservedSequentialID(uint32(index)) && !recordsEqual(projected[index], records[index]) {
			t.Fatalf("unselected record changed at ID %d", index)
		}
	}
	if got := projected[1].StateJSON; !bytes.Equal(got, []byte(`{"reserved_id":{"type":"int","value":1}}`)) {
		t.Fatalf("unexpected reserved state: %s", got)
	}
}

func TestReservedLightAndPhysicsAreNeutral(t *testing.T) {
	lights := bytes.Repeat([]byte{0xff}, physicsRecordCount)
	if err := neutralizeReservedLightProperties(lights); err != nil {
		t.Fatal(err)
	}
	physics := make([]PhysicsRecord, physicsRecordCount)
	for id := range physics {
		physics[id] = PhysicsRecord{
			SequentialID: uint32(id), NetworkHash: uint32(id + 1), Boxes: []CollisionBox{{MaxX: 1}},
			FrictionQ1E8: 1, HorizontalSpeedQ1E8: 2, VerticalSpeedQ1E8: 3, FluidHeightQ1E8: 4,
			Flags: physicsFlagWater, SurfaceResponse: SurfaceBubbleUp,
		}
	}
	if err := neutralizeReservedPhysics(physics); err != nil {
		t.Fatal(err)
	}
	for id := range physics {
		selected := isRetailReservedSequentialID(uint32(id))
		if selected && lights[id] != 0 {
			t.Fatalf("selected light %d is %#x", id, lights[id])
		}
		if !selected && lights[id] != 0xff {
			t.Fatalf("unselected light %d changed", id)
		}
		if !selected {
			continue
		}
		record := physics[id]
		if len(record.Boxes) != 0 || record.Flags != physicsFlagPassable || record.FrictionQ1E8 != defaultSpeedQ1E8 ||
			record.HorizontalSpeedQ1E8 != defaultSpeedQ1E8 || record.VerticalSpeedQ1E8 != defaultSpeedQ1E8 ||
			record.FluidHeightQ1E8 != 0 || record.SurfaceResponse != SurfaceNone {
			t.Fatalf("selected physics %d is not neutral: %+v", id, record)
		}
	}
}
