package main

import (
	"bytes"
	"os"
	"slices"
	"strings"
	"testing"
)

var expectedExplicitNBTIDs = []string{
	"Banner",
	"Barrel",
	"Beacon",
	"Bed",
	"BlastFurnace",
	"BrewingStand",
	"Campfire",
	"Chest",
	"CopperGolemStatue",
	"DecoratedPot",
	"EnchantTable",
	"EnderChest",
	"Furnace",
	"GlowItemFrame",
	"Hopper",
	"ItemFrame",
	"Jukebox",
	"Lectern",
	"Sign",
	"Skull",
	"Smoker",
}

func TestCollectPinnedInventoryCoversExactAuditedDragonflyProducers(t *testing.T) {
	source, err := collectPinnedInventory()
	if err != nil {
		t.Fatalf("collect pinned inventory: %v", err)
	}
	if source.Schema != sourceSchema || source.ProtocolVersion != 1001 {
		t.Fatalf("schema/protocol = %d/%d", source.Schema, source.ProtocolVersion)
	}
	if source.GameVersion != "1.26.30" || source.BDSServerVersion != "1.26.32.2" {
		t.Fatalf("game/BDS versions = %q/%q", source.GameVersion, source.BDSServerVersion)
	}
	if source.Dragonfly.Module != dragonflyModule || source.Dragonfly.Version != dragonflyVersion || source.Dragonfly.Revision != dragonflyRevision {
		t.Fatalf("Dragonfly pin = %#v", source.Dragonfly)
	}
	if source.BDS.ExecutableSHA256 != bdsExecutableSHA256 {
		t.Fatalf("BDS executable hash = %q", source.BDS.ExecutableSHA256)
	}
	if len(source.Entries) != 22 {
		t.Fatalf("source entries = %d, want 22", len(source.Entries))
	}

	var explicit []string
	var idless []sourceEntry
	backingBlockNames, backingStates := 0, 0
	for _, entry := range source.Entries {
		if len(entry.BackingBlocks) == 0 {
			t.Fatalf("%q has no authoritative backing blocks", entry.SourceKey)
		}
		if entry.NBTID == nil {
			idless = append(idless, entry)
		} else {
			explicit = append(explicit, *entry.NBTID)
		}
		backingBlockNames += len(entry.BackingBlocks)
		for _, block := range entry.BackingBlocks {
			backingStates += len(block.States)
		}
	}
	if !slices.Equal(explicit, expectedExplicitNBTIDs) {
		t.Fatalf("explicit IDs = %#v", explicit)
	}
	if len(idless) != 1 || idless[0].SourceKey != "Note" || idless[0].BackingBlocks[0].Name != "minecraft:noteblock" {
		t.Fatalf("id-less producers = %#v", idless)
	}
	if backingBlockNames != 63 || backingStates != 446 {
		t.Fatalf("authoritative backing inventory = %d names/%d states, want 63/446", backingBlockNames, backingStates)
	}
	if len(source.Dragonfly.RegistrationSHA256) != 64 {
		t.Fatalf("registration hash = %q", source.Dragonfly.RegistrationSHA256)
	}
}

func TestReviewedManifestJoinsEverySourceExactlyOnce(t *testing.T) {
	source := mustCollectSource(t)
	manifest := mustReadManifest(t)
	artifact, report, err := joinInventory(source, manifest, joinReviewed)
	if err != nil {
		t.Fatalf("reviewed join: %v", err)
	}
	if len(artifact.Entries) != 22 || report.SourceCount != 22 || report.ManifestCount != 22 || report.JoinedCount != 22 {
		t.Fatalf("join counts = artifact %d, report %#v", len(artifact.Entries), report)
	}
	if artifact.CanonicalBlockStateCounted {
		t.Fatal("block entities were folded into the canonical block-state count")
	}
	for _, entry := range artifact.Entries {
		if entry.Source.DragonflyVersion != dragonflyVersion || entry.Source.DragonflyRegistrationSHA256 != dragonflyRegistrationSHA256 || entry.Source.DragonflySourceFileSHA256 == "" || entry.Source.BDSExecutableSHA256 != bdsExecutableSHA256 {
			t.Fatalf("%q has incomplete per-entry source provenance: %#v", entry.SourceKey, entry.Source)
		}
	}
	if report.ExplicitNBTIDCount != 21 || report.IDLessProducerCount != 1 {
		t.Fatalf("source kind counts = %d/%d", report.ExplicitNBTIDCount, report.IDLessProducerCount)
	}
	if report.ProvenRendererCount != 0 || report.FinalGatePassed {
		t.Fatalf("deferred renderers counted as proven: %#v", report)
	}
	if len(report.FinalBlockers) == 0 {
		t.Fatal("reviewed join must report strict-final blockers")
	}
}

func TestReviewedJoinRejectsMissingExtraAmbiguousAndDriftedInputs(t *testing.T) {
	source := mustCollectSource(t)
	valid := mustReadManifest(t)
	tests := []struct {
		name string
		edit func(*rendererManifest)
		want string
	}{
		{"missing", func(m *rendererManifest) { m.Entries = m.Entries[1:] }, "missing source key"},
		{"extra", func(m *rendererManifest) { m.Entries = append(m.Entries, rendererEntry{SourceKey: "Unregistered"}) }, "manifest-only source key"},
		{"duplicate source", func(m *rendererManifest) { m.Entries[1].SourceKey = m.Entries[0].SourceKey }, "duplicate renderer source key"},
		{"canonical alias", func(m *rendererManifest) { m.Entries[1].NBTAliases = []string{expectedExplicitNBTIDs[0]} }, "ambiguous NBT alias"},
		{"duplicate alias", func(m *rendererManifest) {
			m.Entries[0].NBTAliases = []string{"LegacyShared"}
			m.Entries[1].NBTAliases = []string{"LegacyShared"}
		}, "ambiguous NBT alias"},
		{"missing chunk path", func(m *rendererManifest) {
			m.Entries[0].ChunkNBT.Supported = false
		}, "missing chunk-NBT support or witness"},
		{"missing update witness", func(m *rendererManifest) {
			m.Entries[0].LiveUpdate.WitnessIDs = nil
		}, "missing live-update support or witness"},
		{"missing variants", func(m *rendererManifest) {
			m.Entries[0].RequiredNBTVariants = nil
		}, "missing required NBT variants"},
		{"Dragonfly hash", func(m *rendererManifest) { m.DragonflyRegistrationSHA256 = strings.Repeat("0", 64) }, "Dragonfly registration hash drift"},
		{"BDS hash", func(m *rendererManifest) { m.BDSExecutableSHA256 = strings.Repeat("f", 64) }, "BDS executable hash drift"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			manifest := valid
			manifest.Entries = slices.Clone(valid.Entries)
			test.edit(&manifest)
			_, _, err := joinInventory(source, manifest, joinReviewed)
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want %q", err, test.want)
			}
		})
	}
}

func TestSourcePinTamperingFailsBeforeTheJoin(t *testing.T) {
	source := mustCollectSource(t)
	manifest := mustReadManifest(t)
	source.Dragonfly.RegistrationSHA256 = strings.Repeat("a", 64)
	_, _, err := joinInventory(source, manifest, joinReviewed)
	if err == nil || !strings.Contains(err.Error(), "source Dragonfly pin drift") {
		t.Fatalf("source tamper error = %v", err)
	}
}

func TestRendererManifestDecoderRejectsOversizedAndTrailingPayloads(t *testing.T) {
	oversized := bytes.Repeat([]byte{' '}, maxRendererManifestBytes+1)
	if _, err := decodeRendererManifest(oversized); err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("oversized manifest error = %v", err)
	}
	raw, err := os.ReadFile("../../assets/block-entity-renderers-v1001.json")
	if err != nil {
		t.Fatalf("read renderer manifest: %v", err)
	}
	if _, err := decodeRendererManifest(append(raw, []byte("{}")...)); err == nil || !strings.Contains(err.Error(), "trailing") {
		t.Fatalf("trailing manifest error = %v", err)
	}
}

func TestStrictFinalFailsClosedOnEveryDeferredOrMissingEvidenceClass(t *testing.T) {
	source := mustCollectSource(t)
	manifest := mustReadManifest(t)
	_, report, err := joinInventory(source, manifest, joinStrictFinal)
	if err == nil {
		t.Fatal("strict-final join accepted deferred renderer manifest")
	}
	for _, want := range []string{"renderer status", "implementation symbol", "gallery builder", "NBT variant witness", "GPU/no-draw witness"} {
		if !strings.Contains(err.Error(), want) {
			t.Fatalf("strict-final error %q does not contain %q", err, want)
		}
	}
	if report.ProvenRendererCount != 0 || report.FinalGatePassed {
		t.Fatalf("strict-final report = %#v", report)
	}
}

func TestGeneratedInventoryAndReportMatchDeterministicGoldens(t *testing.T) {
	source := mustCollectSource(t)
	manifest := mustReadManifest(t)
	artifact, report, err := joinInventory(source, manifest, joinReviewed)
	if err != nil {
		t.Fatalf("join reviewed manifest: %v", err)
	}
	inventoryJSON, reportJSON, err := encodeArtifacts(artifact, report)
	if err != nil {
		t.Fatalf("encode artifacts: %v", err)
	}
	secondInventory, secondReport, err := encodeArtifacts(artifact, report)
	if err != nil {
		t.Fatalf("repeat encode: %v", err)
	}
	if !bytes.Equal(inventoryJSON, secondInventory) || !bytes.Equal(reportJSON, secondReport) {
		t.Fatal("artifact encoding is nondeterministic")
	}
	assertGolden(t, "testdata/block-entities-v1001.json", inventoryJSON)
	assertGolden(t, "testdata/block-entity-coverage-v1001-report.json", reportJSON)
}

func mustCollectSource(t *testing.T) sourceInventory {
	t.Helper()
	source, err := collectPinnedInventory()
	if err != nil {
		t.Fatalf("collect source: %v", err)
	}
	return source
}

func mustReadManifest(t *testing.T) rendererManifest {
	t.Helper()
	raw, err := os.ReadFile("../../assets/block-entity-renderers-v1001.json")
	if err != nil {
		t.Fatalf("read renderer manifest: %v", err)
	}
	manifest, err := decodeRendererManifest(raw)
	if err != nil {
		t.Fatalf("decode renderer manifest: %v", err)
	}
	return manifest
}

func assertGolden(t *testing.T, path string, got []byte) {
	t.Helper()
	want, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read golden %s: %v", path, err)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("%s differs from deterministic golden", path)
	}
}
