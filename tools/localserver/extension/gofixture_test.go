package extension

import (
	"bytes"
	"flag"
	"os"
	"path/filepath"
	"slices"
	"testing"

	"github.com/hashimthearab/rust-mcbe/tools/localserver/experience"
)

var updateGoFixtures = flag.Bool("update-go-fixtures", false, "rewrite testdata/go from the server half")

// goFixtureDir holds what the server half itself produces for the Rust client verifiers in
// tools/cxb/tests/fixtures.rs.
var goFixtureDir = filepath.Join("testdata", "go")

// The server half's marker, its Accept for a developer client's v2 Hello and its first envelopes
// after that client's Ready, the second a list in fragments, are checked in, deterministic for a
// fixed clock and randomness, so the Rust test can run them through the client's own verifiers.
// Regenerate with `go test ./extension -run TestGoFixturesAreCurrent -update-go-fixtures`.
func TestGoFixturesAreCurrent(t *testing.T) {
	f := newConn(t, nil)
	h := f.hello()
	accept := f.accept(h)
	r := f.ready(accept)
	f.deliver(f.encode(Control{Ready: &r}))
	toClient := f.channel(ToClient)
	if !f.s.Send(f.player, f.manifest.ID, toClient.ID, toClient.Schema, []experience.Scalar{integer(42)}) {
		t.Fatal("the envelope was not sent")
	}
	items := f.itemsChannel()
	if !f.s.Send(f.player, f.manifest.ID, items.ID, items.Schema, itemsRecord(120)) {
		t.Fatal("the fragmented list was not sent")
	}
	out := f.carrier()
	files := map[string][]byte{
		"marker.json":              f.encode(f.s.Marker()),
		"hello_message.json":       f.encode(Control{Hello: &h}),
		"accept_message.json":      out[0],
		"ready_message.json":       f.encode(Control{Ready: &r}),
		"envelope_to_client.json":  out[1],
		"fragments_to_client.json": []byte("[" + string(bytes.Join(out[2:], []byte(","))) + "]"),
	}
	if *updateGoFixtures {
		if err := os.RemoveAll(goFixtureDir); err != nil {
			t.Fatal(err)
		}
		if err := os.MkdirAll(goFixtureDir, 0o755); err != nil {
			t.Fatal(err)
		}
		for name, data := range files {
			if err := os.WriteFile(filepath.Join(goFixtureDir, name), data, 0o644); err != nil {
				t.Fatal(err)
			}
		}
	}
	const regenerate = "regenerate with `go test ./extension -run TestGoFixturesAreCurrent -update-go-fixtures`"
	entries, err := os.ReadDir(goFixtureDir)
	if err != nil {
		t.Fatalf("%v; %s", err, regenerate)
	}
	var names []string
	for _, e := range entries {
		names = append(names, e.Name())
	}
	var want []string
	for name := range files {
		want = append(want, name)
	}
	slices.Sort(want)
	if !slices.Equal(names, want) {
		t.Fatalf("%s holds %q, want %q; %s", goFixtureDir, names, want, regenerate)
	}
	for name, data := range files {
		if got, err := os.ReadFile(filepath.Join(goFixtureDir, name)); err != nil || string(got) != string(data) {
			t.Errorf("%s is stale (%v); %s", name, err, regenerate)
		}
	}
}
