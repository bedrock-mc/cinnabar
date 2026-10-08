package localworld

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type hostGrantInstance struct {
	*fakeInstance
	granted []string
}

// GrantHost records exactly which locally admitted player received the grant.
func (i *hostGrantInstance) GrantHost(_ context.Context, name string) error {
	i.granted = append(i.granted, name)
	return nil
}

func TestHostGrantRequiresCurrentCheatsEnabledBDSInstance(t *testing.T) {
	for _, test := range []struct {
		name, backend, address string
		cheats                 bool
		state                  State
		want                   bool
	}{
		{"owner", BackendBDS, "127.0.0.1:1", true, StateRunning, true},
		{"stale world", BackendBDS, "127.0.0.1:2", true, StateRunning, false},
		{"cheats off", BackendBDS, "127.0.0.1:1", false, StateRunning, false},
		{"dragonfly", BackendDragonfly, "127.0.0.1:1", true, StateRunning, false},
		{"closing", BackendBDS, "127.0.0.1:1", true, StateStopping, false},
	} {
		t.Run(test.name, func(t *testing.T) {
			inst := &hostGrantInstance{fakeInstance: newFakeInstance()}
			manager := &Manager{state: test.state, inst: inst, world: World{Backend: test.backend, AllowCheats: test.cheats}}
			if err := manager.GrantHost(t.Context(), test.address, "Local Host"); err != nil {
				t.Fatal(err)
			}
			if (len(inst.granted) == 1) != test.want {
				t.Fatalf("grants = %v", inst.granted)
			}
		})
	}
}

func TestHostOperatorCommandRefusesAmbiguousIdentity(t *testing.T) {
	for _, name := range []string{"", "@a", "Host*", " Host", "Host\nstop", "Host\r", "Host\"", "Host\\"} {
		if command, err := hostOperatorCommand(name); err == nil || command != "" {
			t.Fatalf("accepted %q", name)
		}
	}
	if command, err := hostOperatorCommand("Local Host"); err != nil || command != "op \"Local Host\"" {
		t.Fatalf("command = %q, %v", command, err)
	}
}

type hostConsole struct{ bytes.Buffer }

// Close satisfies the managed process's owned console contract.
func (*hostConsole) Close() error { return nil }

func TestNativeHostGrantWritesOnlyQuotedOwner(t *testing.T) {
	console := new(hostConsole)
	inst := &bdsInstance{Instance: &process{stdin: console}}
	if err := inst.GrantHost(t.Context(), "Local Host"); err != nil {
		t.Fatal(err)
	}
	if got := console.String(); got != "op \"Local Host\"\n" {
		t.Fatalf("console = %q", got)
	}
	if err := inst.GrantHost(t.Context(), "@a"); err == nil || strings.Count(console.String(), "\n") != 1 {
		t.Fatal("invalid grant reached console")
	}
}

func TestContainerHostGrantUsesConsoleHelper(t *testing.T) {
	env, logPath := fakeDockerEnv(t)
	inst := &containerInstance{runner: BDSRunner{Docker: os.Args[0], Env: env}, name: "fixture"}
	if err := inst.GrantHost(t.Context(), "Local Host"); err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(logPath)
	if err != nil || string(raw) != "exec fixture send-command op \"Local Host\"\n" {
		t.Fatalf("console = %q, %v", raw, err)
	}
	if err := inst.GrantHost(t.Context(), "Host\nstop"); err == nil {
		t.Fatal("invalid grant accepted")
	}
}

func TestBDSPermissionsResetPreviousHost(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "permissions.json")
	if err := os.WriteFile(path, []byte(`[{"permission":"operator","xuid":"123"}]`), 0600); err != nil {
		t.Fatal(err)
	}
	if err := resetBDSPermissions(dir); err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(path)
	if err != nil || string(raw) != "[]\n" {
		t.Fatalf("permissions = %q, %v", raw, err)
	}
}
