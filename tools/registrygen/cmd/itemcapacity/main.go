package main

import (
	"bufio"
	"bytes"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"regexp"
	"sort"
	"strconv"
	"strings"
)

const (
	targetVersion        = "1.26.40.8"
	targetProtocol       = 2168
	targetProbeRows      = 1518
	targetRetailRows     = 1485
	targetBinarySHA256   = "e7775e636b9fdbbc354823d92d0c22c12738a2141d12557d856744293d258372"
	targetLogSHA256      = "b41a522ab7d23ad8fa7b07f295c4c7d8c6539fcdea4c62575cdb2c1755fc8600"
	targetRetailSHA256   = "ee8917e7293c89469d6d114cad634eac0b45a702a1d73e2edddd6d5eeee725d0"
	targetScriptSHA256   = "c7d3312f08083001aa695368806730379bd0e1b787dbb236eb0ae566e0d43cc4"
	targetManifestSHA256 = "317e96cb230dadcf60331d7b7aaa63d912527840cad13659d1cfb159f126c4f9"
	maximumInputBytes    = 16 << 20
	packHeader           = "Pack Stack - [00] Local item capacity probe (id: a0318bd0-ae1c-4c36-9008-074c5c5cb4df, version: 1.0.0) @ behavior_packs/cinnabar_capacity_probe"
)

//go:embed probe/main.js
var probeScript []byte

//go:embed probe/manifest.json
var probeManifest []byte

var itemIdentifier = regexp.MustCompile(`^minecraft:[a-z0-9_]+$`)

type probeExpectations struct {
	version  string
	rowCount int
}

type capacityRow struct {
	identifier string
	maxCount   uint8
}

type provenance struct {
	Schema          string             `json:"schema"`
	SchemaVersion   int                `json:"schema_version"`
	GameVersion     string             `json:"game_version"`
	ProtocolVersion int                `json:"protocol_version"`
	Source          provenanceSource   `json:"source"`
	RetailAllowlist provenanceArtifact `json:"retail_allowlist"`
	Output          provenanceArtifact `json:"output"`
}

type provenanceSource struct {
	PublicBinarySHA256  string `json:"public_binary_sha256"`
	ProbeLogSHA256      string `json:"probe_log_sha256"`
	ProbeScriptSHA256   string `json:"probe_script_sha256"`
	ProbeManifestSHA256 string `json:"probe_manifest_sha256"`
	ItemTypeCount       int    `json:"item_type_count"`
	Failures            int    `json:"failures"`
}

type provenanceArtifact struct {
	Entries int    `json:"entries"`
	SHA256  string `json:"sha256"`
}

func main() {
	probeLogPath := flag.String("probe-log", "", "path to the complete public server probe log")
	retailPath := flag.String("retail-items", "", "path to the retail item allowlist TSV")
	outPath := flag.String("out", "", "capacity TSV output path")
	provenancePath := flag.String("provenance-out", "", "provenance JSON output path")
	flag.Parse()

	if err := run(*probeLogPath, *retailPath, *outPath, *provenancePath); err != nil {
		fmt.Fprintln(os.Stderr, "itemcapacity:", err)
		os.Exit(1)
	}
}

func run(probeLogPath, retailPath, outPath, provenancePath string) error {
	if probeLogPath == "" || retailPath == "" || outPath == "" || provenancePath == "" {
		return errors.New("-probe-log, -retail-items, -out, and -provenance-out are required")
	}
	if outPath == provenancePath {
		return errors.New("capacity and provenance output paths must differ")
	}
	if got := sha256Hex(probeScript); got != targetScriptSHA256 {
		return fmt.Errorf("bundled probe script hash %s does not match pinned hash", got)
	}
	if got := sha256Hex(probeManifest); got != targetManifestSHA256 {
		return fmt.Errorf("bundled probe manifest hash %s does not match pinned hash", got)
	}

	logBytes, err := readBounded(probeLogPath)
	if err != nil {
		return fmt.Errorf("read probe log: %w", err)
	}
	if got := sha256Hex(logBytes); got != targetLogSHA256 {
		return fmt.Errorf("probe log hash %s does not match pinned hash", got)
	}
	retailBytes, err := readBounded(retailPath)
	if err != nil {
		return fmt.Errorf("read retail allowlist: %w", err)
	}
	if got := sha256Hex(retailBytes); got != targetRetailSHA256 {
		return fmt.Errorf("retail allowlist hash %s does not match pinned hash", got)
	}

	rows, err := parseProbeLog(logBytes, probeExpectations{version: targetVersion, rowCount: targetProbeRows})
	if err != nil {
		return err
	}
	allowlist, err := parseRetailItems(retailBytes)
	if err != nil {
		return err
	}
	if len(allowlist) != targetRetailRows {
		return fmt.Errorf("retail allowlist contains %d rows, want %d", len(allowlist), targetRetailRows)
	}
	projected, err := projectRetail(rows, allowlist)
	if err != nil {
		return err
	}
	table := encodeCapacityTable(projected)
	metadata := provenance{
		Schema: "cinnabar-item-capacity-provenance", SchemaVersion: 1,
		GameVersion: targetVersion, ProtocolVersion: targetProtocol,
		Source: provenanceSource{
			PublicBinarySHA256: targetBinarySHA256, ProbeLogSHA256: targetLogSHA256,
			ProbeScriptSHA256: targetScriptSHA256, ProbeManifestSHA256: targetManifestSHA256,
			ItemTypeCount: targetProbeRows, Failures: 0,
		},
		RetailAllowlist: provenanceArtifact{Entries: len(allowlist), SHA256: targetRetailSHA256},
		Output:          provenanceArtifact{Entries: len(projected), SHA256: sha256Hex(table)},
	}
	metadataBytes, err := encodeProvenance(metadata)
	if err != nil {
		return err
	}
	if err := writeFileAtomic(outPath, table); err != nil {
		return fmt.Errorf("write capacity table: %w", err)
	}
	if err := writeFileAtomic(provenancePath, metadataBytes); err != nil {
		return fmt.Errorf("write provenance: %w", err)
	}
	return nil
}

func encodeProvenance(metadata provenance) ([]byte, error) {
	contents, err := json.MarshalIndent(metadata, "", "  ")
	if err != nil {
		return nil, fmt.Errorf("encode provenance: %w", err)
	}
	return append(contents, '\n'), nil
}

func parseProbeLog(contents []byte, expected probeExpectations) (map[string]uint8, error) {
	rows := make(map[string]uint8, expected.rowCount)
	versionMarkers := 0
	packMarkers := 0
	doneMarkers := 0
	doneTotal := 0
	doneFailures := 0
	scanner := bufio.NewScanner(bytes.NewReader(contents))
	scanner.Buffer(make([]byte, 4096), 1<<20)
	for scanner.Scan() {
		line := strings.TrimSuffix(scanner.Text(), "\r")
		if strings.Contains(line, " INFO] Version: ") {
			versionMarkers++
			if !strings.HasSuffix(line, "Version: "+expected.version) {
				return nil, fmt.Errorf("unexpected server version header")
			}
		}
		if strings.Contains(line, "Pack Stack - [00]") {
			if !strings.Contains(line, packHeader) {
				return nil, fmt.Errorf("unexpected probe pack header")
			}
			packMarkers++
		}
		if strings.Contains(line, "CINNABAR_CAPACITY_ERROR ") {
			return nil, errors.New("probe log contains a capacity error")
		}
		if payload, ok := markerPayload(line, "CINNABAR_CAPACITY_ROW "); ok {
			var fields []json.RawMessage
			if err := json.Unmarshal([]byte(payload), &fields); err != nil || len(fields) != 3 {
				return nil, errors.New("invalid capacity row")
			}
			var identifier string
			var maxCount int
			var stackable bool
			if json.Unmarshal(fields[0], &identifier) != nil || json.Unmarshal(fields[1], &maxCount) != nil || json.Unmarshal(fields[2], &stackable) != nil {
				return nil, errors.New("invalid capacity row fields")
			}
			if !itemIdentifier.MatchString(identifier) || maxCount < 1 || maxCount > 255 {
				return nil, errors.New("invalid capacity row value")
			}
			if stackable != (maxCount > 1) {
				return nil, errors.New("capacity row stackability disagrees with max count")
			}
			if _, exists := rows[identifier]; exists {
				return nil, errors.New("duplicate capacity row")
			}
			rows[identifier] = uint8(maxCount)
		}
		if payload, ok := markerPayload(line, "CINNABAR_CAPACITY_DONE "); ok {
			doneMarkers++
			var done struct {
				Total    int `json:"total"`
				Failures int `json:"failures"`
			}
			decoder := json.NewDecoder(strings.NewReader(payload))
			decoder.DisallowUnknownFields()
			if err := decoder.Decode(&done); err != nil {
				return nil, errors.New("invalid capacity completion marker")
			}
			doneTotal, doneFailures = done.Total, done.Failures
		}
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("scan probe log: %w", err)
	}
	if versionMarkers != 1 || packMarkers != 1 {
		return nil, errors.New("probe log does not contain exactly one expected version and pack header")
	}
	if doneMarkers != 1 || doneTotal != expected.rowCount || doneFailures != 0 {
		return nil, errors.New("probe completion marker is missing or inconsistent")
	}
	if len(rows) != expected.rowCount {
		return nil, fmt.Errorf("probe log contains %d unique rows, want %d", len(rows), expected.rowCount)
	}
	return rows, nil
}

func markerPayload(line, marker string) (string, bool) {
	index := strings.Index(line, marker)
	if index < 0 {
		return "", false
	}
	return line[index+len(marker):], true
}

func parseRetailItems(contents []byte) ([]string, error) {
	var identifiers []string
	seenIDs := make(map[int]struct{})
	seenNames := make(map[string]struct{})
	scanner := bufio.NewScanner(bytes.NewReader(contents))
	for scanner.Scan() {
		fields := strings.Split(strings.TrimSuffix(scanner.Text(), "\r"), "\t")
		if len(fields) != 2 {
			return nil, errors.New("invalid retail item row")
		}
		id, err := strconv.Atoi(fields[0])
		if err != nil || !itemIdentifier.MatchString(fields[1]) {
			return nil, errors.New("invalid retail item row value")
		}
		if _, exists := seenIDs[id]; exists {
			return nil, errors.New("duplicate retail runtime ID")
		}
		if _, exists := seenNames[fields[1]]; exists {
			return nil, errors.New("duplicate retail item identifier")
		}
		seenIDs[id] = struct{}{}
		seenNames[fields[1]] = struct{}{}
		identifiers = append(identifiers, fields[1])
	}
	if err := scanner.Err(); err != nil {
		return nil, fmt.Errorf("scan retail items: %w", err)
	}
	if len(identifiers) == 0 {
		return nil, errors.New("retail item allowlist is empty")
	}
	return identifiers, nil
}

func projectRetail(rows map[string]uint8, allowlist []string) ([]capacityRow, error) {
	projected := make([]capacityRow, 0, len(allowlist))
	for _, identifier := range allowlist {
		maxCount, ok := rows[identifier]
		if !ok {
			return nil, fmt.Errorf("retail identifier %s is missing from probe", identifier)
		}
		projected = append(projected, capacityRow{identifier: identifier, maxCount: maxCount})
	}
	sort.Slice(projected, func(i, j int) bool { return projected[i].identifier < projected[j].identifier })
	return projected, nil
}

func encodeCapacityTable(rows []capacityRow) []byte {
	var output bytes.Buffer
	for _, row := range rows {
		fmt.Fprintf(&output, "%s\t%d\n", row.identifier, row.maxCount)
	}
	return output.Bytes()
}

func readBounded(path string) ([]byte, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	contents, err := io.ReadAll(io.LimitReader(file, maximumInputBytes+1))
	if err != nil {
		return nil, err
	}
	if len(contents) > maximumInputBytes {
		return nil, fmt.Errorf("input exceeds %d bytes", maximumInputBytes)
	}
	return contents, nil
}

func writeFileAtomic(path string, contents []byte) error {
	directory := filepath.Dir(path)
	temporary, err := os.CreateTemp(directory, ".itemcapacity-*")
	if err != nil {
		return err
	}
	temporaryPath := temporary.Name()
	defer os.Remove(temporaryPath)
	if _, err := temporary.Write(contents); err != nil {
		temporary.Close()
		return err
	}
	if err := temporary.Close(); err != nil {
		return err
	}
	if err := os.Chmod(temporaryPath, 0o644); err != nil {
		return err
	}
	return os.Rename(temporaryPath, path)
}

func sha256Hex(contents []byte) string {
	hash := sha256.Sum256(contents)
	return hex.EncodeToString(hash[:])
}
