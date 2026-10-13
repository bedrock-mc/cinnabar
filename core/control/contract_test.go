package control

import (
	"encoding/json"
	"os"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
)

func TestEveryManifestControlMethodReachesItsHandler(t *testing.T) {
	data, err := os.ReadFile("../../crates/bridge/data/contract.json")
	if err != nil {
		t.Fatal(err)
	}
	var manifest struct {
		Constants []struct {
			Name  string          `json:"rust_name"`
			Value json.RawMessage `json:"value"`
		} `json:"constants"`
	}
	if err := json.Unmarshal(data, &manifest); err != nil {
		t.Fatal(err)
	}
	dir := t.TempDir()
	server, err := StartWithWorlds(dir, NewStore(), &stubWorlds{})
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	server.SetPacketDelay(new(fakePacketDelay))
	methods := 0
	for _, row := range manifest.Constants {
		if !strings.HasPrefix(row.Name, "METHOD_") {
			continue
		}
		methods++
		var method string
		if err := json.Unmarshal(row.Value, &method); err != nil {
			t.Fatal(err)
		}
		t.Run(method, func(t *testing.T) {
			payload, err := json.Marshal(map[string]any{
				"jsonrpc": bridgecontract.JSONRPCVersion, "id": 1, "method": method,
			})
			if err != nil {
				t.Fatal(err)
			}
			var reply response
			if err := json.Unmarshal(exchange(t, dir, payload), &reply); err != nil {
				t.Fatal(err)
			}
			if reply.JSONRPC != bridgecontract.JSONRPCVersion ||
				(reply.Error != nil && reply.Error.Code == bridgecontract.RPCMethodNotFound) {
				t.Fatalf("manifest method was not dispatched: %+v", reply)
			}
		})
	}
	if methods == 0 {
		t.Fatal("contract has no control methods")
	}
}

func TestEverySharedMessageEventReachesTheService(t *testing.T) {
	screens := &recordingScreens{}
	dir := startServices(t, NewStore(), screens)
	for _, event := range bridgecontract.MessageEventTypes {
		params, err := json.Marshal(map[string]string{"event_type": event})
		if err != nil {
			t.Fatal(err)
		}
		if reply := rpc(t, dir, bridgecontract.MethodMessageEvent, string(params)); reply.Error != nil {
			t.Fatalf("contract event %q was rejected: %+v", event, reply.Error)
		}
	}
	if len(screens.events) != len(bridgecontract.MessageEventTypes) {
		t.Fatalf("forwarded %d of %d events", len(screens.events), len(bridgecontract.MessageEventTypes))
	}
	for index, event := range screens.events {
		if event.Type != bridgecontract.MessageEventTypes[index] {
			t.Fatalf("event %d = %q, want %q", index, event.Type, bridgecontract.MessageEventTypes[index])
		}
	}
}
