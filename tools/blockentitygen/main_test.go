package main

import (
	"bytes"
	"crypto/sha256"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
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

func TestReadRendererManifestRejectsOversizedFileBeforeDecoding(t *testing.T) {
	path := filepath.Join(t.TempDir(), "renderer-manifest.json")
	file, err := os.Create(path)
	if err != nil {
		t.Fatalf("create oversized manifest: %v", err)
	}
	if err := file.Truncate(maxRendererManifestBytes + 1); err != nil {
		file.Close()
		t.Fatalf("truncate oversized manifest: %v", err)
	}
	if err := file.Close(); err != nil {
		t.Fatalf("close oversized manifest: %v", err)
	}
	if _, err := readRendererManifest(path); err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("oversized manifest error = %v", err)
	}
}

func TestVerifyFileSHA256StreamsWithinBoundedAllocation(t *testing.T) {
	path := filepath.Join(t.TempDir(), "pinned.bin")
	expectedSize := int64(4 << 20)
	expectedHash := writeHashedFixture(t, path, expectedSize)
	if err := verifyFileSHA256(path, expectedSize, expectedHash); err != nil {
		t.Fatalf("verify streamed fixture: %v", err)
	}

	var benchmarkErr error
	result := testing.Benchmark(func(b *testing.B) {
		b.ReportAllocs()
		for range b.N {
			if err := verifyFileSHA256(path, expectedSize, expectedHash); err != nil {
				benchmarkErr = err
				return
			}
		}
	})
	if benchmarkErr != nil {
		t.Fatalf("benchmark streamed fixture: %v", benchmarkErr)
	}
	if allocated := result.AllocedBytesPerOp(); allocated > 256<<10 {
		t.Fatalf("verify allocation = %d bytes/op, want at most %d", allocated, 256<<10)
	}
}

func TestVerifyFileSHA256RejectsSizeBeforeHashing(t *testing.T) {
	path := filepath.Join(t.TempDir(), "wrong-size.bin")
	if err := os.WriteFile(path, []byte("not the expected size"), 0o600); err != nil {
		t.Fatalf("write wrong-size fixture: %v", err)
	}
	if err := verifyFileSHA256(path, 1, strings.Repeat("0", 64)); err == nil || !strings.Contains(err.Error(), "size drift") {
		t.Fatalf("size error = %v", err)
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

func TestStrictFinalAcceptsOnlyCompleteConsistentEvidence(t *testing.T) {
	source := mustCollectSource(t)
	manifest := mustStrictFinalManifest(t)
	_, report, err := joinInventory(source, manifest, joinStrictFinal)
	if err != nil {
		t.Fatalf("strict-final join: %v", err)
	}
	if !report.FinalGatePassed || report.ProvenRendererCount != 22 || len(report.FinalBlockers) != 0 {
		t.Fatalf("strict-final report = %#v", report)
	}
}

func TestReviewedJoinRejectsIncompleteMalformedAndContradictoryDeclarations(t *testing.T) {
	source := mustCollectSource(t)
	valid := mustReadManifest(t)
	tests := []struct {
		name string
		edit func(*rendererManifest)
		want string
	}{
		{"deleted but nonempty variants", func(m *rendererManifest) {
			m.Entries[0].RequiredNBTVariants = m.Entries[0].RequiredNBTVariants[1:]
		}, "required NBT variant set mismatch"},
		{"extra variant", func(m *rendererManifest) {
			m.Entries[0].RequiredNBTVariants = append(m.Entries[0].RequiredNBTVariants, nbtVariant{VariantID: "invented", RequiredFields: []string{"id"}, WitnessIDs: []string{}})
		}, "required NBT variant set mismatch"},
		{"whitespace variant identifier", func(m *rendererManifest) {
			m.Entries[0].RequiredNBTVariants[0].VariantID = " "
		}, "invalid required NBT variant identifier"},
		{"whitespace required field", func(m *rendererManifest) {
			m.Entries[0].RequiredNBTVariants[0].RequiredFields[0] = "\t"
		}, "invalid required NBT field"},
		{"duplicate required field", func(m *rendererManifest) {
			fields := m.Entries[0].RequiredNBTVariants[0].RequiredFields
			m.Entries[0].RequiredNBTVariants[0].RequiredFields = append(fields, fields[0])
		}, "duplicate required NBT field"},
		{"missing required field", func(m *rendererManifest) {
			fields := m.Entries[0].RequiredNBTVariants[0].RequiredFields
			m.Entries[0].RequiredNBTVariants[0].RequiredFields = fields[:len(fields)-1]
		}, "required NBT field set mismatch"},
		{"extra required field", func(m *rendererManifest) {
			m.Entries[0].RequiredNBTVariants[0].RequiredFields = append(m.Entries[0].RequiredNBTVariants[0].RequiredFields, "InventedField")
		}, "required NBT field set mismatch"},
		{"whitespace alias", func(m *rendererManifest) {
			m.Entries[0].NBTAliases = []string{" "}
		}, "invalid NBT alias"},
		{"whitespace chunk witness", func(m *rendererManifest) {
			m.Entries[0].ChunkNBT.WitnessIDs = []string{" "}
		}, "invalid chunk-NBT witness"},
		{"duplicate chunk witness", func(m *rendererManifest) {
			witness := m.Entries[0].ChunkNBT.WitnessIDs[0]
			m.Entries[0].ChunkNBT.WitnessIDs = []string{witness, witness}
		}, "duplicate chunk-NBT witness"},
		{"whitespace live witness", func(m *rendererManifest) {
			m.Entries[0].LiveUpdate.WitnessIDs = []string{" "}
		}, "invalid live-update witness"},
		{"deferred implementation claim", func(m *rendererManifest) {
			m.Entries[0].ImplementationSymbol = stringPointer("render::banner")
		}, "deferred renderer claims implementation or evidence"},
		{"deferred gallery claim", func(m *rendererManifest) {
			m.Entries[0].GalleryBuilder = stringPointer("gallery::banner")
		}, "deferred renderer claims implementation or evidence"},
		{"deferred variant evidence", func(m *rendererManifest) {
			m.Entries[0].RequiredNBTVariants[0].WitnessIDs = []string{"variant::banner::blank"}
		}, "deferred renderer claims implementation or evidence"},
		{"deferred GPU evidence", func(m *rendererManifest) {
			m.Entries[0].Witnesses.GPU = []string{"gpu::banner"}
		}, "deferred renderer claims implementation or evidence"},
		{"deferred no-draw evidence", func(m *rendererManifest) {
			m.Entries[0].Witnesses.NoDraw = []string{"no_draw::banner"}
		}, "deferred renderer claims implementation or evidence"},
		{"unsupported evidence claim", func(m *rendererManifest) {
			m.Entries[0].RendererStatus = "unsupported"
			m.Entries[0].GalleryBuilder = stringPointer("gallery::banner")
		}, "unsupported renderer claims implementation or evidence"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			manifest := cloneManifest(t, valid)
			test.edit(&manifest)
			_, _, err := joinInventory(source, manifest, joinReviewed)
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want %q", err, test.want)
			}
		})
	}
}

func TestCollectPinnedInventoryRejectsActualSourceFileHashDrift(t *testing.T) {
	sourceRoot := mustResolveModuleRoot(t)
	fixtureRoot := t.TempDir()
	seen := map[string]bool{}
	for _, pin := range sourceFilePins {
		if seen[pin.File] {
			continue
		}
		seen[pin.File] = true
		raw, err := os.ReadFile(filepath.Join(sourceRoot, filepath.FromSlash(pin.File)))
		if err != nil {
			t.Fatalf("read pinned source %s: %v", pin.File, err)
		}
		if pin.File == "server/block/banner.go" {
			raw = append(raw, []byte("\n// source hash drift fixture\n")...)
		}
		destination := filepath.Join(fixtureRoot, filepath.FromSlash(pin.File))
		if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
			t.Fatalf("create pinned source directory: %v", err)
		}
		if err := os.WriteFile(destination, raw, 0o600); err != nil {
			t.Fatalf("write pinned source fixture: %v", err)
		}
	}

	_, err := collectPinnedInventoryFromModuleRoot(fixtureRoot)
	if err == nil || !strings.Contains(err.Error(), "Dragonfly source hash drift for Banner") {
		t.Fatalf("source drift error = %v", err)
	}
}

func TestStrictFinalRejectsMalformedDuplicateAndContradictoryEvidence(t *testing.T) {
	source := mustCollectSource(t)
	valid := mustStrictFinalManifest(t)
	tests := []struct {
		name string
		edit func(*rendererManifest)
		want string
	}{
		{"whitespace implementation symbol", func(m *rendererManifest) {
			m.Entries[0].ImplementationSymbol = stringPointer(" ")
		}, "invalid implementation symbol"},
		{"whitespace gallery builder", func(m *rendererManifest) {
			m.Entries[0].GalleryBuilder = stringPointer("\t")
		}, "invalid gallery builder"},
		{"whitespace variant witness", func(m *rendererManifest) {
			m.Entries[0].RequiredNBTVariants[0].WitnessIDs = []string{" "}
		}, "invalid NBT variant witness"},
		{"duplicate variant witness", func(m *rendererManifest) {
			witness := m.Entries[0].RequiredNBTVariants[0].WitnessIDs[0]
			m.Entries[0].RequiredNBTVariants[0].WitnessIDs = []string{witness, witness}
		}, "duplicate NBT variant witness"},
		{"whitespace GPU witness", func(m *rendererManifest) {
			m.Entries[0].Witnesses.GPU = []string{" "}
		}, "invalid GPU witness"},
		{"duplicate GPU witness", func(m *rendererManifest) {
			witness := m.Entries[0].Witnesses.GPU[0]
			m.Entries[0].Witnesses.GPU = []string{witness, witness}
		}, "duplicate GPU witness"},
		{"drawable no-draw contradiction", func(m *rendererManifest) {
			m.Entries[0].Witnesses.NoDraw = []string{"no_draw::banner"}
		}, "drawable renderer claims no-draw evidence"},
		{"invisible GPU contradiction", func(m *rendererManifest) {
			m.Entries[16].Witnesses.GPU = []string{"gpu::jukebox"}
		}, "invisible renderer claims GPU evidence"},
		{"whitespace no-draw witness", func(m *rendererManifest) {
			m.Entries[16].Witnesses.NoDraw = []string{" "}
		}, "invalid no-draw witness"},
		{"duplicate no-draw witness", func(m *rendererManifest) {
			witness := m.Entries[16].Witnesses.NoDraw[0]
			m.Entries[16].Witnesses.NoDraw = []string{witness, witness}
		}, "duplicate no-draw witness"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			manifest := cloneManifest(t, valid)
			test.edit(&manifest)
			_, _, err := joinInventory(source, manifest, joinStrictFinal)
			if err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want %q", err, test.want)
			}
		})
	}
}

func TestGeneratedInventoryAndReportMatchDeterministicGoldens(t *testing.T) {
	source := mustCollectSource(t)
	manifest := mustReadManifest(t)
	catalog := mustReadEvidenceCatalog(t)
	sourceDigest, err := sourceContractSHA256(source)
	if err != nil {
		t.Fatalf("source contract: %v", err)
	}
	rendererDigest, err := rendererContractSHA256(manifest)
	if err != nil {
		t.Fatalf("renderer contract: %v", err)
	}
	manifest, err = joinEvidence(manifest, catalog, evidenceIdentities{
		SourceContractSHA256: sourceDigest, RendererContractSHA256: rendererDigest,
		Targets: map[string]evidenceTargetIdentity{},
	})
	if err != nil {
		t.Fatalf("join evidence catalog: %v", err)
	}
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

func mustReadEvidenceCatalog(t *testing.T) evidenceCatalog {
	t.Helper()
	catalog, err := readEvidenceCatalog("../../docs/evidence/block-entity-render-evidence-v1001.json")
	if err != nil {
		t.Fatalf("read evidence catalog: %v", err)
	}
	return catalog
}

func mustStrictFinalManifest(t *testing.T) rendererManifest {
	t.Helper()
	manifest := cloneManifest(t, mustReadManifest(t))
	for index := range manifest.Entries {
		entry := &manifest.Entries[index]
		entry.RendererStatus = "implemented"
		entry.ImplementationSymbol = stringPointer("render::" + entry.SourceKey)
		entry.GalleryBuilder = stringPointer("gallery::" + entry.SourceKey)
		for variantIndex := range entry.RequiredNBTVariants {
			variant := &entry.RequiredNBTVariants[variantIndex]
			variant.WitnessIDs = []string{"variant::" + entry.SourceKey + "::" + variant.VariantID}
		}
		if entry.RendererClass == "sourced_logical_invisible" {
			entry.Witnesses.NoDraw = []string{"no_draw::" + entry.SourceKey}
		} else {
			entry.Witnesses.GPU = []string{"gpu::" + entry.SourceKey}
		}
	}
	return manifest
}

func cloneManifest(t *testing.T, manifest rendererManifest) rendererManifest {
	t.Helper()
	raw, err := marshalCanonical(manifest)
	if err != nil {
		t.Fatalf("encode renderer manifest clone: %v", err)
	}
	clone, err := decodeRendererManifest(raw)
	if err != nil {
		t.Fatalf("decode renderer manifest clone: %v", err)
	}
	return clone
}

func stringPointer(value string) *string {
	return &value
}

func mustResolveModuleRoot(t *testing.T) string {
	t.Helper()
	command := exec.Command("go", "list", "-m", "-f", "{{.Dir}}", dragonflyModule)
	command.Env = append(os.Environ(), "GOWORK=off")
	raw, err := command.Output()
	if err != nil {
		t.Fatalf("resolve Dragonfly module root: %v", err)
	}
	root := strings.TrimSpace(string(raw))
	if root == "" {
		t.Fatal("resolved empty Dragonfly module root")
	}
	return root
}

func writeHashedFixture(t *testing.T, path string, size int64) string {
	t.Helper()
	file, err := os.Create(path)
	if err != nil {
		t.Fatalf("create hashed fixture: %v", err)
	}
	hash := sha256.New()
	chunk := bytes.Repeat([]byte{0x5a}, 64<<10)
	remaining := size
	for remaining > 0 {
		writeSize := int64(len(chunk))
		if remaining < writeSize {
			writeSize = remaining
		}
		written, writeErr := file.Write(chunk[:writeSize])
		if writeErr != nil {
			file.Close()
			t.Fatalf("write hashed fixture: %v", writeErr)
		}
		if written != int(writeSize) {
			file.Close()
			t.Fatalf("write hashed fixture bytes = %d, want %d", written, writeSize)
		}
		if _, err := hash.Write(chunk[:writeSize]); err != nil {
			file.Close()
			t.Fatalf("hash fixture: %v", err)
		}
		remaining -= writeSize
	}
	if err := file.Close(); err != nil {
		t.Fatalf("close hashed fixture: %v", err)
	}
	return fmt.Sprintf("%x", hash.Sum(nil))
}

func assertGolden(t *testing.T, path string, got []byte) {
	t.Helper()
	want, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("read golden %s: %v", path, err)
	}
	// The generated artifact is always canonical LF. Keep the byte-for-byte
	// comparison meaningful on Windows checkouts created before the repository's
	// eol=lf attributes were present, where Git may already have materialized the
	// tracked text fixture with CRLF line endings.
	want = bytes.ReplaceAll(want, []byte("\r\n"), []byte("\n"))
	if !bytes.Equal(got, want) {
		t.Fatalf("%s differs from deterministic golden", path)
	}
}
