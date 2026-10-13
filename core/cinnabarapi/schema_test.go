package cinnabarapi

import (
	"bytes"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	_ "embed"
	"encoding/json"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

// Pinned from bedrock-mc/cinnabar-api schema/v1.json at main 57d126b34 (x-protocol 1).
//
//go:embed schema/v1.json
var pinnedSchema []byte

func compileDefinition(t *testing.T, name string) *jsonschema.Schema {
	t.Helper()
	document, err := jsonschema.UnmarshalJSON(bytes.NewReader(pinnedSchema))
	if err != nil {
		t.Fatal(err)
	}
	compiler := jsonschema.NewCompiler()
	if err := compiler.AddResource("v1.json", document); err != nil {
		t.Fatal(err)
	}
	schema, err := compiler.Compile("v1.json#/$defs/" + name)
	if err != nil {
		t.Fatal(err)
	}
	return schema
}

func validate(t *testing.T, definition string, value any) {
	t.Helper()
	payload, err := json.Marshal(value)
	if err != nil {
		t.Fatal(err)
	}
	instance, err := jsonschema.UnmarshalJSON(bytes.NewReader(payload))
	if err != nil {
		t.Fatal(err)
	}
	if err := compileDefinition(t, definition).Validate(instance); err != nil {
		t.Fatalf("%s %s: %v", definition, payload, err)
	}
}

// Every message the client encodes must conform to the API's published schema.
func TestClientMessagesConformToPinnedSchema(t *testing.T) {
	key, err := ecdsa.GenerateKey(elliptic.P384(), rand.Reader)
	if err != nil {
		t.Fatal(err)
	}
	signature, err := sign(key, strings.Repeat("A", 43))
	if err != nil {
		t.Fatal(err)
	}
	xuids := make([]string, bridgecontract.CinnabarAPIMaxLookupXUIDs)
	for i := range xuids {
		xuids[i] = "2535400000000000"
	}
	for _, message := range []any{
		helloMessage{Type: typeHello, Protocol: protocolVersion, ClientVersion: "0.1.18", Channel: "nightly", Visible: true, Token: "aGVhZGVy.cGF5bG9hZA.c2ln"},
		proofMessage{Type: typeProof, Signature: signature},
		visibilityMessage{Type: typeVisibility, Visible: false},
		lookupMessage{Type: typeLookup, ID: 4294967295, XUIDs: xuids},
	} {
		validate(t, "ClientMessage", message)
	}
}

// Schema-valid server messages decode into the fields the client reads.
func TestPinnedServerMessagesDecode(t *testing.T) {
	id := uint32(7)
	for _, test := range []struct {
		raw  string
		want inbound
	}{
		{`{"type":"challenge","nonce":"` + strings.Repeat("A", 43) + `"}`, inbound{Type: typeChallenge, Nonce: strings.Repeat("A", 43)}},
		{`{"type":"welcome","xuid":"2535400000000000","display_name":"Player"}`, inbound{Type: typeWelcome}},
		{`{"type":"lookup_result","id":7,"cinnabar":["1"]}`, inbound{Type: typeLookupResult, ID: &id, Cinnabar: []string{"1"}}},
		{`{"type":"error","code":"rate_limited","id":7}`, inbound{Type: typeError, Code: codeRateLimited, ID: &id}},
		{`{"type":"release","channel":"beta","version":"0.2.0","published_at":"2026-10-13T00:00:00.000Z"}`, inbound{Type: typeRelease, Channel: "beta", Version: "0.2.0"}},
	} {
		var value any
		if err := json.Unmarshal([]byte(test.raw), &value); err != nil {
			t.Fatal(err)
		}
		validate(t, "ServerMessage", value)
		var got inbound
		if err := json.Unmarshal([]byte(test.raw), &got); err != nil {
			t.Fatal(err)
		}
		gotJSON, _ := json.Marshal(got)
		wantJSON, _ := json.Marshal(test.want)
		if !bytes.Equal(gotJSON, wantJSON) {
			t.Fatalf("%s decoded as %s", test.raw, gotJSON)
		}
	}
}

// The release hook body the package workflow posts conforms too.
func TestReleaseHookBodyConforms(t *testing.T) {
	validate(t, "ReleaseHookRequest", map[string]string{"channel": "beta", "version": "0.1.18"})
}
