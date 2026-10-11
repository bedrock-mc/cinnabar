package proxy

import (
	"context"
	"encoding/json"
	"errors"
	"math"
	"testing"
	"time"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestSessionDialerDerivesSelfSignedIDFromCoreDevice(t *testing.T) {
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
			want.SelfSignedID = ""
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
				assertSessionDialerSelfSignedID(t, downstream, "838a88a9-913f-32d3-8ab3-a5dc55cc55a0")
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
	if _, err := newSessionDownstream(testSessionConnect(t), &profile); !errors.Is(err, errMalformedSession) {
		t.Fatalf("missing core device ID: %v", err)
	}
}

// assertSessionDialerSelfSignedID checks the upstream login produced from a session's actual client data.
func assertSessionDialerSelfSignedID(t *testing.T, downstream *sessionDownstream, want string) {
	t.Helper()
	listener, network := newAdmissionTestListener(t, nil)
	done := make(chan error, 1)
	go func() {
		conn, err := listener.Accept()
		if err == nil {
			upstream := conn.(*minecraft.Conn)
			defer upstream.Close()
			if upstream.ClientData().SelfSignedID != want {
				err = errors.New("upstream SelfSignedId differs from the regression vector")
			} else {
				err = upstream.WritePacketImmediate(&packet.StartGame{EntityRuntimeID: 9})
				if err == nil {
					_, _ = upstream.ReadPacket()
				}
			}
		}
		done <- err
	}()
	ctx, cancel := context.WithTimeout(t.Context(), 5*time.Second)
	defer cancel()
	dialer := newUpstreamDialerForAdmission(downstream, nil, nil, nil, false)
	conn, err := dialer.DialContextNetwork(ctx, network, "")
	if err != nil {
		_ = listener.Close()
		t.Fatalf("dial: %v; server: %v", err, <-done)
	}
	got := conn.ClientData()
	_ = conn.Close()
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	if got.SelfSignedID != want || got.ClientRandomID != downstream.ClientData().ClientRandomID || got.DeviceID != downstream.ClientData().DeviceID {
		t.Fatal("upstream login changed the session identity inputs or derived a different SelfSignedId")
	}
}

func TestSessionDialerSelfSignedIDVectors(t *testing.T) {
	for _, test := range []struct {
		name string
		id   int64
		want string
	}{
		{name: "minimum", id: math.MinInt64, want: "5edb9ba0-4706-3dd7-a52b-24e7790f4d98"},
		{name: "maximum", id: math.MaxInt64, want: "8c2bc487-6daa-3b9b-8116-dfda6d1a32ce"},
		{name: "negative", id: -1, want: "3e6f158c-edd9-3059-b123-396df53ce814"},
		{name: "zero", id: 0, want: "6e6069e0-2a0c-3fca-97bd-72c46440d781"},
	} {
		t.Run(test.name, func(t *testing.T) {
			request := testSessionConnect(t)
			var input login.ClientData
			if err := json.Unmarshal(request.ClientData, &input); err != nil {
				t.Fatal(err)
			}
			input.ClientRandomID = test.id
			input.MarkClaims("ClientRandomId")
			var err error
			request.ClientData, err = json.Marshal(input)
			if err != nil {
				t.Fatal(err)
			}
			profile := device.New(protocol.DeviceAndroid)
			profile.ID = "audit-device-id"
			downstream, err := newSessionDownstream(request, &profile)
			if err != nil {
				t.Fatal(err)
			}
			assertSessionDialerSelfSignedID(t, downstream, test.want)
		})
	}
}
