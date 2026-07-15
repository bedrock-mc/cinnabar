package main

import (
	"bytes"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func TestEvidenceCatalogRejectsCrossIdentityAndReceiptDrift(t *testing.T) {
	manifest, catalog, identities := validEvidenceCatalog(t)
	tests := []struct {
		name string
		edit func(*evidenceCatalog, *evidenceIdentities)
		want string
	}{
		{"wrong source", func(c *evidenceCatalog, _ *evidenceIdentities) { c.Records[0].SourceKey = "Chest" }, "evidence source mismatch"},
		{"wrong variant", func(c *evidenceCatalog, _ *evidenceIdentities) { c.Records[0].VariantID = "inventory_named" }, "evidence variant mismatch"},
		{"wrong kind", func(c *evidenceCatalog, _ *evidenceIdentities) { c.Records[0].Kind = "no_draw" }, "evidence kind mismatch"},
		{"stale BREG", func(c *evidenceCatalog, _ *evidenceIdentities) { c.BREGSHA256 = strings.Repeat("0", 64) }, "BREG hash mismatch"},
		{"stale assets", func(c *evidenceCatalog, _ *evidenceIdentities) { c.MCBEASSHA256 = strings.Repeat("0", 64) }, "MCBEAS hash mismatch"},
		{"stale inventory contract", func(c *evidenceCatalog, _ *evidenceIdentities) { c.SourceContractSHA256 = strings.Repeat("0", 64) }, "source contract hash mismatch"},
		{"stale manifest contract", func(c *evidenceCatalog, _ *evidenceIdentities) { c.RendererContractSHA256 = strings.Repeat("0", 64) }, "renderer contract hash mismatch"},
		{"stale request", func(c *evidenceCatalog, _ *evidenceIdentities) { c.GalleryRequestSHA256 = strings.Repeat("0", 64) }, "gallery request hash mismatch"},
		{"wrong state", func(_ *evidenceCatalog, i *evidenceIdentities) {
			target := i.Targets[catalog.Records[0].WitnessID]
			target.CanonicalState = "{}"
			i.Targets[catalog.Records[0].WitnessID] = target
		}, "state identity mismatch"},
		{"wrong NBT", func(c *evidenceCatalog, _ *evidenceIdentities) { c.Records[0].NBTSHA256 = strings.Repeat("0", 64) }, "NBT digest mismatch"},
		{"non-adjacent", func(c *evidenceCatalog, _ *evidenceIdentities) { c.Records[0].Frames[1].FrameGeneration += 1 }, "frames are not adjacent"},
		{"frame identity drift", func(c *evidenceCatalog, _ *evidenceIdentities) {
			c.Records[0].Frames[1].PresentedDigestSHA256 = strings.Repeat("e", 64)
		}, "frame identity drift"},
		{"wrong stream", func(c *evidenceCatalog, _ *evidenceIdentities) { c.Records[0].Frames[1].BackingStream = "model" }, "backing stream mismatch"},
		{"wrong backing count", func(c *evidenceCatalog, _ *evidenceIdentities) { c.Records[0].Frames[1].BackingRefCount = 2 }, "backing reference count mismatch"},
		{"extra draw", func(c *evidenceCatalog, _ *evidenceIdentities) { c.Records[0].Frames[1].AdditionalRefCount = 1 }, "additional block-entity references"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			candidate := cloneEvidenceCatalog(t, catalog)
			candidateIdentities := cloneEvidenceIdentities(identities)
			test.edit(&candidate, &candidateIdentities)
			if _, err := joinEvidence(manifest, candidate, candidateIdentities); err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want %q", err, test.want)
			}
		})
	}
}

func TestEvidenceCatalogRejectsUnknownDuplicateAndIncompleteWitnesses(t *testing.T) {
	manifest, catalog, identities := validEvidenceCatalog(t)
	tests := []struct {
		name string
		edit func(*rendererManifest, *evidenceCatalog)
		want string
	}{
		{"unknown witness", func(m *rendererManifest, _ *evidenceCatalog) {
			m.Entries[1].RequiredNBTVariants[0].WitnessIDs = []string{"unknown"}
		}, "unknown evidence witness"},
		{"duplicate witness", func(_ *rendererManifest, c *evidenceCatalog) { c.Records[1].WitnessID = c.Records[0].WitnessID }, "duplicate evidence witness"},
		{"unsorted ownership", func(_ *rendererManifest, c *evidenceCatalog) { c.Records[0], c.Records[1] = c.Records[1], c.Records[0] }, "records are not sorted"},
		{"unowned record", func(m *rendererManifest, _ *evidenceCatalog) {
			m.Entries[1].Witnesses.GPU = m.Entries[1].Witnesses.GPU[1:]
		}, "unowned evidence witness"},
		{"incomplete variant", func(m *rendererManifest, _ *evidenceCatalog) { m.Entries[1].RequiredNBTVariants[0].WitnessIDs = nil }, "incomplete evidence variant"},
	}
	for _, test := range tests {
		t.Run(test.name, func(t *testing.T) {
			candidateManifest := cloneManifest(t, manifest)
			candidateCatalog := cloneEvidenceCatalog(t, catalog)
			test.edit(&candidateManifest, &candidateCatalog)
			if _, err := joinEvidence(candidateManifest, candidateCatalog, cloneEvidenceIdentities(identities)); err == nil || !strings.Contains(err.Error(), test.want) {
				t.Fatalf("error = %v, want %q", err, test.want)
			}
		})
	}
}

func TestEvidenceCatalogDecoderIsBoundedStrictAndCanonical(t *testing.T) {
	path := filepath.Join(t.TempDir(), "evidence.json")
	file, err := os.Create(path)
	if err != nil {
		t.Fatalf("create oversized catalog: %v", err)
	}
	if err := file.Truncate(maxEvidenceCatalogBytes + 1); err != nil {
		file.Close()
		t.Fatalf("truncate oversized catalog: %v", err)
	}
	if err := file.Close(); err != nil {
		t.Fatalf("close oversized catalog: %v", err)
	}
	if _, err := readEvidenceCatalog(path); err == nil || !strings.Contains(err.Error(), "exceeds") {
		t.Fatalf("oversized catalog error = %v", err)
	}

	_, catalog, _ := validEvidenceCatalog(t)
	raw, err := marshalCanonical(catalog)
	if err != nil {
		t.Fatalf("encode catalog: %v", err)
	}
	if _, err := decodeEvidenceCatalog(append(raw, []byte("{}")...)); err == nil || !strings.Contains(err.Error(), "trailing") {
		t.Fatalf("trailing catalog error = %v", err)
	}
	unknown := bytes.Replace(raw, []byte("\"schema\":"), []byte("\"unknown\":0,\"schema\":"), 1)
	if _, err := decodeEvidenceCatalog(unknown); err == nil || !strings.Contains(err.Error(), "unknown") {
		t.Fatalf("unknown-field catalog error = %v", err)
	}
	catalog.Records[0].NBTSHA256 = strings.Repeat("A", 64)
	if _, err := joinEvidence(mustEvidenceManifest(t), catalog, evidenceIdentitiesForCatalog(catalog)); err == nil || !strings.Contains(err.Error(), "lowercase hexadecimal") {
		t.Fatalf("uppercase hash error = %v", err)
	}
}

func TestEvidenceCatalogEmptyJoinPreservesDeferredManifestAndBindsContracts(t *testing.T) {
	manifest := mustReadManifest(t)
	source := mustCollectSource(t)
	sourceDigest, err := sourceContractSHA256(source)
	if err != nil {
		t.Fatalf("source contract: %v", err)
	}
	rendererDigest, err := rendererContractSHA256(manifest)
	if err != nil {
		t.Fatalf("renderer contract: %v", err)
	}
	catalog := evidenceCatalog{
		Schema: evidenceSchema, ProtocolVersion: protocolVersion, GameVersion: gameVersion,
		SourceContractSHA256: sourceDigest, RendererContractSHA256: rendererDigest, Records: []evidenceRecord{},
	}
	joined, err := joinEvidence(manifest, catalog, evidenceIdentities{SourceContractSHA256: sourceDigest, RendererContractSHA256: rendererDigest, Targets: map[string]evidenceTargetIdentity{}})
	if err != nil {
		t.Fatalf("join empty evidence: %v", err)
	}
	if len(joined.Entries) != 22 || joined.evidenceDigest == "" || joined.sourceContractDigest != sourceDigest || joined.rendererContractDigest != rendererDigest {
		t.Fatalf("joined evidence metadata = entries %d, evidence %q, source %q, renderer %q", len(joined.Entries), joined.evidenceDigest, joined.sourceContractDigest, joined.rendererContractDigest)
	}
	for _, entry := range joined.Entries {
		if entry.RendererStatus != "deferred" || len(entry.Witnesses.GPU) != 0 || len(entry.Witnesses.NoDraw) != 0 {
			t.Fatalf("empty catalog promoted %q: %#v", entry.SourceKey, entry)
		}
	}
	claimed := cloneManifest(t, manifest)
	claimed.Entries[0].RequiredNBTVariants[0].WitnessIDs = []string{"invented"}
	if _, err := joinEvidence(claimed, catalog, evidenceIdentities{SourceContractSHA256: sourceDigest, RendererContractSHA256: rendererDigest, Targets: map[string]evidenceTargetIdentity{}}); err == nil || !strings.Contains(err.Error(), "unknown evidence witness") {
		t.Fatalf("empty catalog accepted invented witness: %v", err)
	}
}

func TestEvidenceContractProjectionsExcludePromotionAndEvidenceFields(t *testing.T) {
	manifest := mustReadManifest(t)
	baseline, err := rendererContractSHA256(manifest)
	if err != nil {
		t.Fatalf("renderer contract: %v", err)
	}
	promoted := cloneManifest(t, manifest)
	promoted.Entries[0].RendererStatus = "implemented"
	promoted.Entries[0].ImplementationSymbol = stringPointer("render::Banner")
	promoted.Entries[0].GalleryBuilder = stringPointer("gallery::Banner")
	promoted.Entries[0].RequiredNBTVariants[0].WitnessIDs = []string{"variant::banner"}
	promoted.Entries[0].Witnesses.GPU = []string{"gpu::banner"}
	promoted.digest = strings.Repeat("a", 64)
	promoted.evidenceDigest = strings.Repeat("b", 64)
	got, err := rendererContractSHA256(promoted)
	if err != nil {
		t.Fatalf("promoted renderer contract: %v", err)
	}
	if got != baseline {
		t.Fatalf("renderer contract changed across evidence promotion: %s != %s", got, baseline)
	}
}

func validEvidenceCatalog(t *testing.T) (rendererManifest, evidenceCatalog, evidenceIdentities) {
	t.Helper()
	manifest := cloneManifest(t, mustReadManifest(t))
	sources := map[string]bool{"Barrel": true, "BlastFurnace": true, "Furnace": true, "Smoker": true, "Jukebox": true, "Note": true}
	records := []evidenceRecord{}
	targets := map[string]evidenceTargetIdentity{}
	for entryIndex := range manifest.Entries {
		entry := &manifest.Entries[entryIndex]
		if !sources[entry.SourceKey] {
			continue
		}
		kind := "gpu"
		if entry.RendererClass == "sourced_logical_invisible" {
			kind = "no_draw"
		}
		var witnesses []string
		for variantIndex := range entry.RequiredNBTVariants {
			variant := &entry.RequiredNBTVariants[variantIndex]
			witness := "evidence::" + entry.SourceKey + "::" + variant.VariantID
			variant.WitnessIDs = []string{witness}
			witnesses = append(witnesses, witness)
			record := evidenceRecord{
				WitnessID: witness, SourceKey: entry.SourceKey, VariantID: variant.VariantID, Kind: kind,
				BREGSHA256: strings.Repeat("1", 64), MCBEASSHA256: strings.Repeat("2", 64),
				SourceContractSHA256: strings.Repeat("3", 64), RendererContractSHA256: strings.Repeat("4", 64),
				GalleryRequestSHA256: strings.Repeat("5", 64), NBTSHA256: strings.Repeat("6", 64),
				CanonicalState: "{\"minecraft:cardinal_direction\":\"north\"}", SequentialID: uint32(100 + len(records)),
				NetworkHash: uint32(200 + len(records)), Position: [3]int32{int32(len(records)), 64, 0},
				Frames: [2]frameReceipt{
					{FrameGeneration: 10, ViewGeneration: 7, BackingStream: "cube", BackingRefCount: 1, AdditionalRefCount: 0, PresentedDigestSHA256: strings.Repeat("7", 64)},
					{FrameGeneration: 11, ViewGeneration: 7, BackingStream: "cube", BackingRefCount: 1, AdditionalRefCount: 0, PresentedDigestSHA256: strings.Repeat("7", 64)},
				},
			}
			records = append(records, record)
			targets[witness] = evidenceTargetIdentity{
				SourceKey: record.SourceKey, VariantID: record.VariantID, NBTSHA256: record.NBTSHA256,
				CanonicalState: record.CanonicalState, SequentialID: record.SequentialID, NetworkHash: record.NetworkHash,
				Position: record.Position, BackingStream: "cube", BackingRefCount: 1, AdditionalRefCount: 0,
			}
		}
		if kind == "gpu" {
			entry.Witnesses.GPU = witnesses
		} else {
			entry.Witnesses.NoDraw = witnesses
		}
	}
	slices.SortFunc(records, func(left, right evidenceRecord) int { return strings.Compare(left.WitnessID, right.WitnessID) })
	catalog := evidenceCatalog{
		Schema: evidenceSchema, ProtocolVersion: protocolVersion, GameVersion: gameVersion,
		BREGSHA256: strings.Repeat("1", 64), MCBEASSHA256: strings.Repeat("2", 64),
		SourceContractSHA256: strings.Repeat("3", 64), RendererContractSHA256: strings.Repeat("4", 64),
		GalleryRequestSHA256: strings.Repeat("5", 64), Records: records,
	}
	identities := evidenceIdentities{
		BREGSHA256: catalog.BREGSHA256, MCBEASSHA256: catalog.MCBEASSHA256,
		SourceContractSHA256: catalog.SourceContractSHA256, RendererContractSHA256: catalog.RendererContractSHA256,
		GalleryRequestSHA256: catalog.GalleryRequestSHA256, Targets: targets,
	}
	return manifest, catalog, identities
}

func mustEvidenceManifest(t *testing.T) rendererManifest {
	t.Helper()
	manifest, _, _ := validEvidenceCatalog(t)
	return manifest
}

func cloneEvidenceCatalog(t *testing.T, catalog evidenceCatalog) evidenceCatalog {
	t.Helper()
	raw, err := marshalCanonical(catalog)
	if err != nil {
		t.Fatalf("encode evidence clone: %v", err)
	}
	clone, err := decodeEvidenceCatalog(raw)
	if err != nil {
		t.Fatalf("decode evidence clone: %v", err)
	}
	return clone
}

func cloneEvidenceIdentities(identities evidenceIdentities) evidenceIdentities {
	clone := identities
	clone.Targets = make(map[string]evidenceTargetIdentity, len(identities.Targets))
	for witness, target := range identities.Targets {
		clone.Targets[witness] = target
	}
	return clone
}

func evidenceIdentitiesForCatalog(catalog evidenceCatalog) evidenceIdentities {
	targets := make(map[string]evidenceTargetIdentity, len(catalog.Records))
	for _, record := range catalog.Records {
		targets[record.WitnessID] = evidenceTargetIdentity{
			SourceKey: record.SourceKey, VariantID: record.VariantID, NBTSHA256: record.NBTSHA256,
			CanonicalState: record.CanonicalState, SequentialID: record.SequentialID, NetworkHash: record.NetworkHash,
			Position: record.Position, BackingStream: record.Frames[0].BackingStream,
			BackingRefCount: record.Frames[0].BackingRefCount, AdditionalRefCount: record.Frames[0].AdditionalRefCount,
		}
	}
	return evidenceIdentities{
		BREGSHA256: catalog.BREGSHA256, MCBEASSHA256: catalog.MCBEASSHA256,
		SourceContractSHA256: catalog.SourceContractSHA256, RendererContractSHA256: catalog.RendererContractSHA256,
		GalleryRequestSHA256: catalog.GalleryRequestSHA256, Targets: targets,
	}
}
