package main

import (
	"encoding/json"
	"os"
	"os/exec"
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
	bytes, err := os.ReadFile(filepath.Join("..", "..", "crates", "protocol-ids", "data", "protocol-ids.json"))
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

// TestGeneratedProtocolIDsMatchInputs checks the Rust declarations and active air identity.
func TestGeneratedProtocolIDsMatchInputs(t *testing.T) {
	python, err := exec.LookPath("python3")
	if err != nil {
		python, err = exec.LookPath("python")
	}
	if err != nil {
		t.Fatal("Python is required to check generated protocol IDs")
	}
	command := exec.Command(python, "generate_ids.py", "--check")
	if output, err := command.CombinedOutput(); err != nil {
		t.Fatalf("check generated protocol IDs: %v\n%s", err, output)
	}
}
