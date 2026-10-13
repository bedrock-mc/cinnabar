package main

import (
	"encoding/json"
	"os"
	"path/filepath"
	"reflect"
	"testing"
)

// TestProtocolIDsMatchPinnedSource checks every value and notices newly added constants.
func TestProtocolIDsMatchPinnedSource(t *testing.T) {
	actual, err := protocolIDFixture()
	if err != nil {
		t.Fatal(err)
	}
	bytes, err := os.ReadFile(filepath.Join("..", "..", "crates", "protocol", "data", "protocol-ids.json"))
	if err != nil {
		t.Fatal(err)
	}
	var expected idFixture
	if err := json.Unmarshal(bytes, &expected); err != nil {
		t.Fatal(err)
	}
	if !reflect.DeepEqual(actual, expected) {
		t.Fatal("protocol ID fixture drift; regenerate with fixturegen -ids-out")
	}
}
