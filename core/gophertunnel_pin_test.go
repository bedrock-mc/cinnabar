package core_test

import (
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"testing"
)

// Each transport consumer must use the same fork commits as the client core.
func TestModulesPinTheCoreTransportForks(t *testing.T) {
	modules, err := filepath.Glob("../tools/*/go.mod")
	if err != nil {
		t.Fatal(err)
	}
	for _, library := range []string{"gophertunnel", "go-raknet"} {
		t.Run(library, func(t *testing.T) {
			core := forkPin(t, "go.mod", library)
			if core == "" {
				t.Fatalf("core/go.mod does not replace %s with the fork", library)
			}
			for _, module := range modules {
				if pin := forkPin(t, module, library); pin != "" && pin != core {
					t.Errorf("%s pins %q, core pins %q", module, pin, core)
				}
			}
		})
	}
}

// forkPin reads one selected transport replacement from a module manifest.
func forkPin(t *testing.T, path, library string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	pattern := regexp.MustCompile(fmt.Sprintf(`github\.com/sandertv/%s\s*=>\s*(github\.com/hashimthearab/%s\s+\S+)`, regexp.QuoteMeta(library), regexp.QuoteMeta(library)))
	match := pattern.FindSubmatch(data)
	if match == nil {
		return ""
	}
	return string(match[1])
}
