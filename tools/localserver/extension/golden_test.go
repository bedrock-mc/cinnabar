package extension

import (
	"bytes"
	"crypto/ed25519"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"reflect"
	"slices"
	"strconv"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

// facts are the pretty-printed fixtures that describe the byte-exact goldens.
var facts = []string{"constants.json", "digests.json", "enums.json", "test_seeds.json"}

// goldens maps every byte-exact fixture that `cinnabar-cxb write-fixtures` writes to a fresh Go
// mirror of its Rust type.
var goldens = map[string]func() Document{
	"offer_payload.json":             func() Document { return new(Offer) },
	"offer_signed.json":              func() Document { return new(SignedDocument) },
	"marker.json":                    func() Document { return new(Marker) },
	"hello_payload.json":             func() Document { return new(Hello) },
	"hello_message.json":             func() Document { return new(Control) },
	"hello_v2_payload.json":          func() Document { return new(Hello) },
	"hello_v2_message.json":          func() Document { return new(Control) },
	"accept_payload.json":            func() Document { return new(Accept) },
	"accept_signed.json":             func() Document { return new(SignedDocument) },
	"accept_message.json":            func() Document { return new(Control) },
	"accept_v2_payload.json":         func() Document { return new(Accept) },
	"accept_v2_signed.json":          func() Document { return new(SignedDocument) },
	"accept_v2_message.json":         func() Document { return new(Control) },
	"ready_message.json":             func() Document { return new(Control) },
	"epoch_message.json":             func() Document { return new(Control) },
	"envelope_to_client.json":        func() Document { return new(Envelope) },
	"envelope_to_server.json":        func() Document { return new(Envelope) },
	"envelope_all_scalar_types.json": func() Document { return new(Envelope) },
	"channel_all_field_types.json":   func() Document { return new(Channel) },
	"channel_list_record.json":       func() Document { return new(Channel) },
	"envelope_v2_list_record.json":   func() Document { return new(Envelope) },
	"fragments_v2_list_record.json":  func() Document { return new(fragments) },
	"manifest_payload.json":          func() Document { return new(Manifest) },
	"manifest_signed.json":           func() Document { return new(SignedDocument) },
}

// fragments is a Rust Vec<wire::Fragment>: the carrier messages of one fragmented message.
type fragments []Fragment

func (f fragments) appendJSON(b []byte) ([]byte, error) { return list[Fragment](f).appendJSON(b) }

func (f *fragments) decodeJSON(r *reader) (err error) {
	*f, err = readList[Fragment](r)
	return err
}

// fixture reads one file that `cinnabar-cxb write-fixtures` wrote.
func fixture(t *testing.T, name string) []byte {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("testdata", name))
	if err != nil {
		t.Fatal(err)
	}
	return data
}

// decodeFact decodes a pretty-printed fact fixture, refusing members that Go does not mirror.
func decodeFact(t *testing.T, name string, v any) {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(fixture(t, name)))
	decoder.DisallowUnknownFields()
	decoder.UseNumber()
	if err := decoder.Decode(v); err != nil {
		t.Fatalf("%s: %v", name, err)
	}
}

// decodeGolden decodes a byte-exact fixture into v.
func decodeGolden(t *testing.T, name string, v Target) {
	t.Helper()
	if err := Decode(fixture(t, name), v); err != nil {
		t.Fatalf("decoding %s: %v", name, err)
	}
}

// A fixture that Rust adds without a Go mirror fails here instead of going unchecked.
func TestGoldensCoverEveryFixture(t *testing.T) {
	paths, err := filepath.Glob(filepath.Join("testdata", "*.json"))
	if err != nil || len(paths) == 0 {
		t.Fatalf("no fixtures: %v", err)
	}
	for _, path := range paths {
		name := filepath.Base(path)
		if _, ok := goldens[name]; !ok && !slices.Contains(facts, name) {
			t.Errorf("the fixture %s has no Go mirror", name)
		}
	}
	for name := range goldens {
		fixture(t, name)
	}
}

// Every golden decodes into its Go mirror and re-encodes to exactly Rust's bytes.
func TestGoldensRoundTrip(t *testing.T) {
	for name, fresh := range goldens {
		t.Run(name, func(t *testing.T) {
			data := fixture(t, name)
			v := fresh()
			if err := Decode(data, v); err != nil {
				t.Fatalf("decoding: %v", err)
			}
			got, err := Encode(v)
			if err != nil {
				t.Fatalf("encoding: %v", err)
			}
			if !bytes.Equal(got, data) {
				t.Fatalf("re-encoded as\n%s\nwant\n%s", got, data)
			}
		})
	}
}

// The scalars decode to what experience.Scalar's own encoding/json decoder reads, so the round
// trip is not an identity on escaped text or a detour through float64.
func TestGoldenScalarsDecodeToRustValues(t *testing.T) {
	for _, name := range []string{"envelope_all_scalar_types.json", "envelope_v2_list_record.json"} {
		data := fixture(t, name)
		var envelope Envelope
		decodeGolden(t, name, &envelope)
		var plain struct {
			Payload []experience.Scalar `json:"payload"`
		}
		if err := json.Unmarshal(data, &plain); err != nil {
			t.Fatal(err)
		}
		if len(plain.Payload) == 0 {
			t.Fatalf("%s holds no scalars", name)
		}
		if !reflect.DeepEqual(envelope.Payload, plain.Payload) {
			t.Fatalf("%s decoded %s, experience.Scalar decodes %s", name, describe(envelope.Payload), describe(plain.Payload))
		}
	}
}

// describe prints scalars by value rather than by pointer.
func describe(scalars []experience.Scalar) string {
	var out []string
	for _, s := range scalars {
		switch {
		case s.Bool != nil:
			out = append(out, strconv.FormatBool(*s.Bool))
		case s.Integer != nil:
			out = append(out, strconv.FormatInt(*s.Integer, 10))
		case s.Text != nil:
			out = append(out, strconv.Quote(*s.Text))
		case s.Choice != nil:
			out = append(out, "choice "+strconv.FormatUint(uint64(*s.Choice), 10))
		case s.List != nil:
			out = append(out, "list "+describe(*s.List))
		case s.Record != nil:
			out = append(out, "record "+describe(*s.Record))
		default:
			out = append(out, "empty")
		}
	}
	return "[" + strings.Join(out, ", ") + "]"
}

// seedFacts mirrors test_seeds.json.
type seedFacts struct {
	Server    seedFact `json:"server"`
	Publisher seedFact `json:"publisher"`
}

type seedFact struct {
	Seed      string `json:"seed"`
	PublicKey string `json:"public_key"`
}

// testKeys loads the fixture seeds through ParseSeed.
func testKeys(t *testing.T) (seeds seedFacts, server, publisher ed25519.PrivateKey) {
	t.Helper()
	decodeFact(t, "test_seeds.json", &seeds)
	var err error
	if server, err = ParseSeed(seeds.Server.Seed); err != nil {
		t.Fatal(err)
	}
	if publisher, err = ParseSeed(seeds.Publisher.Seed); err != nil {
		t.Fatal(err)
	}
	return seeds, server, publisher
}

// A seed file holds one line of lowercase hex, like cinnabar-cxb keygen writes; Go derives the
// same public keys as Rust.
func TestSeedsDeriveRustPublicKeys(t *testing.T) {
	seeds, server, publisher := testKeys(t)
	if got := PublicKey(server); got != seeds.Server.PublicKey {
		t.Errorf("server public key %s, Rust %s", got, seeds.Server.PublicKey)
	}
	if got := PublicKey(publisher); got != seeds.Publisher.PublicKey {
		t.Errorf("publisher public key %s, Rust %s", got, seeds.Publisher.PublicKey)
	}
	if key, err := ParseSeed(" \r\n" + seeds.Server.Seed + "\n"); err != nil || PublicKey(key) != seeds.Server.PublicKey {
		t.Errorf("a seed with surrounding whitespace: %v", err)
	}
	seed := seeds.Server.Seed
	for _, bad := range []string{"", seed[:62], seed + "00", "0A" + seed[2:], "zz" + seed[2:], seed[:30] + " " + seed[31:]} {
		if _, err := ParseSeed(bad); err == nil {
			t.Errorf("ParseSeed accepted %q", bad)
		}
	}
}

// signedGolden is one signed fixture with its payload, domain, Rust's verification limit and
// whether the server seed (true) or the publisher seed signs it.
type signedGolden struct {
	payload, signed string
	domain          string
	limit           int
	server          bool
	fresh           func() Document
}

// signedGoldens lists every signed fixture with the limit its Rust verifier uses.
var signedGoldens = []signedGolden{
	{"offer_payload.json", "offer_signed.json", OfferDomain, MaxMarkerBytes / 2, true, func() Document { return new(Offer) }},
	{"accept_payload.json", "accept_signed.json", AcceptDomain, MaxPayloadBytes, true, func() Document { return new(Accept) }},
	{"accept_v2_payload.json", "accept_v2_signed.json", AcceptDomain, MaxPayloadBytes, true, func() Document { return new(Accept) }},
	{"manifest_payload.json", "manifest_signed.json", ManifestDomain, MaxMarkerBytes / 2, false, func() Document { return new(Manifest) }},
}

// Re-signing each decoded payload with its fixture seed reproduces Rust's signed document byte
// for byte.
func TestSignMatchesRust(t *testing.T) {
	_, server, publisher := testKeys(t)
	for _, c := range signedGoldens {
		t.Run(c.signed, func(t *testing.T) {
			key := publisher
			if c.server {
				key = server
			}
			v := c.fresh()
			decodeGolden(t, c.payload, v)
			document, err := Sign(c.domain, v, key)
			if err != nil {
				t.Fatal(err)
			}
			got, err := Encode(document)
			if err != nil {
				t.Fatal(err)
			}
			if want := fixture(t, c.signed); !bytes.Equal(got, want) {
				t.Fatalf("signed as\n%s\nwant\n%s", got, want)
			}
		})
	}
}

// Rust's signed documents verify in Go, decode to their payload and yield Rust's digest.
func TestVerifyAcceptsRustDocuments(t *testing.T) {
	seeds, _, _ := testKeys(t)
	var digests map[string]string
	decodeFact(t, "digests.json", &digests)
	for _, c := range signedGoldens {
		t.Run(c.signed, func(t *testing.T) {
			key := seeds.Publisher.PublicKey
			if c.server {
				key = seeds.Server.PublicKey
			}
			var document SignedDocument
			decodeGolden(t, c.signed, &document)
			v := c.fresh()
			digest, err := document.Verify(key, c.domain, c.limit, v)
			if err != nil {
				t.Fatal(err)
			}
			if digest != digests[c.payload] || digest == "" {
				t.Errorf("digest %s, Rust %s", digest, digests[c.payload])
			}
			if got, _ := Encode(v); !bytes.Equal(got, fixture(t, c.payload)) {
				t.Errorf("verified value encodes as\n%s", got)
			}
		})
	}
}

// Digest is SHA-256 in lowercase hex, as Rust names offers and bundles.
func TestDigestsMatchRust(t *testing.T) {
	var digests map[string]string
	decodeFact(t, "digests.json", &digests)
	if len(digests) == 0 {
		t.Fatal("no digests")
	}
	for name, want := range digests {
		if got := Digest(fixture(t, name)); got != want {
			t.Errorf("Digest(%s) = %s, Rust %s", name, got, want)
		}
	}
}

// Every protocol constant equals its Rust value, and the fixture lists no constant that Go lacks.
func TestConstantsMatchRust(t *testing.T) {
	var rust map[string]any
	decodeFact(t, "constants.json", &rust)
	mirror := map[string]any{
		"carrier":                   Carrier,
		"marker_path":               MarkerPath,
		"manifest_path":             ManifestPath,
		"offer_domain":              OfferDomain,
		"accept_domain":             AcceptDomain,
		"manifest_domain":           ManifestDomain,
		"wire_version":              WireVersion,
		"max_wire_version":          MaxWireVersion,
		"api_version":               APIVersion,
		"initial_bundle_generation": InitialBundleGeneration,
		"max_marker_bytes":          MaxMarkerBytes,
		"max_payload_bytes":         MaxPayloadBytes,
		"max_message_bytes":         MaxMessageBytes,
		"max_queue_bytes":           MaxQueueBytes,
		"max_envelope_bytes":        MaxEnvelopeBytes,
		"max_messages_per_second":   MaxMessagesPerSecond,
		"max_bytes_per_second":      MaxBytesPerSecond,
		"max_offer_lifetime_secs":   MaxOfferLifetimeSecs,
		"negotiation_timeout_ms":    NegotiationTimeoutMs,
		"max_bundles":               MaxBundles,
		"max_bundle_bytes":          MaxBundleBytes,
		"max_expanded_bytes":        MaxExpandedBytes,
		"max_channels":              MaxChannels,
		"max_channel_fields":        MaxChannelFields,
		"max_field_depth":           MaxFieldDepth,
		"max_identifier_bytes":      MaxIdentifierBytes,
		"max_fallback_bytes":        MaxFallbackBytes,
		"max_url_bytes":             MaxURLBytes,
		"max_origins":               MaxOrigins,
		"max_guest_memory":          MaxGuestMemory,
		"max_session_memory":        MaxSessionMemory,
		"max_gpu_bytes":             MaxGPUBytes,
	}
	for name, want := range rust {
		got, ok := mirror[name]
		if !ok {
			t.Errorf("the Rust constant %s has no Go mirror", name)
			continue
		}
		if fmt.Sprint(got) != fmt.Sprint(want) {
			t.Errorf("%s: Go %q, Rust %q", name, fmt.Sprint(got), fmt.Sprint(want))
		}
	}
	for name := range mirror {
		if _, ok := rust[name]; !ok {
			t.Errorf("the Go constant %s is not in the Rust fixture", name)
		}
	}
}

// Go knows exactly Rust's enum strings, in Rust's declaration order, and refuses any other.
func TestEnumsMatchRust(t *testing.T) {
	var rust struct {
		Permissions            []string `json:"permissions"`
		ImplementedPermissions []string `json:"implemented_permissions"`
		Directions             []string `json:"directions"`
	}
	decodeFact(t, "enums.json", &rust)
	if !slices.Equal(permissionNames[:], rust.Permissions) {
		t.Errorf("Go permissions %q, Rust %q", permissionNames, rust.Permissions)
	}
	var implemented []string
	for _, p := range ImplementedPermissions.List() {
		implemented = append(implemented, p.String())
	}
	if !slices.Equal(implemented, rust.ImplementedPermissions) {
		t.Errorf("Go implemented permissions %q, Rust %q", implemented, rust.ImplementedPermissions)
	}
	if !slices.Equal(directionNames[:], rust.Directions) {
		t.Errorf("Go directions %q, Rust %q", directionNames, rust.Directions)
	}
	for _, other := range []string{`"admin"`, `"UI"`, `""`, `null`, `1`} {
		var set Permissions
		if err := Decode([]byte("["+other+"]"), &set); err == nil {
			t.Errorf("Permissions decoded [%s]", other)
		}
		var direction Direction
		if err := Decode([]byte(other), &direction); err == nil {
			t.Errorf("Direction decoded %s", other)
		}
	}
	if _, err := Encode(Permissions(1 << len(permissionNames))); err == nil {
		t.Error("a permission beyond Rust's encoded")
	}
	if _, err := Encode(Direction(len(directionNames))); err == nil {
		t.Error("a direction beyond Rust's encoded")
	}
}
