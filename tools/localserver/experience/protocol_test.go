package experience

import (
	"bytes"
	"encoding/binary"
	"encoding/json"
	"maps"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

// fixtureLimits mirrors the limits fixture: the constants the Go adapter must share with Rust.
type fixtureLimits struct {
	MaxFrameBytes     int    `json:"max_frame_bytes"`
	Protocol          uint32 `json:"protocol"`
	MaxBlockDataBytes int    `json:"max_block_data_bytes"`
	MaxStagedOps      int    `json:"max_staged_ops"`
	MaxTells          int    `json:"max_tells"`
	MaxTellBytes      int    `json:"max_tell_bytes"`
	MaxClientSends    int    `json:"max_client_sends"`
}

// rustLimits reads the limits fixture.
func rustLimits(t *testing.T) fixtureLimits {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", "protocol", "limits.json"))
	if err != nil {
		t.Fatal(err)
	}
	var rust fixtureLimits
	if err := decodeStrict(data, &rust); err != nil {
		t.Fatal(err)
	}
	return rust
}

// fixtureEnums mirrors the enums fixture: every protocol enum string, in Rust's order.
type fixtureEnums struct {
	Faces     []Face     `json:"faces"`
	Causes    []Cause    `json:"causes"`
	FailKinds []FailKind `json:"fail_kinds"`
}

// fixturePaths lists the golden fixtures that the Rust runtime writes.
func fixturePaths(t *testing.T) []string {
	t.Helper()
	paths, err := filepath.Glob(filepath.Join("testdata", "protocol", "*.json"))
	if err != nil || len(paths) == 0 {
		t.Fatalf("no protocol fixtures: %v", err)
	}
	return paths
}

// fixtureName is the fixture's file stem.
func fixtureName(path string) string {
	return strings.TrimSuffix(filepath.Base(path), ".json")
}

// fixtureValue returns a pointer to the Go type that mirrors the fixture name.
func fixtureValue(t *testing.T, name string) any {
	t.Helper()
	switch {
	case strings.HasPrefix(name, "request_"):
		return new(Request)
	case strings.HasPrefix(name, "response_"):
		return new(Response)
	case name == "enums":
		return new(fixtureEnums)
	case name == "limits":
		return new(fixtureLimits)
	}
	t.Fatalf("no Go type mirrors the fixture %s", name)
	return nil
}

// normalized re-encodes data compactly with sorted keys, keeping numbers as written.
func normalized(t *testing.T, data []byte) string {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(data))
	decoder.UseNumber()
	var v any
	if err := decoder.Decode(&v); err != nil {
		t.Fatalf("normalizing %s: %v", data, err)
	}
	out, err := json.Marshal(v)
	if err != nil {
		t.Fatalf("normalizing %s: %v", data, err)
	}
	return string(out)
}

// Every fixture decodes into its Go mirror and re-encodes to the same JSON.
func TestFixturesRoundTrip(t *testing.T) {
	for _, path := range fixturePaths(t) {
		name := fixtureName(path)
		t.Run(name, func(t *testing.T) {
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			v := fixtureValue(t, name)
			if err := decodeStrict(data, v); err != nil {
				t.Fatalf("decoding: %v", err)
			}
			encoded, err := json.Marshal(v)
			if err != nil {
				t.Fatalf("encoding: %v", err)
			}
			if got, want := normalized(t, encoded), normalized(t, data); got != want {
				t.Fatalf("re-encoded as\n%s\nwant\n%s", got, want)
			}
			if enums, ok := v.(*fixtureEnums); ok {
				checkEnums(t, enums)
			}
		})
	}
}

// checkEnums checks that Go knows exactly Rust's enum strings and refuses any other.
func checkEnums(t *testing.T, rust *fixtureEnums) {
	t.Helper()
	if !slices.Equal(faces, rust.Faces) {
		t.Errorf("Go faces %q, Rust faces %q", faces, rust.Faces)
	}
	if !slices.Equal(causes, rust.Causes) {
		t.Errorf("Go causes %q, Rust causes %q", causes, rust.Causes)
	}
	if !slices.Equal(failKinds, rust.FailKinds) {
		t.Errorf("Go fail kinds %q, Rust fail kinds %q", failKinds, rust.FailKinds)
	}
	for _, other := range []string{`"sideways"`, `"Up"`, `""`, `null`, `1`} {
		var face Face
		if err := decodeStrict([]byte(other), &face); err == nil {
			t.Errorf("Face decoded %s", other)
		}
		var cause Cause
		if err := decodeStrict([]byte(other), &cause); err == nil {
			t.Errorf("Cause decoded %s", other)
		}
		var kind FailKind
		if err := decodeStrict([]byte(other), &kind); err == nil {
			t.Errorf("FailKind decoded %s", other)
		}
	}
	if out, err := json.Marshal(Face("sideways")); err == nil {
		t.Errorf("an unknown Face encoded as %s", out)
	}
}

// unknownMember is the member TestUnknownFieldRejected adds.
const unknownMember = "unexpected"

// withUnknownMember returns one copy of v per JSON object in it, v included, with unknownMember
// added to that object.
func withUnknownMember(v any) []any {
	var copies []any
	switch v := v.(type) {
	case map[string]any:
		self := maps.Clone(v)
		self[unknownMember] = true
		copies = append(copies, self)
		for key, child := range v {
			for _, changed := range withUnknownMember(child) {
				parent := maps.Clone(v)
				parent[key] = changed
				copies = append(copies, parent)
			}
		}
	case []any:
		for i, child := range v {
			for _, changed := range withUnknownMember(child) {
				parent := slices.Clone(v)
				parent[i] = changed
				copies = append(copies, parent)
			}
		}
	}
	return copies
}

// A member that the Rust type does not have is refused at every depth of every message, and so
// is a variant that Rust does not have.
func TestUnknownFieldRejected(t *testing.T) {
	for _, path := range fixturePaths(t) {
		name := fixtureName(path)
		if !strings.HasPrefix(name, "request_") && !strings.HasPrefix(name, "response_") {
			continue
		}
		t.Run(name, func(t *testing.T) {
			data, err := os.ReadFile(path)
			if err != nil {
				t.Fatal(err)
			}
			if err := decodeStrict(data, fixtureValue(t, name)); err != nil {
				t.Fatalf("the fixture itself does not decode: %v", err)
			}
			decoder := json.NewDecoder(bytes.NewReader(data))
			decoder.UseNumber()
			var message any
			if err := decoder.Decode(&message); err != nil {
				t.Fatal(err)
			}
			for _, changed := range withUnknownMember(message) {
				bad, err := json.Marshal(changed)
				if err != nil {
					t.Fatal(err)
				}
				err = decodeStrict(bad, fixtureValue(t, name))
				if err == nil || !strings.Contains(err.Error(), unknownMember) {
					t.Errorf("decoding %s: %v; want the unknown member refused", bad, err)
				}
			}
		})
	}
	for _, bad := range []struct {
		into any
		json string
	}{
		{new(Request), `{"type":"reload"}`},
		{new(Request), `{"dir":"/srv"}`},
		{new(Response), `{"type":"result","seq":1,"outcome":{"type":"exploded"}}`},
		{new(Response), `{"type":"result","seq":1,"outcome":null}`},
		{new(Response), `null`},
		{new(Scalar), `{"type":"float","value":1.5}`},
		{new(Scalar), `{"type":"text"}`},
		{new(Scalar), `{"type":"text","value":null}`},
		{new(Scalar), `{"type":"bool","value":1}`},
		{new(Scalar), `{"type":"choice","value":65536}`},
		{new(Scalar), `{"type":"integer","value":1.5}`},
	} {
		if err := decodeStrict([]byte(bad.json), bad.into); err == nil {
			t.Errorf("%T decoded %s", bad.into, bad.json)
		}
	}
}

// A Scalar has the client wire protocol's form, whose canonical bytes escape no HTML, so its
// encoder decides: one that escapes HTML escapes it, one that does not leaves it as written.
func TestScalarLeavesHTMLEscapingToEncoder(t *testing.T) {
	text := "<a & b>"
	payload := []Scalar{{Text: &text}}
	var raw bytes.Buffer
	encoder := json.NewEncoder(&raw)
	encoder.SetEscapeHTML(false)
	if err := encoder.Encode(payload); err != nil {
		t.Fatal(err)
	}
	if got, want := raw.String(), `[{"type":"text","value":"<a & b>"}]`+"\n"; got != want {
		t.Errorf("without HTML escaping: %s, want %s", got, want)
	}
	escaped, err := json.Marshal(payload)
	if err != nil {
		t.Fatal(err)
	}
	if got, want := string(escaped), `[{"type":"text","value":"\u003ca \u0026 b\u003e"}]`; got != want {
		t.Errorf("with HTML escaping: %s, want %s", got, want)
	}
}

// Go shares the frame limit and the protocol version with Rust, and enforces the limit like
// Rust's read_frame and write_frame: a body of exactly maxFrameBytes passes, one more byte does
// not.
func TestFrameLimitMatchesRust(t *testing.T) {
	rust := rustLimits(t)
	if maxFrameBytes != rust.MaxFrameBytes {
		t.Errorf("maxFrameBytes = %d, Rust MAX_FRAME_BYTES = %d", maxFrameBytes, rust.MaxFrameBytes)
	}
	if protocolVersion != rust.Protocol {
		t.Errorf("protocolVersion = %d, Rust PROTOCOL_VERSION = %d", protocolVersion, rust.Protocol)
	}

	t.Run("read", func(t *testing.T) {
		const head, tail = `{"type":"load_failed","reason":"`, `"}`
		reason := strings.Repeat("r", maxFrameBytes-len(head)-len(tail))
		frame := binary.LittleEndian.AppendUint32(nil, maxFrameBytes)
		frame = append(frame, head+reason+tail...)
		response, err := readFrame(bytes.NewReader(frame))
		if err != nil || response.LoadFailed == nil || response.LoadFailed.Reason != reason {
			t.Fatalf("a frame of exactly maxFrameBytes: %v", err)
		}
		// Only the length arrives: an implementation that read on would report the missing body.
		oversized := binary.LittleEndian.AppendUint32(nil, maxFrameBytes+1)
		if _, err := readFrame(bytes.NewReader(oversized)); err == nil || !strings.Contains(err.Error(), "exceeds") {
			t.Fatalf("a length of maxFrameBytes+1: %v; want it refused before the body", err)
		}
	})

	t.Run("write", func(t *testing.T) {
		const head, tail = `{"type":"load","dir":"`, `"}`
		dir := strings.Repeat("d", maxFrameBytes-len(head)-len(tail))
		body, err := encodeFrame(Request{Load: &LoadRequest{Dir: dir}})
		if err != nil || len(body) != maxFrameBytes {
			t.Fatalf("a body of exactly maxFrameBytes: %d bytes, %v", len(body), err)
		}
		if _, err := encodeFrame(Request{Load: &LoadRequest{Dir: dir + "d"}}); err == nil {
			t.Fatal("a body of maxFrameBytes+1 was encoded")
		}
	})
}

// The commit check enforces the runtime's op, block data, tell and client message limits again,
// so Go shares them with Rust.
func TestCommitLimitsMatchRust(t *testing.T) {
	rust := rustLimits(t)
	for _, limit := range []struct {
		name      string
		goV, rust int
	}{
		{"maxBlockDataBytes", maxBlockDataBytes, rust.MaxBlockDataBytes},
		{"maxStagedOps", maxStagedOps, rust.MaxStagedOps},
		{"maxTells", maxTells, rust.MaxTells},
		{"maxTellBytes", maxTellBytes, rust.MaxTellBytes},
		{"maxClientSends", maxClientSends, rust.MaxClientSends},
	} {
		if limit.goV != limit.rust {
			t.Errorf("%s = %d, Rust has %d", limit.name, limit.goV, limit.rust)
		}
	}
}
