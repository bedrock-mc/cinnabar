package control

import (
	"encoding/json"
	"github.com/hashimthearab/rust-mcbe/core/xboxpresence"
	"testing"
)

func TestPresenceStateAcceptedWithoutAccount(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":1,"method":"presence.v1","params":{"in_world":true,"game_mode":1,"realm":false}}`))
	var answer struct {
		Error  *responseError  `json:"error"`
		Result json.RawMessage `json:"result"`
	}
	if err := json.Unmarshal(payload, &answer); err != nil {
		t.Fatal(err)
	}
	if answer.Error != nil || len(answer.Result) == 0 {
		t.Fatalf("presence state rejected: %s", payload)
	}
}

func TestPresenceControlForwardsStateAndRejectsMalformedInput(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	defer server.Close()
	states := make(chan xboxpresence.State, 2)
	server.SetPresence(func(state xboxpresence.State) { states <- state })
	payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":1,"method":"presence.v1","params":{"in_world":true,"game_mode":2,"realm":true}}`))
	var answer struct {
		Error *responseError `json:"error"`
	}
	if err := json.Unmarshal(payload, &answer); err != nil || answer.Error != nil {
		t.Fatalf("presence: %s", payload)
	}
	state := <-states
	if !state.InWorld || state.GameMode != 2 || !state.Realm {
		t.Fatalf("wrong state: %+v", state)
	}
	for _, params := range []string{`{"in_world":true,"experience":"invalid"}`, `{"in_world":true,"unknown":true}`} {
		payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":2,"method":"presence.v1","params":`+params+`}`))
		if err := json.Unmarshal(payload, &answer); err != nil || answer.Error == nil || answer.Error.Code != -32602 {
			t.Fatalf("malformed state accepted: %s", payload)
		}
	}
}
