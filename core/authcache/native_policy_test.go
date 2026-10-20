package authcache

import (
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// TestNativeAccountPolicyMatchesGoOwners rejects drift in the policy consumed by both Rust account owners.
func TestNativeAccountPolicyMatchesGoOwners(t *testing.T) {
	os := protocol.DeviceOS(bridgecontract.NativeAccountDeviceOS)
	config, ok := device.AuthConfig(os)
	if !ok || os != DeviceOS || config.ClientID != bridgecontract.NativeAuthClientID ||
		config.UserAgent != bridgecontract.NativeAuthUserAgent || config.Device.Version != bridgecontract.NativeAuthDeviceVersion ||
		config.Sandbox != bridgecontract.NativeAuthSandbox {
		t.Fatal("shared native account policy differs from the actual Go Android owner")
	}
	profile := device.New(os)
	profile.Model = bridgecontract.NativeAccountDefaultModel
	if !profile.Valid() {
		t.Fatal("shared native default model or device identifier is incompatible with the actual Go profile")
	}
	var claims login.ClientData
	profile.Apply(&claims)
	if claims.DefaultInputMode != bridgecontract.NativeAccountInputMode || int(claims.PlatformType) != bridgecontract.NativeAccountPlatformType {
		t.Fatal("shared native metadata policy differs from the actual Go device projection")
	}
}
