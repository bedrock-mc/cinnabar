package authcache

import (
	"os/exec"
	"testing"
)

func makeLinkedDirectory(t *testing.T, link, target string) {
	t.Helper()
	output, err := exec.Command("cmd", "/c", "mklink", "/J", link, target).CombinedOutput()
	if err != nil {
		t.Fatalf("create Windows junction: %v: %s", err, output)
	}
}
