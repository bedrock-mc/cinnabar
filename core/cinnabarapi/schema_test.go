package cinnabarapi

import (
	"bytes"
	"crypto/ecdsa"
	"crypto/elliptic"
	"crypto/rand"
	"crypto/sha256"
	_ "embed"
	"encoding/hex"
	"encoding/json"
	"os"
	"os/exec"
	"path/filepath"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/internal/bridgecontract"
	"github.com/santhosh-tekuri/jsonschema/v6"
)

// schema/source.json records the immutable server revision for this exact schema.
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
		helloMessage{Type: typeHello, Protocol: protocolVersion, ClientVersion: "0.1.18", ClientBuild: 42, Channel: "nightly", Visible: true, Token: "aGVhZGVy.cGF5bG9hZA.c2ln"},
		proofMessage{Type: typeProof, Signature: signature},
		visibilityMessage{Type: typeVisibility, Visible: false},
		lookupMessage{Type: typeLookup, ID: 4294967295, XUIDs: xuids},
		lookupMessage{Type: typeLookup, ID: 1, UUIDs: []string{"00112233-4455-6677-8899-aabbccddeeff"}},
		lookupMessage{Type: typeLookup, ID: 2, XUIDs: xuids[:1], UUIDs: []string{"00112233-4455-6677-8899-aabbccddeeff"}},
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
		{`{"type":"welcome","xuid":"2535400000000000"}`, inbound{Type: typeWelcome, XUID: "2535400000000000"}},
		{`{"type":"welcome","xuid":"1","lookup_batch_max":64,"lookup_bucket_capacity":100,"lookup_refill_per_min":50,"cache_ttl_ms":500,"ping_interval_ms":1000,"reconnect_max_ms":2000,"future":true}`, inbound{Type: typeWelcome, XUID: "1", parameters: parameters{LookupBatchMax: 64, LookupBucketCapacity: 100, LookupRefillPerMin: 50, CacheTTLMS: 500, PingIntervalMS: 1000, ReconnectMaxMS: 2000}}},
		{`{"type":"lookup_result","id":7,"cinnabar":["1"]}`, inbound{Type: typeLookupResult, ID: &id, Cinnabar: []string{"1"}}},
		{`{"type":"lookup_result","id":7,"cinnabar":[],"cinnabar_uuids":["00112233-4455-6677-8899-aabbccddeeff"]}`, inbound{Type: typeLookupResult, ID: &id, Cinnabar: []string{}, CinnabarUUIDs: []string{"00112233-4455-6677-8899-aabbccddeeff"}}},
		{`{"type":"error","code":"rate_limited","id":7}`, inbound{Type: typeError, Code: codeRateLimited, ID: &id}},
		{`{"type":"release","channel":"beta","version":"0.2.0","build":42,"published_at":"2026-10-13T00:00:00.000Z"}`, inbound{Type: typeRelease, Channel: "beta", Version: "0.2.0", Build: 42, PublishedAt: "2026-10-13T00:00:00.000Z"}},
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
	validate(t, "ReleaseHookRequest", map[string]any{"channel": "beta", "version": "0.1.18", "build": 42})
}

// Every fallback parameter must match the server schema, and every advertised default must be implemented.
func TestWelcomeDefaultsMatchPinnedSchema(t *testing.T) {
	var schema struct {
		Defs struct {
			Welcome struct {
				Properties map[string]struct {
					Default *int64 `json:"default"`
				} `json:"properties"`
			} `json:"Welcome"`
		} `json:"$defs"`
	}
	if err := json.Unmarshal(pinnedSchema, &schema); err != nil {
		t.Fatal(err)
	}
	encoded, _ := json.Marshal(defaults())
	var got map[string]int64
	_ = json.Unmarshal(encoded, &got)
	for name, property := range schema.Defs.Welcome.Properties {
		if property.Default == nil {
			continue
		}
		if actual, ok := got[name]; !ok || actual != *property.Default {
			t.Fatalf("%s fallback = %d, want %d", name, actual, *property.Default)
		}
		delete(got, name)
	}
	if len(got) != 0 {
		t.Fatalf("defaults absent from schema: %v", got)
	}
}

// Build bounds stay aligned with the server's exact JSON integer representation.
func TestBuildBoundMatchesPinnedSchema(t *testing.T) {
	var schema struct {
		Defs struct {
			Build struct {
				Maximum uint64 `json:"maximum"`
			} `json:"Build"`
		} `json:"$defs"`
	}
	if err := json.Unmarshal(pinnedSchema, &schema); err != nil {
		t.Fatal(err)
	}
	if schema.Defs.Build.Maximum != maxBuildNumber {
		t.Fatal("build bound differs from schema")
	}
}

// The recorded digest makes accidental changes to the pinned server schema fail CI.
func TestPinnedSchemaDigest(t *testing.T) {
	var source struct{ Commit, SHA256 string }
	if err := json.Unmarshal(pinnedSource, &source); err != nil {
		t.Fatal(err)
	}
	commit, err := hex.DecodeString(source.Commit)
	if err != nil || len(commit) != 20 {
		t.Fatal("schema source needs a full commit ID")
	}
	digest := sha256.Sum256(pinnedSchema)
	if hex.EncodeToString(digest[:]) != source.SHA256 {
		t.Fatal("schema differs from recorded server digest")
	}
	t.Run("WindowsCheckout", func(t *testing.T) {
		digest := sha256.Sum256(windowsSchemaCheckout(t))
		if hex.EncodeToString(digest[:]) != source.SHA256 {
			t.Fatal("Windows checkout changes the pinned schema digest")
		}
	})
}

// windowsSchemaCheckout checks out the schema with Windows line-ending conversion enabled.
func windowsSchemaCheckout(t *testing.T) []byte {
	t.Helper()
	git, err := exec.LookPath("git")
	if err != nil {
		t.Skip("missing fixture: git executable for the Windows checkout test")
	}
	attributes, err := os.ReadFile("../../.gitattributes")
	if err != nil {
		t.Fatal(err)
	}
	root := t.TempDir()
	schemaPath := filepath.Join("core", "cinnabarapi", "schema", "v1.json")
	if err := os.MkdirAll(filepath.Dir(filepath.Join(root, schemaPath)), 0o755); err != nil {
		t.Fatal(err)
	}
	for name, data := range map[string][]byte{
		".gitattributes": attributes,
		schemaPath:       pinnedSchema,
		"control.txt":    []byte("Windows checkout control\n"),
	} {
		if err := os.WriteFile(filepath.Join(root, name), data, 0o644); err != nil {
			t.Fatal(err)
		}
	}
	// runGit keeps checkout settings independent of the developer's Git configuration.
	runGit := func(args ...string) {
		t.Helper()
		command := exec.Command(git, append([]string{
			"-c", "core.autocrlf=true", "-c", "core.eol=crlf",
			"-c", "core.safecrlf=false", "-c", "core.attributesFile=" + os.DevNull,
		}, args...)...)
		command.Dir = root
		command.Env = append(os.Environ(), "GIT_CONFIG_NOSYSTEM=1", "GIT_CONFIG_GLOBAL="+os.DevNull)
		if output, err := command.CombinedOutput(); err != nil {
			t.Fatalf("git %v: %v\n%s", args, err, output)
		}
	}
	runGit("init", "--quiet", "--template=")
	runGit("add", "--", ".gitattributes", schemaPath, "control.txt")
	for _, name := range []string{schemaPath, "control.txt"} {
		if err := os.Remove(filepath.Join(root, name)); err != nil {
			t.Fatal(err)
		}
	}
	runGit("checkout-index", "--force", "--all")
	control, err := os.ReadFile(filepath.Join(root, "control.txt"))
	if err != nil || string(control) != "Windows checkout control\r\n" {
		t.Fatalf("Windows checkout did not produce CRLF: %q, %v", control, err)
	}
	data, err := os.ReadFile(filepath.Join(root, schemaPath))
	if err != nil {
		t.Fatal(err)
	}
	return data
}

//go:embed schema/source.json
var pinnedSource []byte

// Local identifier validation must accept exactly the schema's identifier patterns.
func TestIdentifierPatternsMatchPinnedSchema(t *testing.T) {
	var schema struct {
		Defs struct {
			Xuid, Uuid struct{ Pattern string }
		} `json:"$defs"`
	}
	if err := json.Unmarshal(pinnedSchema, &schema); err != nil {
		t.Fatal(err)
	}
	if schema.Defs.Xuid.Pattern != xuidPattern.String() || schema.Defs.Uuid.Pattern != uuidPattern.String() {
		t.Fatal("identifier patterns differ from schema")
	}
}
