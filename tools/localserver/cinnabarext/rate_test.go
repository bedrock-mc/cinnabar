package cinnabarext

import (
	"encoding/json"
	"os"
	"strings"
	"testing"
)

// TestSharedRateClassesAndEncodedCosts reads the same independent cases as Rust.
func TestSharedRateClassesAndEncodedCosts(t *testing.T) {
	data, err := os.ReadFile("../../../crates/server-experience/schema/rate-conformance.json")
	if err != nil {
		t.Fatal(err)
	}
	var cases []struct {
		Name            string
		Control         *string
		Direction       string
		MatchingNotice  bool `json:"matching_notice"`
		Carriers        []string
		Messages, Bytes uint64
	}
	if err := json.Unmarshal(data, &cases); err != nil {
		t.Fatal(err)
	}
	for _, c := range cases {
		t.Run(c.Name, func(t *testing.T) {
			class := RateData
			if c.Control != nil {
				var control Control
				if err := Decode([]byte(*c.Control), &control); err != nil {
					t.Fatal(err)
				}
				direction := ToClient
				if c.Direction == "to_server" {
					direction = ToServer
				}
				class = control.RateClass(direction, c.MatchingNotice)
			}
			sizes := make([]int, len(c.Carriers))
			for i, raw := range c.Carriers {
				sizes[i] = len(raw)
			}
			cost, err := class.Cost(sizes)
			if err != nil || cost != (RateCost{c.Messages, c.Bytes}) {
				t.Fatalf("cost %+v: %v", cost, err)
			}
		})
	}
}

// TestMaximumDefaultPayloadFitsAfterEncoding checks the shared burst against JSON expansion.
func TestMaximumDefaultPayloadFitsAfterEncoding(t *testing.T) {
	for _, fragmentBytes := range []uint32{4, 8, 128, 512, 768, 1024, MaxPayloadBytes} {
		limits := HostLimits
		limits.MaxFragmentBytes = fragmentBytes
		limits = limits.fitRate()
		maxPayload := int(limits.MaxMessageBytes)
		for _, character := range []string{`"`, `\`, "\x00", "é", "😀"} {
			t.Run(character, func(t *testing.T) {
				payload := Record{text("")}
				empty, _ := Encode(payload)
				encoded, _ := appendString(nil, character)
				width := len(encoded) - 2
				count := (maxPayload - len(empty)) / width
				payload = Record{text(strings.Repeat(character, count) + strings.Repeat("a", maxPayload-len(empty)-count*width))}
				e := Envelope{Version: MaxWireVersion, Session: strings.Repeat("f", 64), Connection: strings.Repeat("f", 64), Bundle: strings.Repeat("b", MaxIdentifierBytes), Generation: ^uint64(0), Channel: strings.Repeat("c", MaxIdentifierBytes), Schema: ^uint16(0), Sequence: ^uint64(0), WorldEpoch: ^uint64(0), Payload: payload}
				carriers, err := EncodeEnvelope(e, Wire{MaxWireVersion, limits})
				if err != nil {
					t.Fatal(err)
				}
				sizes := make([]int, len(carriers))
				for i, data := range carriers {
					sizes[i] = len(data)
				}
				if _, err := RateData.Cost(sizes); err != nil {
					t.Fatal(err)
				}
			})
		}
	}
}
