package proxy

import (
	"encoding/json"
	"os"
	"testing"

	"github.com/sandertv/gophertunnel/minecraft/protocol/login"
)

// TestNativeClientDataRawConformance checks only the actual decoding/opaque-value boundary.
// These partial claim objects deliberately do not approve title metadata or an account login.
func TestNativeClientDataRawConformance(t *testing.T) {
	data, err := os.ReadFile("../../crates/protocol/tests/fixtures/native-client-data.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name         string          `json:"name"`
		ClientData   json.RawMessage `json:"client_data"`
		RandomID     int64           `json:"random_id"`
		DisplayName  string          `json:"display_name"`
		SelfSignedID string          `json:"self_signed_id"`
		Opaque       string          `json:"opaque"`
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, test := range cases {
		t.Run(test.Name, func(t *testing.T) {
			var client login.ClientData
			if err := json.Unmarshal(test.ClientData, &client); err != nil {
				t.Fatal(err)
			}
			if client.ClientRandomID != test.RandomID || client.ThirdPartyName != test.DisplayName || client.SelfSignedID != test.SelfSignedID {
				t.Fatal("actual client-data decoder changed the selected fields")
			}
			if string(client.ExtraData["Unknown"]) != test.Opaque {
				t.Fatal("actual client-data decoder changed opaque value representation")
			}
		})
	}
}
