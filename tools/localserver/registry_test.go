package localserver

import (
	"bytes"
	"encoding/json"
	"io"
	"os"
	"strings"
	"testing"

	"github.com/df-mc/dragonfly/server"
	"github.com/df-mc/dragonfly/server/world"
)

func TestRegistryAndListCommand(t *testing.T) {
	const id = "registry-test-islands"
	called := false
	factory := func(seed int64) (map[world.Dimension]world.Generator, error) {
		called = true
		if seed != -7 {
			t.Errorf("seed = %d", seed)
		}
		return nil, nil
	}
	if err := RegisterGenerator(id, "Floating Islands", factory); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		generatorRegistry.Lock()
		defer generatorRegistry.Unlock()
		generatorRegistry.entries = generatorRegistry.entries[:len(generatorRegistry.entries)-1]
	})
	for _, invalid := range []struct {
		id, label string
		factory   GeneratorFactory
	}{{"", "label", factory}, {"id", "", factory}, {"id", "label", nil}, {id, "duplicate", factory}} {
		if RegisterGenerator(invalid.id, invalid.label, invalid.factory) == nil {
			t.Fatal("invalid registration accepted")
		}
	}
	var out bytes.Buffer
	if err := Run([]string{"-list-generators"}, strings.NewReader(""), &out, io.Discard); err != nil {
		t.Fatal(err)
	}
	var list []GeneratorInfo
	if err := json.Unmarshal(out.Bytes(), &list); err != nil {
		t.Fatal(err)
	}
	if list[len(list)-1] != (GeneratorInfo{id, "Floating Islands"}) || called {
		t.Fatalf("list = %v; factory called = %v", list, called)
	}
	cfg, err := parseSettings([]string{"-dir", "d", "-addr", "127.0.0.1:0", "-generator", id, "-seed", "-7"}, io.Discard)
	if err != nil {
		t.Fatal(err)
	}
	var conf server.Config
	generators, err := cfg.configureGenerators(&conf)
	if err != nil {
		t.Fatal(err)
	}
	defer generators.close()
	if !called {
		t.Fatal("custom factory was not used")
	}
	list[0].ID = "changed"
	if generatorList()[0].ID == "changed" {
		t.Fatal("catalog aliases registry")
	}
}

// TestGeneratorCatalogContract checks the emitted catalog against the shared wire cases.
func TestGeneratorCatalogContract(t *testing.T) {
	raw, err := os.ReadFile("contract/conformance.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name, JSON string
		Accepted   bool
	}
	if err := json.Unmarshal(raw, &cases); err != nil {
		t.Fatal(err)
	}
	for _, c := range cases {
		t.Run(c.Name, func(t *testing.T) {
			var list []GeneratorInfo
			decoder := json.NewDecoder(strings.NewReader(c.JSON))
			decoder.DisallowUnknownFields()
			err := decoder.Decode(&list)
			var extra any
			valid := err == nil && decoder.Decode(&extra) == io.EOF && len(list) > 0
			ids := map[string]bool{}
			for _, entry := range list {
				valid = valid && entry.ID != "" && entry.Label != "" && !ids[entry.ID]
				ids[entry.ID] = true
			}
			if valid != c.Accepted {
				t.Fatalf("accepted=%v, want %v: %v", valid, c.Accepted, err)
			}
		})
	}
}

// TestShippedCatalogExample keeps offline UI fixtures in sync with the registry.
func TestShippedCatalogExample(t *testing.T) {
	raw, err := os.ReadFile("contract/catalog.example.json")
	if err != nil {
		t.Fatal(err)
	}
	var example []GeneratorInfo
	if err := json.Unmarshal(raw, &example); err != nil {
		t.Fatal(err)
	}
	actual, _ := json.Marshal(generatorList())
	expected, _ := json.Marshal(example)
	if !bytes.Equal(actual, expected) {
		t.Fatalf("update the shared catalog example: got %s, want %s", actual, expected)
	}
}

// registryGenerator tracks cleanup of a factory's partially constructed result.
type registryGenerator struct {
	pregenTestGenerator
	closed bool
}

// Close records that startup released the generator.
func (g *registryGenerator) Close() error { g.closed = true; return nil }

func TestCustomGeneratorDimensionsAndCleanup(t *testing.T) {
	const id = "registry-dimensions"
	generator := &registryGenerator{}
	if err := RegisterGenerator(id, "Test", func(int64) (map[world.Dimension]world.Generator, error) {
		return map[world.Dimension]world.Generator{world.Overworld: generator}, nil
	}); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		generatorRegistry.Lock()
		defer generatorRegistry.Unlock()
		generatorRegistry.entries = generatorRegistry.entries[:len(generatorRegistry.entries)-1]
	})
	var conf server.Config
	if _, err := (settings{generator: id}).configureGenerators(&conf); err == nil {
		t.Fatal("incomplete dimension map accepted")
	}
	if !generator.closed {
		t.Fatal("failed startup leaked a generator")
	}
}

func TestCustomFactoryInstallsEveryDimension(t *testing.T) {
	const id = "registry-complete"
	built := map[world.Dimension]world.Generator{
		world.Overworld: &registryGenerator{}, world.Nether: &registryGenerator{}, world.End: &registryGenerator{},
	}
	if err := RegisterGenerator(id, "Complete", func(seed int64) (map[world.Dimension]world.Generator, error) {
		if seed != -7 {
			t.Errorf("factory seed = %d", seed)
		}
		return built, nil
	}); err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() {
		generatorRegistry.Lock()
		defer generatorRegistry.Unlock()
		generatorRegistry.entries = generatorRegistry.entries[:len(generatorRegistry.entries)-1]
	})
	var conf server.Config
	generators, err := (settings{generator: id, seed: -7}).configureGenerators(&conf)
	if err != nil {
		t.Fatal(err)
	}
	for dim, generator := range built {
		if conf.Generator(dim) != generator {
			t.Fatalf("dimension %v did not use the registered generator", dim)
		}
	}
	if err := generators.close(); err != nil {
		t.Fatal(err)
	}
	for _, generator := range built {
		if !generator.(*registryGenerator).closed {
			t.Fatal("generator was not closed")
		}
	}
}
