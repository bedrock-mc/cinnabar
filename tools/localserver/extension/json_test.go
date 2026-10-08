package extension

import (
	"math"
	"reflect"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// hello is a valid Hello body; the cases below change one thing in it.
const hello = `{"version":1,"api":1,"capabilities":["ui"],"offer_digest":"d","client_challenge":"c","connection":"n","subclient":0}`

// Decode refuses what serde_json refuses for the same Rust type, including what encoding/json
// would let through: other key cases, duplicates, missing members, null, floats, negative zero,
// invalid UTF-8 and lone surrogates.
func TestDecodeRejectsWhatSerdeRejects(t *testing.T) {
	envelope := string(fixture(t, "envelope_to_server.json"))
	for _, c := range []struct {
		name string
		into func() Target
		json string
	}{
		{"unknown member", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":1,"extra":1`, 1)},
		{"key case", func() Target { return new(Hello) }, strings.Replace(hello, `"version"`, `"Version"`, 1)},
		{"duplicate member", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":1,"api":1`, 1)},
		{"missing member", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1,`, ``, 1)},
		{"null member", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":null`, 1)},
		{"float", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":1.0`, 1)},
		{"exponent", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":1e0`, 1)},
		{"leading zero", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":01`, 1)},
		{"negative unsigned", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":-1`, 1)},
		{"u16 overflow", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":65536`, 1)},
		{"u8 overflow", func() Target { return new(Hello) }, strings.Replace(hello, `"subclient":0`, `"subclient":256`, 1)},
		{"string for integer", func() Target { return new(Hello) }, strings.Replace(hello, `"api":1`, `"api":"1"`, 1)},
		{"unknown permission", func() Target { return new(Hello) }, strings.Replace(hello, `["ui"]`, `["ui","admin"]`, 1)},
		{"trailing data", func() Target { return new(Hello) }, hello + `{}`},
		{"trailing comma", func() Target { return new(Hello) }, strings.Replace(hello, `"subclient":0}`, `"subclient":0,}`, 1)},
		{"array form", func() Target { return new(Hello) }, `[1,1,["ui"],"d","c","n",0]`},
		{"raw control character", func() Target { return new(Hello) }, strings.Replace(hello, `"c"`, "\"c\x01\"", 1)},
		{"invalid UTF-8", func() Target { return new(Hello) }, strings.Replace(hello, `"c"`, "\"c\xff\"", 1)},
		{"lone leading surrogate", func() Target { return new(Hello) }, strings.Replace(hello, `"c"`, `"\ud800"`, 1)},
		{"lone trailing surrogate", func() Target { return new(Hello) }, strings.Replace(hello, `"c"`, `"\udc00"`, 1)},
		{"unpaired surrogates", func() Target { return new(Hello) }, strings.Replace(hello, `"c"`, `"\ud800\u0041"`, 1)},
		{"invalid escape", func() Target { return new(Hello) }, strings.Replace(hello, `"c"`, `"\x41"`, 1)},
		{"i64 overflow", func() Target { return new(Record) }, `[{"type":"integer","value":9223372036854775808}]`},
		{"i64 underflow", func() Target { return new(Record) }, `[{"type":"integer","value":-9223372036854775809}]`},
		{"negative zero", func() Target { return new(Record) }, `[{"type":"integer","value":-0}]`},
		{"unknown scalar", func() Target { return new(Record) }, `[{"type":"float","value":1}]`},
		{"scalar without value", func() Target { return new(Record) }, `[{"type":"bool"}]`},
		{"scalar member", func() Target { return new(Record) }, `[{"type":"bool","value":true,"extra":1}]`},
		{"scalar duplicate tag", func() Target { return new(Record) }, `[{"type":"bool","type":"bool","value":true}]`},
		{"scalar type mismatch", func() Target { return new(Record) }, `[{"type":"choice","value":-1}]`},
		{"field member", func() Target { return new(Channel) }, `{"id":"a.b","schema":1,"direction":"to_client","fields":[{"type":"bool","max":1}]}`},
		{"field missing bound", func() Target { return new(Channel) }, `{"id":"a.b","schema":1,"direction":"to_client","fields":[{"type":"integer","min":0}]}`},
		{"unknown field type", func() Target { return new(Channel) }, `{"id":"a.b","schema":1,"direction":"to_client","fields":[{"type":"list"}]}`},
		{"unknown direction", func() Target { return new(Channel) }, `{"id":"a.b","schema":1,"direction":"both","fields":[]}`},
		{"envelope member", func() Target { return new(Envelope) }, strings.Replace(envelope, `"version":1`, `"version":1,"extra":1`, 1)},
		{"unknown kind", func() Target { return new(Control) }, `{"kind":"reset","body":null}`},
		{"hello without body", func() Target { return new(Control) }, `{"kind":"hello"}`},
		{"disabled with body", func() Target { return new(Control) }, `{"kind":"disabled","body":{}}`},
		{"control member", func() Target { return new(Control) }, `{"kind":"disabled","extra":1}`},
		{"control without kind", func() Target { return new(Control) }, `{"body":null}`},
		{"empty", func() Target { return new(Hello) }, ``},
		{"deep nesting", func() Target { return new(Control) }, `{"body":` + strings.Repeat("[", 200) + strings.Repeat("]", 200) + `,"kind":"disabled"}`},
	} {
		t.Run(c.name, func(t *testing.T) {
			if err := Decode([]byte(c.json), c.into()); err == nil {
				t.Fatalf("decoded %s", c.json)
			}
		})
	}
}

// Decode accepts what serde_json accepts beyond the canonical form: whitespace, any member order,
// escaped keys, surrogate pairs, content before the tag, set members in any order and repeated,
// and a missing Option.
func TestDecodeAcceptsWhatSerdeAccepts(t *testing.T) {
	var h Hello
	spaced := " {\n\t\"subclient\" : 0 , \"\\u0076ersion\":1,\"api\":1,\"capabilities\":[\"messaging\",\"ui\",\"messaging\"],\"offer_digest\":\"d\",\"client_challenge\":\"\\ud83d\\ude00\\/\",\"connection\":\"n\"}\r\n"
	if err := Decode([]byte(spaced), &h); err != nil {
		t.Fatal(err)
	}
	want := Hello{Version: 1, API: 1, Capabilities: NewPermissions(PermissionUI, PermissionMessaging), OfferDigest: "d", ClientChallenge: "\U0001F600/", Connection: "n"}
	if h != want {
		t.Fatalf("decoded %+v, want %+v", h, want)
	}

	var record Record
	if err := Decode([]byte(`[{"value":-9223372036854775808,"type":"integer"},{"value":"x","type":"text"}]`), &record); err != nil {
		t.Fatal(err)
	}
	if want := (Record{integer(math.MinInt64), text("x")}); !reflect.DeepEqual(record, want) {
		t.Fatalf("decoded %s, want %s", describe(record), describe(want))
	}

	var control Control
	if err := Decode([]byte(`{"body":`+hello+`,"kind":"hello"}`), &control); err != nil || control.Hello == nil {
		t.Fatalf("content before the tag: %v", err)
	}
	for _, disabled := range []string{`{"kind":"disabled"}`, `{"body":null,"kind":"disabled"}`} {
		control = Control{}
		if err := Decode([]byte(disabled), &control); err != nil || !control.Disabled {
			t.Fatalf("%s: %v", disabled, err)
		}
	}

	var channel Channel
	if err := Decode([]byte(`{"fields":[{"max":2,"type":"integer","min":1}],"direction":"to_server","schema":1,"id":"a.b"}`), &channel); err != nil {
		t.Fatal(err)
	}
	if len(channel.Fields) != 1 || channel.Fields[0] != (IntegerField{Min: 1, Max: 2}) || channel.Direction != ToServer {
		t.Fatalf("decoded %+v", channel)
	}

	manifest := strings.Replace(string(fixture(t, "manifest_payload.json")), `"component":"component.wasm",`, ``, 1)
	var m Manifest
	if err := Decode([]byte(manifest), &m); err != nil || m.Component != nil {
		t.Fatalf("a manifest without a component: %v, %v", err, m.Component)
	}
}

// Sets encode as Rust's BTreeSet does, whatever order and repetition Go holds them in, and maps
// in key order, so a Go-built document is canonical.
func TestEncodeSortsSets(t *testing.T) {
	component := "c.wasm"
	cases := []struct {
		value Message
		want  string
	}{
		{NewPermissions(PermissionMedia, PermissionUI, PermissionMessaging, PermissionUI), `["ui","messaging","media"]`},
		{Scope{Permissions: NewPermissions(PermissionMessaging), Origins: []string{"https://b.example", "https://a.example", "https://b.example"}}, `{"permissions":["messaging"],"origins":["https://a.example","https://b.example"],"memory_bytes":0,"gpu_bytes":0}`},
		{Manifest{Component: &component, Actions: []string{"z", "a", "z"}}, `{"version":0,"api":0,"id":"","publisher_key":"","package_version":"","permissions":[],"component":"c.wasm","channels":[],"actions":["a","z"],"files":[]}`},
		{Manifest{}, `{"version":0,"api":0,"id":"","publisher_key":"","package_version":"","permissions":[],"component":null,"channels":[],"actions":[],"files":[]}`},
		{Control{Ready: &Ready{Packages: []string{"b", "a"}, Permissions: map[string]Permissions{"z": 0, "b.c": NewPermissions(PermissionUI), "b": 0}}}, `{"kind":"ready","body":{"session":"","packages":["b","a"],"generation":0,"permissions":{"b":[],"b.c":["ui"],"z":[]},"world_epoch":0}}`},
	}
	for _, c := range cases {
		got, err := Encode(c.value)
		if err != nil || string(got) != c.want {
			t.Errorf("%T encoded as %s (%v), want %s", c.value, got, err, c.want)
		}
	}
	scope := Scope{Origins: []string{"b", "a"}}
	if _, err := Encode(scope); err != nil || scope.Origins[0] != "b" {
		t.Errorf("encoding reordered the caller's set: %v", scope.Origins)
	}
}

// What has no Rust value does not encode: invalid UTF-8, an empty or ambiguous control message,
// a scalar with no value or two, and a missing field.
func TestEncodeRejectsWhatRustCannotHold(t *testing.T) {
	yes := true
	for _, value := range []Message{
		Hello{Connection: "\xff"},
		Record{text("ok\xc3")},
		Control{},
		Control{Hello: &Hello{}, Disabled: true},
		Record{experience.Scalar{}},
		Record{{Bool: &yes, Text: new(string)}},
		Channel{Fields: []Field{nil}},
	} {
		if got, err := Encode(value); err == nil {
			t.Errorf("%#v encoded as %s", value, got)
		}
	}
}
