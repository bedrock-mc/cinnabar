package proxy

import (
	"bytes"
	"encoding/json"
	"net"
	"os"
	"reflect"
	"strconv"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/sandertv/gophertunnel/minecraft/device"
	"github.com/sandertv/gophertunnel/minecraft/protocol"
)

type nativeClientDataEntry struct {
	Name  string          `json:"name"`
	Value json.RawMessage `json:"value"`
}

// TestNativeClientDataOrderedConformance preserves duplicate/folded entries through real admission.
func TestNativeClientDataOrderedConformance(t *testing.T) {
	data, err := os.ReadFile("../../crates/protocol/tests/fixtures/native-client-data-decoder.json")
	if err != nil {
		t.Fatal(err)
	}
	cases := []struct {
		Name          string                     `json:"name"`
		Entries       []nativeClientDataEntry    `json:"entries"`
		ProtocolDelta int32                      `json:"protocol_delta"`
		Outcome       string                     `json:"outcome"`
		Observed      map[string]json.RawMessage `json:"observed"`
	}{}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	claimed := device.Profile{
		OS: protocol.DeviceAndroid, Model: bridgecontract.NativeAccountDefaultModel,
		ID: "ffeeddccbbaa99887766554433221100",
	}
	for _, test := range cases {
		t.Run(test.Name, func(t *testing.T) {
			request := testSessionConnect(t)
			request.ClientData = nativeClientDataEntries(t, request.ClientData, test.Entries)
			request.Protocol += test.ProtocolDelta
			downstream, err := newSessionDownstream(request, &claimed)
			outcome := nativeClientDataOutcome(t, err)
			t.Logf("actual session outcome: %s", outcome)
			if outcome != test.Outcome {
				t.Fatalf("actual session admission returned %q, want %q", outcome, test.Outcome)
			}
			if len(test.Observed) == 0 {
				return
			}
			if err != nil {
				t.Fatal("refused metadata has no successful typed projection")
			}
			encoded, err := json.Marshal(downstream.ClientData())
			if err != nil {
				t.Fatal(err)
			}
			actual := map[string]json.RawMessage{}
			if err := json.Unmarshal(encoded, &actual); err != nil {
				t.Fatal(err)
			}
			for name, wanted := range test.Observed {
				value, ok := actual[name]
				if !ok {
					t.Fatalf("actual typed projection omitted %q", name)
				}
				if !reflect.DeepEqual(nativeClientDataValue(t, value), nativeClientDataValue(t, wanted)) {
					t.Fatalf("actual typed %s is %s, want %s", name, value, wanted)
				}
			}
		})
	}
}

// nativeClientDataEntries appends original raw values after the real builder's baseline object.
func nativeClientDataEntries(t *testing.T, baseline json.RawMessage, entries []nativeClientDataEntry) json.RawMessage {
	t.Helper()
	fields := map[string]json.RawMessage{}
	if err := json.Unmarshal(baseline, &fields); err != nil {
		t.Fatal(err)
	}
	address := net.JoinHostPort("127.0.0.1", strconv.Itoa(bridgecontract.DefaultBedrockPort))
	encodedAddress, err := json.Marshal(address)
	if err != nil {
		t.Fatal(err)
	}
	fields["ServerAddress"] = encodedAddress
	encoded, err := json.Marshal(fields)
	if err != nil {
		t.Fatal(err)
	}
	var object bytes.Buffer
	object.Write(encoded[:len(encoded)-1])
	for _, entry := range entries {
		name, err := json.Marshal(entry.Name)
		if err != nil {
			t.Fatal(err)
		}
		object.WriteByte(',')
		object.Write(name)
		object.WriteByte(':')
		object.Write(entry.Value)
	}
	object.WriteByte('}')
	return object.Bytes()
}

// nativeClientDataValue compares only selected typed output values without float64 rounding.
func nativeClientDataValue(t *testing.T, value json.RawMessage) any {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(value))
	decoder.UseNumber()
	var decoded any
	if err := decoder.Decode(&decoded); err != nil {
		t.Fatal(err)
	}
	return decoded
}
