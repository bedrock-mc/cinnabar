package main

import (
	"bytes"
	"crypto/sha256"
	"encoding/json"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
)

// bindingChecker locates the generator's shared binding validator.
func bindingChecker(t *testing.T) (string, string, string) {
	t.Helper()
	root, err := filepath.Abs(filepath.Join("..", ".."))
	if err != nil {
		t.Fatal(err)
	}
	python, err := exec.LookPath("python3")
	if err != nil {
		python, err = exec.LookPath("python")
	}
	if err != nil {
		t.Fatal("binding validation requires Python 3")
	}
	return root, python, filepath.Join(root, "tools", "registrygen", "update_bindings.py")
}

// TestCommittedBlockRegistryBindingsMatchCurrentRegistry discovers pins in every tracked data file.
func TestCommittedBlockRegistryBindingsMatchCurrentRegistry(t *testing.T) {
	root, python, script := bindingChecker(t)
	if output, err := exec.Command(python, script, "--root", root, "--check").CombinedOutput(); err != nil {
		t.Fatalf("stale committed registry binding: %v\n%s", err, output)
	}
}

// TestNewRegistryDependentsCannotKeepStalePins covers newly added text and binary dependents.
func TestNewRegistryDependentsCannotKeepStalePins(t *testing.T) {
	source, python, script := bindingChecker(t)
	root := t.TempDir()
	read := func(path string) []byte {
		t.Helper()
		data, err := os.ReadFile(filepath.Join(source, filepath.FromSlash(path)))
		if err != nil {
			t.Fatal(err)
		}
		return data
	}
	var target bedrockTargetManifest
	if err := json.Unmarshal(read("assets/bedrock-target.json"), &target); err != nil {
		t.Fatal(err)
	}
	registry := read(target.Artifacts["block_registry"])
	current := sha256.Sum256(registry)
	old := sha256.Sum256(read("crates/assets/data/block-registry-v1001.bin"))
	write := func(path string, data []byte) {
		t.Helper()
		destination := filepath.Join(root, filepath.FromSlash(path))
		if err := os.MkdirAll(filepath.Dir(destination), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(destination, data, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	write("assets/bedrock-target.json", read("assets/bedrock-target.json"))
	write(target.Artifacts["block_registry"], registry)
	runGit := func(args ...string) {
		t.Helper()
		if output, err := exec.Command("git", append([]string{"-C", root}, args...)...).CombinedOutput(); err != nil {
			t.Fatalf("git: %v: %s", err, output)
		}
	}
	runGit("init", "--quiet")
	for _, kind := range []string{"json", "tsv", "bin", "unrecognized"} {
		t.Run(kind, func(t *testing.T) {
			path := fmt.Sprintf("crates/assets/data/additional-binding-v%d.%s", target.WireProtocol, kind)
			makeBinding := func(hash [32]byte) []byte {
				switch kind {
				case "json":
					return []byte(fmt.Sprintf(`{"registry_sha256":"%x"}`, hash))
				case "tsv":
					return []byte(fmt.Sprintf("# registry_sha256=%x\n", hash))
				case "bin":
					data := bytes.Clone(read(target.Artifacts["light_registry"]))
					copy(data[16:48], hash[:])
					return data
				default:
					return hash[:]
				}
			}
			write(path, makeBinding(current))
			runGit("add", ".")
			output, err := exec.Command(python, script, "--root", root, "--check").CombinedOutput()
			if kind == "unrecognized" {
				if err == nil || !bytes.Contains(output, []byte("unrecognized")) {
					t.Fatalf("accepted an unregistered binding format: %v: %s", err, output)
				}
			} else {
				if err != nil {
					t.Fatalf("rejected current binding: %v: %s", err, output)
				}
				write(path, makeBinding(old))
				if output, err := exec.Command(python, script, "--root", root, "--check").CombinedOutput(); err == nil || !bytes.Contains(output, []byte(filepath.FromSlash(path))) {
					t.Fatalf("accepted stale %s binding: %v: %s", kind, err, output)
				}
			}
			if err := os.Remove(filepath.Join(root, filepath.FromSlash(path))); err != nil {
				t.Fatal(err)
			}
			runGit("add", ".")
		})
	}
}
