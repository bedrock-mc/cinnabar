package bridgecontract

import (
	"os/exec"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestSharedContractIsCurrent(t *testing.T) {
	python, err := exec.LookPath("python3")
	if err != nil {
		python, err = exec.LookPath("python")
	}
	if err != nil {
		t.Fatal("Python 3 is required to verify the shared bridge contract")
	}
	output, err := exec.Command(python, "../../../tools/bridgegen/generate.py", "--check").CombinedOutput()
	if err != nil {
		t.Fatalf("regenerate bridge declarations: %v\n%s", err, output)
	}
	output, err = exec.Command(python, "../../../tools/bridgegen/test_generate.py").CombinedOutput()
	if err != nil {
		t.Fatalf("bridge generator regression: %v\n%s", err, output)
	}
}

// TestResourcePackTypesMatchProtocol rejects drift from the selected protocol dependency.
func TestResourcePackTypesMatchProtocol(t *testing.T) {
	if ResourcePackTypeAddon != packet.ResourcePackTypeAddon ||
		ResourcePackTypeBehaviour != packet.ResourcePackTypeBehaviour ||
		ResourcePackTypeResources != packet.ResourcePackTypeResources ||
		ResourcePackTypeSkins != packet.ResourcePackTypeSkins ||
		ResourcePackTypeWorldTemplate != packet.ResourcePackTypeWorldTemplate {
		t.Fatal("shared archive delivery types differ from the selected protocol")
	}
}
