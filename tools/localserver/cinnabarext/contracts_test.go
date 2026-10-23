package cinnabarext

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

// sharedCorpus reads the same cases as the Rust client contract tests.
func sharedCorpus(t *testing.T) (corpus struct {
	Documents []struct {
		Name, Document, JSON string
		Accepted             bool
	}
	Ingress []struct {
		Name  string
		Steps []struct {
			Sequence            uint64
			Schema              *uint16
			WorldEpoch          *uint64 `json:"world_epoch"`
			Connection          *string
			Generation          *uint64
			Accepted, Delivered bool
		}
	}
}) {
	t.Helper()
	data, err := os.ReadFile(filepath.Join("..", "..", "..", "crates", "server-experience", "schema", "conformance.json"))
	if err != nil {
		t.Fatal(err)
	}
	if err := json.Unmarshal(data, &corpus); err != nil {
		t.Fatal(err)
	}
	return corpus
}

func TestSharedDocumentCases(t *testing.T) {
	for _, c := range sharedCorpus(t).Documents {
		t.Run(c.Name, func(t *testing.T) {
			var err error
			switch c.Document {
			case "hello":
				err = Decode([]byte(c.JSON), new(Hello))
			case "control":
				err = Decode([]byte(c.JSON), new(Control))
			case "scalar":
				err = within([]byte(c.JSON), func(r *reader) error { _, err := readScalar(r); return err })
			case "field":
				err = within([]byte(c.JSON), func(r *reader) error { _, err := readField(r); return err })
			default:
				t.Fatalf("unknown document %s", c.Document)
			}
			if (err == nil) != c.Accepted {
				t.Fatalf("accepted=%v, want %v: %v", err == nil, c.Accepted, err)
			}
		})
	}
}

func TestSharedIngressTransitions(t *testing.T) {
	for _, c := range sharedCorpus(t).Ingress {
		t.Run(c.Name, func(t *testing.T) {
			grant, epoch := goldenGrant(t)
			var base Envelope
			decodeGolden(t, "envelope_to_client.json", &base)
			in := NewIngress(ToClient, 0)
			for _, step := range c.Steps {
				envelope := base
				envelope.Sequence = step.Sequence
				if step.Schema != nil {
					envelope.Schema = *step.Schema
				}
				if step.WorldEpoch != nil {
					envelope.WorldEpoch = *step.WorldEpoch
				}
				if step.Connection != nil {
					envelope.Connection = *step.Connection
				}
				if step.Generation != nil {
					envelope.Generation = *step.Generation
				}
				data, err := Encode(envelope)
				if err != nil {
					t.Fatal(err)
				}
				got, err := in.Receive(data, 0, epoch, grant)
				if (err == nil) != step.Accepted || (got != nil) != step.Delivered {
					t.Fatalf("sequence %d: accepted=%v delivered=%v; want %v %v: %v", step.Sequence, err == nil, got != nil, step.Accepted, step.Delivered, err)
				}
			}
		})
	}
}
