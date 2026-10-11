package main

import (
	"testing"
)

// TestPhysicsSeedCarriesExplicitMovementSupport keeps typed movement facts in compiler inputs.
func TestPhysicsSeedCarriesExplicitMovementSupport(t *testing.T) {
	records := []PhysicsRecord{{SequentialID: 0, NetworkHash: 7, FrictionQ1E8: defaultFrictionQ1E8, HorizontalSpeedQ1E8: defaultSpeedQ1E8, VerticalSpeedQ1E8: defaultSpeedQ1E8, MovementSemantics: MovementSemanticsSpecial}}
	blob, err := encodePhysicsRegistryForProtocol([]byte("identity fixture"), records, len(records), v2193BlockProtocol)
	if err != nil {
		t.Fatal(err)
	}
	if string(blob[:8]) != physicsRegistryHeader {
		t.Fatalf("unexpected seed format: %q", blob[:8])
	}
	if got := MovementSemantics(blob[physicsRegistryHeaderBytes+physicsMovementSemanticsOffset]); got != MovementSemanticsSpecial {
		t.Fatalf("seed movement support = %v, want special", got)
	}
}

// TestMovementSupportRejectsUnknownDiscriminants keeps unsupported data out of current carriers.
func TestMovementSupportRejectsUnknownDiscriminants(t *testing.T) {
	record := PhysicsRecord{SequentialID: 0, NetworkHash: 7, FrictionQ1E8: defaultFrictionQ1E8, HorizontalSpeedQ1E8: defaultSpeedQ1E8, VerticalSpeedQ1E8: defaultSpeedQ1E8, MovementSemantics: maxMovementSemantics + 1}
	if _, err := encodePhysicsRegistryForProtocol([]byte("identity fixture"), []PhysicsRecord{record}, 1, v2193BlockProtocol); err == nil {
		t.Fatal("unknown movement support emitted as a valid record")
	}
}
