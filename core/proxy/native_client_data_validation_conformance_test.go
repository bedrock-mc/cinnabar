package proxy

import (
	"encoding/json"
	"errors"
	"os"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// TestNativeClientDataValidationConformance runs shared edits through the real session admission
// and refusal owners. Successful metadata does not authorize an account or upstream connection.
func TestNativeClientDataValidationConformance(t *testing.T) {
	data, err := os.ReadFile("../../crates/protocol/tests/fixtures/native-client-data-validation.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name          string                     `json:"name"`
		Edits         map[string]json.RawMessage `json:"edits"`
		ProtocolDelta int32                      `json:"protocol_delta"`
		Outcome       string                     `json:"outcome"`
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	claimed := device.Profile{
		OS: protocol.DeviceAndroid, Model: bridgecontract.NativeAccountDefaultModel,
		ID: "00112233445566778899aabbccddeeff",
	}
	for _, test := range cases {
		t.Run(test.Name, func(t *testing.T) {
			request := testSessionConnect(t)
			var fields map[string]json.RawMessage
			if err := json.Unmarshal(request.ClientData, &fields); err != nil {
				t.Fatal(err)
			}
			// A numeric fixture address keeps validation independent of external DNS.
			fields["ServerAddress"] = json.RawMessage(`"127.0.0.1:19132"`)
			for name, value := range test.Edits {
				fields[name] = value
			}
			request.ClientData, err = json.Marshal(fields)
			if err != nil {
				t.Fatal(err)
			}
			request.Protocol += test.ProtocolDelta
			downstream, err := newSessionDownstream(request, &claimed)
			if outcome := nativeClientDataOutcome(t, err); outcome != test.Outcome {
				t.Fatalf("actual session admission returned %q, want %q", outcome, test.Outcome)
			}
			if err == nil {
				actual := downstream.ClientData()
				if actual.DeviceOS != claimed.OS || actual.DeviceModel != claimed.Model || actual.DeviceID != claimed.ID {
					t.Fatal("admission did not apply the owned device profile")
				}
				if actual.SelfSignedID != "" {
					t.Fatal("claimed-device admission must leave identity derivation to the upstream owner")
				}
			}
		})
	}
}

// nativeClientDataOutcome projects only the actual refusal key, never diagnostic text.
func nativeClientDataOutcome(t *testing.T, err error) string {
	t.Helper()
	if err == nil {
		return "accepted"
	}
	var refused *sessionRefusedError
	if !errors.As(err, &refused) {
		return "generic"
	}
	switch refused.key {
	case bridgecontract.DisconnectKeyOutdatedClient:
		return "outdated_client"
	case bridgecontract.DisconnectKeyOutdatedServer:
		return "outdated_server"
	case bridgecontract.DisconnectKeyInvalidSkin:
		return "invalid_skin"
	case bridgecontract.DisconnectKeyInvalidName:
		return "invalid_name"
	case bridgecontract.DisconnectKeyCantConnect:
		return "generic"
	default:
		t.Fatalf("session admission returned an unclassified refusal key %q", refused.key)
		return ""
	}
}
