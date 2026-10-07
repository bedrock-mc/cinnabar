package localworld

import (
	"context"
	"encoding/json"
	"errors"
	"os"
	"path/filepath"
	"strconv"
	"strings"
	"testing"
)

func TestCreationChoicesPersistIndependently(t *testing.T) {
	store := newTestStore(t)
	for _, backend := range []string{BackendDragonfly, BackendBDS} {
		for _, generator := range []string{GeneratorNormal, GeneratorFlat} {
			prefs, err := store.UpdatePrefs(PrefsUpdate{CreationBackend: &backend, CreationGenerator: &generator})
			if err != nil || prefs.CreationBackend != backend || prefs.CreationGenerator != generator {
				t.Fatalf("choices = %+v, %v", prefs, err)
			}
			reopened, err := OpenStore(store.root)
			if err != nil || reopened.Prefs() != prefs {
				t.Fatalf("reopened choices = %+v, %v", reopened.Prefs(), err)
			}
		}
	}
	invalid := "void"
	before := store.Prefs()
	if _, err := store.UpdatePrefs(PrefsUpdate{CreationGenerator: &invalid}); !errors.Is(err, ErrInvalid) {
		t.Fatalf("void accepted: %v", err)
	}
	if store.Prefs() != before {
		t.Fatal("invalid choices changed saved preferences")
	}
}

func TestLegacyWorldSettingsKeepTheirOriginalServerAndTerrain(t *testing.T) {
	for _, backend := range []string{"", BackendDragonfly, BackendBDS} {
		store := newTestStore(t)
		world, err := store.Create(Spec{Name: "Legacy", Backend: BackendDragonfly})
		if err != nil {
			t.Fatal(err)
		}
		dir, _ := store.Dir(world.ID)
		raw, _ := os.ReadFile(filepath.Join(dir, metaFile))
		var metadata map[string]any
		if err := json.Unmarshal(raw, &metadata); err != nil {
			t.Fatal(err)
		}
		delete(metadata, "generator")
		delete(metadata, "allow_cheats")
		if backend == "" {
			delete(metadata, "backend")
		} else {
			metadata["backend"] = backend
		}
		raw, _ = json.Marshal(metadata)
		if err := os.WriteFile(filepath.Join(dir, metaFile), raw, 0600); err != nil {
			t.Fatal(err)
		}
		store.SetDefaultBackend(BackendBDS)
		saved, err := store.Get(world.ID)
		expectedBackend, expectedGenerator := BackendDragonfly, GeneratorFlat
		if backend == BackendBDS {
			expectedBackend, expectedGenerator = BackendBDS, GeneratorNormal
		}
		if err != nil || saved.Backend != expectedBackend || saved.Generator != expectedGenerator || saved.AllowCheats != (expectedBackend == BackendDragonfly) {
			t.Fatalf("legacy %q = %+v, %v", backend, saved, err)
		}
	}
}

func TestCheatsPersistAndReachBothBDSRuntimes(t *testing.T) {
	for _, enabled := range []bool{false, true} {
		permission := permissionMember
		if enabled {
			permission = permissionOperator
		}
		store := newTestStore(t)
		world, err := store.Create(Spec{Name: "Commands", Backend: BackendBDS, Generator: GeneratorFlat, AllowCheats: enabled})
		if err != nil {
			t.Fatal(err)
		}
		saved, err := store.Get(world.ID)
		if err != nil || saved.AllowCheats != enabled {
			t.Fatalf("cheats = %+v, %v", saved, err)
		}
		spec := StartSpec{World: saved}
		props := string(serverProperties(spec, 19132, 1, false))
		if !strings.Contains(props, "default-player-permission-level="+permission+"\n") || !strings.Contains(props, "allow-cheats="+strconv.FormatBool(enabled)+"\n") || !strings.Contains(props, "level-type=FLAT\n") {
			t.Fatalf("incorrect properties: %s", props)
		}
		args := strings.Join(containerArgs(spec, testImage, "fixture-version", t.TempDir(), 19132, 1, false, 0), " ")
		if !strings.Contains(args, "DEFAULT_PLAYER_PERMISSION_LEVEL="+permission) || !strings.Contains(args, "ALLOW_CHEATS="+strconv.FormatBool(enabled)) || !strings.Contains(args, "LEVEL_TYPE=FLAT") {
			t.Fatal("container lost world settings")
		}
	}
}

func TestEverySupportedServerTerrainCombinationStaysExplicit(t *testing.T) {
	store := newTestStore(t)
	for _, backend := range []string{BackendDragonfly, BackendBDS} {
		for _, generator := range []string{GeneratorNormal, GeneratorFlat} {
			saved, err := store.Create(Spec{Name: "Matrix", Backend: backend, Generator: generator})
			if err != nil || saved.Backend != backend || saved.Generator != generator {
				t.Fatalf("%s/%s = %+v, %v", backend, generator, saved, err)
			}
		}
		if _, err := store.Create(Spec{Name: "Unsupported", Backend: backend, Generator: "void"}); !errors.Is(err, ErrInvalid) {
			t.Fatalf("%s accepted unsupported Void: %v", backend, err)
		}
	}
}

// countedRuntimeWait detects unnecessary waits without using a timing assertion.
type countedRuntimeWait struct {
	*Provisioner
	calls int
}

// AwaitRuntime reports a pending probe and records whether creation consulted it.
func (s *countedRuntimeWait) AwaitRuntime(context.Context) error {
	s.calls++
	return context.DeadlineExceeded
}

func TestDefaultDragonflyCreationDoesNotConsultBDSDetection(t *testing.T) {
	store := newTestStore(t)
	store.SetDefaultBackend(BackendDragonfly)
	setup := &countedRuntimeWait{Provisioner: &Provisioner{Root: t.TempDir()}}
	manager := NewManager(store, Runners{}, nil)
	manager.SetSetup(setup)
	world, err := manager.Create(Spec{Name: "Default server"})
	if err != nil || world.Backend != BackendDragonfly || setup.calls != 0 {
		t.Fatalf("default creation = %+v, %v; runtime waits = %d", world, err, setup.calls)
	}
}
