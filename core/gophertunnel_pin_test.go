package core_test

import (
	"os"
	"path/filepath"
	"regexp"
	"testing"
)

var gophertunnelFork = regexp.MustCompile(`github\.com/sandertv/gophertunnel\s*=>\s*(github\.com/hashimthearab/gophertunnel\s+\S+)`)

// Every module that builds against our gophertunnel fork must pin the same commit as the core,
// or the workspace refuses to build and standalone tools drift from the client.
func TestModulesPinTheCoreGophertunnelFork(t *testing.T) {
	core := forkPin(t, "go.mod")
	if core == "" {
		t.Fatal("core/go.mod does not replace gophertunnel with the fork")
	}
	modules, err := filepath.Glob("../tools/*/go.mod")
	if err != nil {
		t.Fatal(err)
	}
	for _, module := range modules {
		if pin := forkPin(t, module); pin != "" && pin != core {
			t.Errorf("%s pins %q, core pins %q", module, pin, core)
		}
	}
}

func forkPin(t *testing.T, path string) string {
	t.Helper()
	data, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	match := gophertunnelFork.FindSubmatch(data)
	if match == nil {
		return ""
	}
	return string(match[1])
}
