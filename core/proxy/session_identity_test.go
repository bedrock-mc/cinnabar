package proxy

import (
	"encoding/json"
	"errors"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

func TestSessionDownstreamInitializesSelfSignedIDFromCoreDevice(t *testing.T) {
	t.Parallel()
	profile := device.Profile{
		OS: protocol.DeviceAndroid, Model: device.Models(protocol.DeviceAndroid)[0],
		ID: "00112233445566778899aabbccddeeff",
	}
	for _, test := range []struct {
		name, incoming string
		absent         bool
	}{
		{name: "absent", absent: true},
		{name: "empty"},
		{name: "first join", incoming: "00000000-0000-4000-8000-000000000001"},
		{name: "next join", incoming: "00000000-0000-4000-8000-000000000002"},
	} {
		t.Run(test.name, func(t *testing.T) {
			request := testSessionConnect(t)
			var input login.ClientData
			if err := json.Unmarshal(request.ClientData, &input); err != nil {
				t.Fatal(err)
			}
			input.ClientRandomID = 0x0102030405060708
			input.SelfSignedID = test.incoming
			input.DeviceID = "ffeeddccbbaa99887766554433221100"
			input.PlatformOfflineID = "00000000-0000-4000-8000-000000000003"
			input.PlatformOnlineID = "123456789"
			input.PlayFabID = "0123456789abcdef"
			input.CapeID = "fixture-cape"
			input.SkinID = "fixture-skin.Custom"
			input.SkinGeometry = "e30="
			input.ExtraData = map[string]json.RawMessage{"FixtureClaim": json.RawMessage(`{"value":7}`)}
			input.MarkClaims("ClientRandomId", "SelfSignedId", "DeviceId", "PlatformOfflineId", "PlatformOnlineId",
				"PlayFabId", "CapeId", "SkinId", "SkinGeometryData")
			encoded, err := json.Marshal(input)
			if err != nil {
				t.Fatal(err)
			}
			if test.absent {
				var fields map[string]json.RawMessage
				if err := json.Unmarshal(encoded, &fields); err != nil {
					t.Fatal(err)
				}
				delete(fields, "SelfSignedId")
				encoded, err = json.Marshal(fields)
				if err != nil {
					t.Fatal(err)
				}
			}
			request.ClientData = encoded
			if err := json.Unmarshal(encoded, &input); err != nil {
				t.Fatal(err)
			}
			want := input
			profile.Apply(&want)
			want.SelfSignedID = "838a88a9-913f-32d3-8ab3-a5dc55cc55a0"
			want.MarkClaims("SelfSignedId")
			wantJSON, err := json.Marshal(want)
			if err != nil {
				t.Fatal(err)
			}
			// Recreating the downstream also recreates its login state.
			for range 2 {
				downstream, err := newSessionDownstream(request, &profile)
				if err != nil {
					t.Fatal(err)
				}
				gotJSON, err := json.Marshal(downstream.ClientData())
				if err != nil {
					t.Fatal(err)
				}
				if string(gotJSON) != string(wantJSON) {
					t.Fatalf("login claims differ: SelfSignedId = %q, want %q",
						downstream.ClientData().SelfSignedID, want.SelfSignedID)
				}
			}
		})
	}
}

func TestSessionDownstreamKeepsCallerIdentityWithoutCoreDevice(t *testing.T) {
	t.Parallel()
	request := testSessionConnect(t)
	var input login.ClientData
	if err := json.Unmarshal(request.ClientData, &input); err != nil {
		t.Fatal(err)
	}
	input.SelfSignedID = "00000000-0000-4000-8000-000000000001"
	input.DeviceID = "caller-device"
	input.MarkClaims("SelfSignedId", "DeviceId")
	request.ClientData, _ = json.Marshal(input)
	downstream, err := newSessionDownstream(request, nil)
	if err != nil {
		t.Fatal(err)
	}
	got, err := json.Marshal(downstream.ClientData())
	if err != nil {
		t.Fatal(err)
	}
	if string(got) != string(request.ClientData) {
		t.Fatal("caller-provided login claims changed")
	}
}

func TestSessionDownstreamRejectsMissingCoreDeviceID(t *testing.T) {
	t.Parallel()
	profile := device.New(protocol.DeviceAndroid)
	profile.ID = ""
	if _, err := newSessionDownstream(testSessionConnect(t), &profile); !errors.Is(err, errMalformedSessionMessage) {
		t.Fatalf("missing core device ID: %v", err)
	}
}
