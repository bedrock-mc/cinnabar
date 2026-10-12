package proxy

import (
	"encoding/json"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// Session claims and the upstream login must carry the persisted Android profile's mobile category.
func TestSessionLoginClaimsAndroidPlatformType(t *testing.T) {
	t.Parallel()
	profile := device.Profile{
		OS:    protocol.DeviceAndroid,
		Model: device.Models(protocol.DeviceAndroid)[0],
		ID:    "00112233445566778899aabbccddeeff",
	}
	for _, test := range []struct {
		name     string
		platform json.RawMessage
		want     int
	}{
		{name: "absent platform", want: login.PlatformTypeMobile},
		{name: "desktop zero", platform: json.RawMessage(`0`), want: login.PlatformTypeMobile},
		{name: "mobile platform", platform: json.RawMessage(`1`), want: login.PlatformTypeMobile},
		{name: "unknown nonzero", platform: json.RawMessage(`7`), want: 7},
	} {
		t.Run(test.name, func(t *testing.T) {
			request := testSessionConnect(t)
			claims := map[string]json.RawMessage{}
			if err := json.Unmarshal(request.ClientData, &claims); err != nil {
				t.Fatal(err)
			}
			delete(claims, "DeviceOS")
			delete(claims, "PlatformType")
			if test.platform != nil {
				claims["PlatformType"] = test.platform
			}
			claims["ClientRandomId"] = json.RawMessage(`-7`)
			claims["FixtureClaim"] = json.RawMessage(`"kept"`)
			var err error
			request.ClientData, err = json.Marshal(claims)
			if err != nil {
				t.Fatal(err)
			}
			downstream, err := newSessionDownstream(request, &profile)
			if err != nil {
				t.Fatal(err)
			}
			encoded, err := json.Marshal(downstream.ClientData())
			if err != nil {
				t.Fatal(err)
			}
			var data login.ClientData
			if err := json.Unmarshal(encoded, &data); err != nil {
				t.Fatal(err)
			}
			if data.PlatformType != test.want {
				t.Fatalf("serialized PlatformType = %d, want %d", data.PlatformType, test.want)
			}
			if data.DeviceOS != profile.OS || data.DeviceID != profile.ID || data.DeviceModel != profile.Model {
				t.Fatal("session login changed the persisted device profile")
			}
			if data.ClientRandomID != -7 || data.SkinID != "skin" || string(data.ExtraData["FixtureClaim"]) != `"kept"` {
				t.Fatal("session login changed caller identity, skin or unknown claims")
			}
			wantID := login.DeriveSelfSignedID(data.ClientRandomID, string(profile.ID))
			assertSessionDialerSelfSignedID(t, downstream, wantID)
		})
	}
}
