package main

import (
	"os"
	"os/exec"
	"strings"
	"testing"
)

const entryFixture = "CINNABAR_CORE_ENTRY_FIXTURE"

// init runs the production entry point in an isolated child with the parent's permissions.
func init() {
	if argument := os.Getenv(entryFixture); argument != "" {
		os.Args = []string{os.Args[0], argument}
		main()
		os.Exit(0)
	}
}

func TestMainEntryPreservesInheritedPermissions(t *testing.T) {
	for _, test := range []struct {
		argument string
		exitCode int
		output   string
	}{
		{"-h", 0, "Usage of"},
		{"-definitely-not-a-flag", 1, "flag provided but not defined"},
	} {
		t.Run(test.argument, func(t *testing.T) {
			command := exec.Command(os.Args[0])
			command.Env = append(os.Environ(), entryFixture+"="+test.argument)
			output, err := command.CombinedOutput()
			code := 0
			if err != nil {
				if exit, ok := err.(*exec.ExitError); ok {
					code = exit.ExitCode()
				} else {
					t.Fatal(err)
				}
			}
			if code != test.exitCode || !strings.Contains(string(output), test.output) {
				t.Fatalf("main(%q): exit=%d, output=%s", test.argument, code, output)
			}
		})
	}
}
