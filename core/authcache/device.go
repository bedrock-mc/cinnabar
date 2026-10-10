package authcache

import (
	"encoding/json"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// DeviceOS is the platform this core signs in as, the platform of auth.AndroidConfig's title.
const DeviceOS = protocol.DeviceAndroid

// maxDeviceProfileBytes bounds the saved profile, a few short JSON strings.
const maxDeviceProfileBytes = 4 << 10

// LoadDevice returns the install's device profile saved at path, replacing one that is missing,
// unreadable or for another platform with a new saved profile. An empty path keeps a new profile in
// memory only. The error reports a profile that could not be saved; the profile is still usable.
func LoadDevice(path string) (device.Profile, error) {
	if path != "" {
		// A missing, unreadable or corrupt profile is replaced alike.
		if saved, err := loadPrivate(path, maxDeviceProfileBytes); err == nil {
			var profile device.Profile
			if json.Unmarshal(saved, &profile) == nil && profile.OS == DeviceOS && profile.Valid() {
				return profile, nil
			}
		}
	}
	profile := device.New(DeviceOS)
	if path == "" {
		return profile, nil
	}
	serialized, err := json.Marshal(profile)
	if err != nil {
		return profile, err
	}
	return profile, savePrivate(path, serialized)
}
