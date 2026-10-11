package authcache

import (
	"encoding/json"
	"os"
	"path/filepath"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// DeviceOS is the platform this core signs in as, the platform of auth.AndroidConfig's title.
const DeviceOS = protocol.DeviceAndroid

// maxDeviceProfileBytes bounds the saved profile, a few short JSON strings.
const maxDeviceProfileBytes = 4 << 10

// deviceLeaseTimeout bounds the wait for another core initializing the same profile.
const deviceLeaseTimeout = 10 * time.Second

// LoadDevice returns the install's device profile saved at path, replacing one that is missing,
// unreadable or for another platform with a new saved profile. Cores sharing path hold a lease
// across that, so they all claim one device. An empty path keeps a new profile in memory only. The
// error reports a profile that could not be saved; the profile is still usable.
func LoadDevice(path string) (device.Profile, error) {
	if path == "" {
		return device.New(DeviceOS), nil
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return device.New(DeviceOS), err
	}
	lease, err := lockfile.Acquire(path+".lock", deviceLeaseTimeout)
	if err != nil {
		if profile, ok := readDevice(path); ok {
			return profile, nil
		}
		return device.New(DeviceOS), err
	}
	defer lease.Close()
	if profile, ok := readDevice(path); ok {
		return profile, nil
	}
	profile := device.New(DeviceOS)
	serialized, err := json.Marshal(profile)
	if err != nil {
		return profile, err
	}
	return profile, savePrivate(path, serialized)
}

// readDevice returns the saved profile when it is readable, valid and for DeviceOS.
func readDevice(path string) (device.Profile, bool) {
	saved, err := loadPrivate(path, maxDeviceProfileBytes)
	if err != nil {
		return device.Profile{}, false
	}
	var profile device.Profile
	if json.Unmarshal(saved, &profile) != nil || profile.OS != DeviceOS || !profile.Valid() {
		return device.Profile{}, false
	}
	return profile, true
}
