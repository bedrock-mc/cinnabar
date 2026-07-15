package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"math"
	"os"
	"strings"
)

const (
	evidenceSchema          = 1
	maxEvidenceCatalogBytes = 4 << 20
)

type frameReceipt struct {
	FrameGeneration       uint64 `json:"frame_generation"`
	ViewGeneration        uint64 `json:"view_generation"`
	BackingStream         string `json:"backing_stream"`
	BackingRefCount       uint64 `json:"backing_ref_count"`
	AdditionalRefCount    uint64 `json:"additional_block_entity_ref_count"`
	PresentedDigestSHA256 string `json:"presented_digest_sha256"`
}

type evidenceRecord struct {
	WitnessID              string          `json:"witness_id"`
	SourceKey              string          `json:"source_key"`
	VariantID              string          `json:"variant_id"`
	Kind                   string          `json:"kind"`
	BREGSHA256             string          `json:"breg_sha256"`
	MCBEASSHA256           string          `json:"mcbeas_sha256"`
	SourceContractSHA256   string          `json:"source_contract_sha256"`
	RendererContractSHA256 string          `json:"renderer_contract_sha256"`
	GalleryRequestSHA256   string          `json:"gallery_request_sha256"`
	NBTSHA256              string          `json:"nbt_sha256"`
	CanonicalState         string          `json:"canonical_state"`
	SequentialID           uint32          `json:"sequential_id"`
	NetworkHash            uint32          `json:"network_hash"`
	Position               [3]int32        `json:"position"`
	Frames                 [2]frameReceipt `json:"frames"`
}

type evidenceCatalog struct {
	Schema                 int              `json:"schema"`
	ProtocolVersion        int              `json:"protocol_version"`
	GameVersion            string           `json:"game_version"`
	BREGSHA256             string           `json:"breg_sha256"`
	MCBEASSHA256           string           `json:"mcbeas_sha256"`
	SourceContractSHA256   string           `json:"source_contract_sha256"`
	RendererContractSHA256 string           `json:"renderer_contract_sha256"`
	GalleryRequestSHA256   string           `json:"gallery_request_sha256"`
	Records                []evidenceRecord `json:"records"`
	digest                 string
}

type evidenceTargetIdentity struct {
	SourceKey          string
	VariantID          string
	NBTSHA256          string
	CanonicalState     string
	SequentialID       uint32
	NetworkHash        uint32
	Position           [3]int32
	BackingStream      string
	BackingRefCount    uint64
	AdditionalRefCount uint64
}

type evidenceIdentities struct {
	BREGSHA256             string
	MCBEASSHA256           string
	SourceContractSHA256   string
	RendererContractSHA256 string
	GalleryRequestSHA256   string
	Targets                map[string]evidenceTargetIdentity
}

type rendererContractManifest struct {
	Schema                      int                     `json:"schema"`
	ProtocolVersion             int                     `json:"protocol_version"`
	GameVersion                 string                  `json:"game_version"`
	DragonflyModule             string                  `json:"dragonfly_module"`
	DragonflyVersion            string                  `json:"dragonfly_version"`
	DragonflyRevision           string                  `json:"dragonfly_revision"`
	DragonflyRegistrationSHA256 string                  `json:"dragonfly_registration_sha256"`
	BDSServerVersion            string                  `json:"bds_server_version"`
	BDSExecutableSHA256         string                  `json:"bds_executable_sha256"`
	Entries                     []rendererContractEntry `json:"entries"`
}

type rendererContractEntry struct {
	SourceKey           string                       `json:"source_key"`
	NBTID               *string                      `json:"nbt_id"`
	NBTAliases          []string                     `json:"nbt_aliases"`
	RequiredNBTVariants []rendererContractNBTVariant `json:"required_nbt_variants"`
	ChunkNBTSupported   bool                         `json:"chunk_nbt_supported"`
	LiveUpdateSupported bool                         `json:"live_update_supported"`
	RendererClass       string                       `json:"renderer_class"`
}

type rendererContractNBTVariant struct {
	VariantID      string   `json:"variant_id"`
	RequiredFields []string `json:"required_fields"`
}

func readEvidenceCatalog(path string) (evidenceCatalog, error) {
	file, err := os.Open(path)
	if err != nil {
		return evidenceCatalog{}, fmt.Errorf("open evidence catalog: %w", err)
	}
	defer file.Close()
	info, err := file.Stat()
	if err != nil {
		return evidenceCatalog{}, fmt.Errorf("stat evidence catalog: %w", err)
	}
	if info.Size() > maxEvidenceCatalogBytes {
		return evidenceCatalog{}, fmt.Errorf("evidence catalog size %d exceeds %d bytes", info.Size(), maxEvidenceCatalogBytes)
	}
	raw, err := io.ReadAll(io.LimitReader(file, maxEvidenceCatalogBytes+1))
	if err != nil {
		return evidenceCatalog{}, fmt.Errorf("read evidence catalog: %w", err)
	}
	return decodeEvidenceCatalog(raw)
}

func decodeEvidenceCatalog(raw []byte) (evidenceCatalog, error) {
	if len(raw) > maxEvidenceCatalogBytes {
		return evidenceCatalog{}, fmt.Errorf("evidence catalog size %d exceeds %d bytes", len(raw), maxEvidenceCatalogBytes)
	}
	var catalog evidenceCatalog
	decoder := json.NewDecoder(bytes.NewReader(raw))
	decoder.DisallowUnknownFields()
	if err := decoder.Decode(&catalog); err != nil {
		return evidenceCatalog{}, fmt.Errorf("decode evidence catalog: %w", err)
	}
	if err := decoder.Decode(&struct{}{}); err != io.EOF {
		return evidenceCatalog{}, errors.New("evidence catalog has trailing JSON content")
	}
	canonical, err := marshalCanonical(catalog)
	if err != nil {
		return evidenceCatalog{}, fmt.Errorf("encode canonical evidence catalog: %w", err)
	}
	digest := sha256.Sum256(canonical)
	catalog.digest = hex.EncodeToString(digest[:])
	return catalog, nil
}

func sourceContractSHA256(source sourceInventory) (string, error) {
	return canonicalSHA256(source)
}

func rendererContractSHA256(manifest rendererManifest) (string, error) {
	projection := rendererContractManifest{
		Schema: manifest.Schema, ProtocolVersion: manifest.ProtocolVersion, GameVersion: manifest.GameVersion,
		DragonflyModule: manifest.DragonflyModule, DragonflyVersion: manifest.DragonflyVersion,
		DragonflyRevision: manifest.DragonflyRevision, DragonflyRegistrationSHA256: manifest.DragonflyRegistrationSHA256,
		BDSServerVersion: manifest.BDSServerVersion, BDSExecutableSHA256: manifest.BDSExecutableSHA256,
		Entries: make([]rendererContractEntry, 0, len(manifest.Entries)),
	}
	for _, entry := range manifest.Entries {
		projected := rendererContractEntry{
			SourceKey: entry.SourceKey, NBTID: entry.NBTID, NBTAliases: entry.NBTAliases,
			ChunkNBTSupported: entry.ChunkNBT.Supported, LiveUpdateSupported: entry.LiveUpdate.Supported,
			RendererClass:       entry.RendererClass,
			RequiredNBTVariants: make([]rendererContractNBTVariant, 0, len(entry.RequiredNBTVariants)),
		}
		for _, variant := range entry.RequiredNBTVariants {
			projected.RequiredNBTVariants = append(projected.RequiredNBTVariants, rendererContractNBTVariant{
				VariantID: variant.VariantID, RequiredFields: variant.RequiredFields,
			})
		}
		projection.Entries = append(projection.Entries, projected)
	}
	return canonicalSHA256(projection)
}

func canonicalSHA256(value any) (string, error) {
	raw, err := marshalCanonical(value)
	if err != nil {
		return "", err
	}
	digest := sha256.Sum256(raw)
	return hex.EncodeToString(digest[:]), nil
}

func joinEvidence(manifest rendererManifest, catalog evidenceCatalog, identities evidenceIdentities) (rendererManifest, error) {
	if catalog.Schema != evidenceSchema || catalog.ProtocolVersion != protocolVersion || catalog.GameVersion != gameVersion {
		return rendererManifest{}, errors.New("evidence catalog schema/protocol/game pin drift")
	}
	if catalog.digest == "" {
		digest, err := canonicalSHA256(catalog)
		if err != nil {
			return rendererManifest{}, fmt.Errorf("hash evidence catalog: %w", err)
		}
		catalog.digest = digest
	}
	if err := validateEvidenceHash("source contract", catalog.SourceContractSHA256); err != nil {
		return rendererManifest{}, err
	}
	if err := validateEvidenceHash("renderer contract", catalog.RendererContractSHA256); err != nil {
		return rendererManifest{}, err
	}
	if catalog.SourceContractSHA256 != identities.SourceContractSHA256 {
		return rendererManifest{}, errors.New("source contract hash mismatch")
	}
	if catalog.RendererContractSHA256 != identities.RendererContractSHA256 {
		return rendererManifest{}, errors.New("renderer contract hash mismatch")
	}
	if len(catalog.Records) == 0 {
		if catalog.BREGSHA256 != "" || catalog.MCBEASSHA256 != "" || catalog.GalleryRequestSHA256 != "" {
			return rendererManifest{}, errors.New("empty evidence catalog has artifact hashes")
		}
	} else {
		for _, pair := range []struct {
			name     string
			catalog  string
			expected string
		}{
			{"BREG", catalog.BREGSHA256, identities.BREGSHA256},
			{"MCBEAS", catalog.MCBEASSHA256, identities.MCBEASSHA256},
			{"gallery request", catalog.GalleryRequestSHA256, identities.GalleryRequestSHA256},
		} {
			if err := validateEvidenceHash(pair.name, pair.catalog); err != nil {
				return rendererManifest{}, err
			}
			if pair.catalog != pair.expected {
				return rendererManifest{}, fmt.Errorf("%s hash mismatch", pair.name)
			}
		}
	}

	records := make(map[string]evidenceRecord, len(catalog.Records))
	previousWitness := ""
	for index, record := range catalog.Records {
		if !isTrimmedNonEmpty(record.WitnessID) {
			return rendererManifest{}, errors.New("invalid evidence witness identifier")
		}
		if index != 0 && strings.Compare(previousWitness, record.WitnessID) >= 0 {
			if previousWitness == record.WitnessID {
				return rendererManifest{}, fmt.Errorf("duplicate evidence witness %q", record.WitnessID)
			}
			return rendererManifest{}, errors.New("evidence records are not sorted by witness ID")
		}
		previousWitness = record.WitnessID
		if _, exists := records[record.WitnessID]; exists {
			return rendererManifest{}, fmt.Errorf("duplicate evidence witness %q", record.WitnessID)
		}
		if err := validateEvidenceRecord(record, catalog, identities); err != nil {
			return rendererManifest{}, fmt.Errorf("%s: %w", record.WitnessID, err)
		}
		records[record.WitnessID] = record
	}

	variantOwners := make(map[string]int, len(records))
	kindOwners := make(map[string]int, len(records))
	for _, entry := range manifest.Entries {
		entryHasEvidence := false
		for _, variant := range entry.RequiredNBTVariants {
			if len(variant.WitnessIDs) != 0 {
				entryHasEvidence = true
			}
			for _, witness := range variant.WitnessIDs {
				record, exists := records[witness]
				if !exists {
					return rendererManifest{}, fmt.Errorf("unknown evidence witness %q", witness)
				}
				if record.SourceKey != entry.SourceKey {
					return rendererManifest{}, fmt.Errorf("evidence source mismatch for %q", witness)
				}
				if record.VariantID != variant.VariantID {
					return rendererManifest{}, fmt.Errorf("evidence variant mismatch for %q", witness)
				}
				variantOwners[witness]++
			}
		}
		for _, witness := range entry.Witnesses.GPU {
			entryHasEvidence = true
			if err := validateEvidenceWitnessKind(records, witness, entry.SourceKey, "gpu"); err != nil {
				return rendererManifest{}, err
			}
			kindOwners[witness]++
		}
		for _, witness := range entry.Witnesses.NoDraw {
			entryHasEvidence = true
			if err := validateEvidenceWitnessKind(records, witness, entry.SourceKey, "no_draw"); err != nil {
				return rendererManifest{}, err
			}
			kindOwners[witness]++
		}
		if entryHasEvidence {
			for _, variant := range entry.RequiredNBTVariants {
				if len(variant.WitnessIDs) == 0 {
					return rendererManifest{}, fmt.Errorf("%s: incomplete evidence variant %q", entry.SourceKey, variant.VariantID)
				}
			}
		}
	}
	for witness := range records {
		if variantOwners[witness] != 1 || kindOwners[witness] != 1 {
			return rendererManifest{}, fmt.Errorf("unowned evidence witness %q", witness)
		}
	}
	manifest.evidenceDigest = catalog.digest
	manifest.sourceContractDigest = catalog.SourceContractSHA256
	manifest.rendererContractDigest = catalog.RendererContractSHA256
	return manifest, nil
}

func validateEvidenceWitnessKind(records map[string]evidenceRecord, witness, sourceKey, kind string) error {
	record, exists := records[witness]
	if !exists {
		return fmt.Errorf("unknown evidence witness %q", witness)
	}
	if record.SourceKey != sourceKey {
		return fmt.Errorf("evidence source mismatch for %q", witness)
	}
	if record.Kind != kind {
		return fmt.Errorf("evidence kind mismatch for %q", witness)
	}
	return nil
}

func validateEvidenceRecord(record evidenceRecord, catalog evidenceCatalog, identities evidenceIdentities) error {
	if !isTrimmedNonEmpty(record.SourceKey) || !isTrimmedNonEmpty(record.VariantID) {
		return errors.New("invalid evidence source or variant identifier")
	}
	if record.Kind != "gpu" && record.Kind != "no_draw" {
		return fmt.Errorf("invalid evidence kind %q", record.Kind)
	}
	for _, binding := range []struct {
		name   string
		actual string
		want   string
	}{
		{"BREG", record.BREGSHA256, catalog.BREGSHA256},
		{"MCBEAS", record.MCBEASSHA256, catalog.MCBEASSHA256},
		{"source contract", record.SourceContractSHA256, catalog.SourceContractSHA256},
		{"renderer contract", record.RendererContractSHA256, catalog.RendererContractSHA256},
		{"gallery request", record.GalleryRequestSHA256, catalog.GalleryRequestSHA256},
	} {
		if err := validateEvidenceHash(binding.name, binding.actual); err != nil {
			return err
		}
		if binding.actual != binding.want {
			return fmt.Errorf("%s hash mismatch", binding.name)
		}
	}
	if err := validateEvidenceHash("NBT digest", record.NBTSHA256); err != nil {
		return err
	}
	target, exists := identities.Targets[record.WitnessID]
	if !exists {
		return errors.New("missing expected evidence target")
	}
	if record.SourceKey != target.SourceKey {
		return errors.New("evidence source mismatch")
	}
	if record.VariantID != target.VariantID {
		return errors.New("evidence variant mismatch")
	}
	if record.NBTSHA256 != target.NBTSHA256 {
		return errors.New("NBT digest mismatch")
	}
	if record.CanonicalState != target.CanonicalState || record.SequentialID != target.SequentialID || record.NetworkHash != target.NetworkHash || record.Position != target.Position {
		return errors.New("state identity mismatch")
	}
	for _, frame := range record.Frames {
		if frame.BackingStream != target.BackingStream {
			return errors.New("backing stream mismatch")
		}
		if frame.BackingRefCount != target.BackingRefCount {
			return errors.New("backing reference count mismatch")
		}
		if frame.AdditionalRefCount != target.AdditionalRefCount || frame.AdditionalRefCount != 0 {
			return errors.New("additional block-entity references must be zero")
		}
		if err := validateEvidenceHash("presented digest", frame.PresentedDigestSHA256); err != nil {
			return err
		}
	}
	first, second := record.Frames[0], record.Frames[1]
	if first.FrameGeneration == math.MaxUint64 || second.FrameGeneration != first.FrameGeneration+1 {
		return errors.New("evidence frames are not adjacent")
	}
	if first.ViewGeneration != second.ViewGeneration || first.BackingStream != second.BackingStream || first.BackingRefCount != second.BackingRefCount || first.AdditionalRefCount != second.AdditionalRefCount || first.PresentedDigestSHA256 != second.PresentedDigestSHA256 {
		return errors.New("evidence frame identity drift")
	}
	return nil
}

func validateEvidenceHash(name, value string) error {
	if len(value) != sha256.Size*2 {
		return fmt.Errorf("%s must be exactly 64 lowercase hexadecimal characters", name)
	}
	for _, character := range value {
		if (character < '0' || character > '9') && (character < 'a' || character > 'f') {
			return fmt.Errorf("%s must be exactly 64 lowercase hexadecimal characters", name)
		}
	}
	return nil
}
