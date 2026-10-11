package bridgecontract

import (
	"os/exec"
	"testing"
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
}
