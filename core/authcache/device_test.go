package authcache

import (
	"os"
	"path/filepath"
	"sync"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/auth"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// The claimed device is the platform of the Xbox title the core signs in as.
func TestDeviceOSMatchesTheSignInTitle(t *testing.T) {
	config, ok := device.AuthConfig(DeviceOS)
	if !ok || config.TitleID != auth.AndroidConfig.TitleID {
		t.Fatalf("device OS %v signs in as title %v, the core as %v", DeviceOS, config.TitleID, auth.AndroidConfig.TitleID)
	}
}

// One install keeps one device across launches; a profile for another platform is replaced.
func TestLoadDeviceKeepsOneProfilePerInstall(t *testing.T) {
	path := filepath.Join(t.TempDir(), "device.json")
	first, err := LoadDevice(path)
	if err != nil || first.OS != DeviceOS || !first.Valid() {
		t.Fatalf("first profile = %+v, %v", first, err)
	}
	again, err := LoadDevice(path)
	if err != nil || again != first {
		t.Fatalf("reloaded profile = %+v, %v; want %+v", again, err, first)
	}
	if err := os.WriteFile(path, []byte(`{"os":7,"model":"XboxOne","id":"x"}`), 0o600); err != nil {
		t.Fatal(err)
	}
	replaced, err := LoadDevice(path)
	if err != nil || replaced.OS != protocol.DeviceAndroid || !replaced.Valid() {
		t.Fatalf("replaced profile = %+v, %v", replaced, err)
	}
	if memory, err := LoadDevice(""); err != nil || !memory.Valid() {
		t.Fatalf("in-memory profile = %+v, %v", memory, err)
	}
}

// Cores starting together on a fresh install all claim the one device that is saved.
func TestLoadDeviceAgreesAcrossConcurrentFirstLoads(t *testing.T) {
	path := filepath.Join(t.TempDir(), "device.json")
	profiles := make(chan device.Profile, 16)
	var started sync.WaitGroup
	for range cap(profiles) {
		started.Add(1)
		go func() {
			defer started.Done()
			profile, err := LoadDevice(path)
			if err != nil {
				t.Error(err)
			}
			profiles <- profile
		}()
	}
	started.Wait()
	close(profiles)
	saved, err := LoadDevice(path)
	if err != nil {
		t.Fatal(err)
	}
	for profile := range profiles {
		if profile != saved {
			t.Fatalf("a core claimed %+v while %+v was saved", profile, saved)
		}
	}
}
