package proxy

import (
	"encoding/base64"
	"encoding/json"
	"net"
	"os"
	"strconv"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

// TestNativeClientDataMetadataLimits qualifies the same two inputs through actual session admission.
func TestNativeClientDataMetadataLimits(t *testing.T) {
	data, err := os.ReadFile("../../crates/protocol/tests/fixtures/native-client-data-limits.json")
	if err != nil {
		t.Fatal(err)
	}
	var inputs struct {
		BracketedPort   string `json:"bracketed_port"`
		DecodedGeometry string `json:"decoded_geometry"`
	}
	if err := json.Unmarshal(data, &inputs); err != nil {
		t.Fatal(err)
	}
	t.Run("port-bracket-before-lookup", func(t *testing.T) {
		request, profile := nativeClientDataLimitsBaseline(t)
		if _, err := newSessionDownstream(request, &profile); err != nil {
			t.Fatalf("valid numeric baseline refused: %v", err)
		}
		request.ClientData = nativeClientDataLimitsEdit(t, request.ClientData, "ServerAddress", inputs.BracketedPort)
		_, err := newSessionDownstream(request, &profile)
		if outcome := nativeClientDataOutcome(t, err); outcome != "generic" {
			t.Fatalf("bracket in port returned %q, want generic", outcome)
		}
	})
	t.Run("decoded-object-depth", func(t *testing.T) {
		request, profile := nativeClientDataLimitsBaseline(t)
		if _, err := newSessionDownstream(request, &profile); err != nil {
			t.Fatalf("valid numeric baseline refused: %v", err)
		}
		encoded := base64.StdEncoding.EncodeToString([]byte(inputs.DecodedGeometry))
		request.ClientData = nativeClientDataLimitsEdit(t, request.ClientData, "SkinGeometryData", encoded)
		if _, err := newSessionDownstream(request, &profile); err != nil {
			t.Fatalf("valid bounded decoded object refused: %v", err)
		}
	})
}

// nativeClientDataLimitsBaseline uses the real builder and numeric metadata, without DNS or dialing.
func nativeClientDataLimitsBaseline(t *testing.T) (sessionConnectRequest, device.Profile) {
	t.Helper()
	request := testSessionConnect(t)
	address := net.JoinHostPort("127.0.0.1", strconv.Itoa(bridgecontract.DefaultBedrockPort))
	request.ClientData = nativeClientDataLimitsEdit(t, request.ClientData, "ServerAddress", address)
	return request, device.Profile{
		OS: protocol.DeviceAndroid, Model: bridgecontract.NativeAccountDefaultModel,
		ID: "00112233445566778899aabbccddeeff",
	}
}

// nativeClientDataLimitsEdit changes one actual known string claim, keeping the real baseline skin.
func nativeClientDataLimitsEdit(t *testing.T, data json.RawMessage, name, value string) json.RawMessage {
	t.Helper()
	var fields map[string]json.RawMessage
	if err := json.Unmarshal(data, &fields); err != nil {
		t.Fatal(err)
	}
	encoded, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	fields[name] = encoded
	result, err := json.Marshal(fields)
	if err != nil {
		t.Fatal(err)
	}
	return result
}
