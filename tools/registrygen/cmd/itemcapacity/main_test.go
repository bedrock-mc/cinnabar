package main

import (
	"bytes"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

var fixtureExpectations = probeExpectations{
	version:  "1.26.40.8",
	rowCount: 4,
}

func TestProbeFixturesRejectBadDuplicateAndIncompleteLogs(t *testing.T) {
	for _, name := range []string{"bad-row.txt", "duplicate.txt", "incomplete.txt"} {
		t.Run(name, func(t *testing.T) {
			if _, err := parseProbeLog(readFixture(t, name), fixtureExpectations); err == nil {
				t.Fatalf("parseProbeLog(%s) unexpectedly succeeded", name)
			}
		})
	}
}

func TestProbeRejectsCapacityOutsideUint8Range(t *testing.T) {
	contents := bytes.Replace(readFixture(t, "valid.txt"), []byte(`"minecraft:apple",64,true`), []byte(`"minecraft:apple",256,true`), 1)
	if _, err := parseProbeLog(contents, fixtureExpectations); err == nil {
		t.Fatal("parseProbeLog unexpectedly accepted capacity 256")
	}
}

func TestRetailProjectionRejectsMissingAllowedIdentifier(t *testing.T) {
	rows, err := parseProbeLog(readFixture(t, "valid.txt"), fixtureExpectations)
	if err != nil {
		t.Fatal(err)
	}
	allowlist, err := parseRetailItems([]byte("1\tminecraft:apple\n2\tminecraft:missing\n"))
	if err != nil {
		t.Fatal(err)
	}
	if _, err := projectRetail(rows, allowlist); err == nil || !strings.Contains(err.Error(), "minecraft:missing") {
		t.Fatalf("missing projection error = %v", err)
	}
}

func TestRetailProjectionIsPositiveSortedAndDeterministic(t *testing.T) {
	rows, err := parseProbeLog(readFixture(t, "valid.txt"), fixtureExpectations)
	if err != nil {
		t.Fatal(err)
	}
	allowlist, err := parseRetailItems(readFixture(t, "retail.tsv"))
	if err != nil {
		t.Fatal(err)
	}
	projected, err := projectRetail(rows, allowlist)
	if err != nil {
		t.Fatal(err)
	}
	want := "minecraft:apple\t64\nminecraft:bucket\t16\nminecraft:water_bucket\t1\n"
	first := encodeCapacityTable(projected)
	second := encodeCapacityTable(projected)
	if string(first) != want {
		t.Fatalf("capacity table = %q, want %q", first, want)
	}
	if !bytes.Equal(first, second) {
		t.Fatal("identical projection produced different bytes")
	}
	if bytes.Contains(first, []byte("probe_only")) {
		t.Fatal("non-retail probe identifier escaped projection")
	}
}

func TestRetailParserRejectsDuplicateAndMalformedRows(t *testing.T) {
	for _, input := range []string{
		"1\tminecraft:apple\n2\tminecraft:apple\n",
		"not-an-id\tminecraft:apple\n",
		"1\tinvalid\n",
	} {
		if _, err := parseRetailItems([]byte(input)); err == nil {
			t.Fatalf("parseRetailItems(%q) unexpectedly succeeded", input)
		}
	}
}

func TestBundledProbeAndProvenanceAreDeterministic(t *testing.T) {
	if got := sha256Hex(probeScript); got != targetScriptSHA256 {
		t.Fatalf("probe script hash = %s", got)
	}
	if got := sha256Hex(probeManifest); got != targetManifestSHA256 {
		t.Fatalf("probe manifest hash = %s", got)
	}
	metadata := provenance{
		Schema: "test", SchemaVersion: 1,
		Source: provenanceSource{ItemTypeCount: 4},
		Output: provenanceArtifact{Entries: 3, SHA256: "abc"},
	}
	first, err := encodeProvenance(metadata)
	if err != nil {
		t.Fatal(err)
	}
	second, err := encodeProvenance(metadata)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(first, second) {
		t.Fatal("identical provenance produced different bytes")
	}
}

func readFixture(t *testing.T, name string) []byte {
	t.Helper()
	contents, err := os.ReadFile(filepath.Join("testdata", name))
	if err != nil {
		t.Fatal(err)
	}
	return contents
}
