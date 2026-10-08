package experience

import (
	"bytes"
	"errors"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"testing"
)

func openTestStore(t *testing.T, dir string) *Store {
	t.Helper()
	s, err := OpenStore(dir)
	if err != nil {
		t.Fatalf("OpenStore(%q): %v", dir, err)
	}
	return s
}

func TestStorePlaceStartsNewGeneration(t *testing.T) {
	s := openTestStore(t, t.TempDir())
	k := Key{Dim: 0, X: 1, Y: 64, Z: -3}

	first := s.Place("demo", k)
	if first.Revision != 0 {
		t.Fatalf("placed revision = %d, want 0", first.Revision)
	}
	if err := s.SetData("demo", k, []byte{1, 2, 3}, true); err != nil {
		t.Fatal(err)
	}

	second := s.Place("demo", k)
	if second.Generation == first.Generation {
		t.Fatalf("replacement kept generation %d", first.Generation)
	}
	if second.Revision != 0 {
		t.Fatalf("replacement revision = %d, want 0", second.Revision)
	}
	if data, ok := s.Data("demo", k); ok || data != nil {
		t.Fatalf("replacement kept data %v (present %v)", data, ok)
	}
	if got, ok := s.Token("demo", k); !ok || got != second {
		t.Fatalf("Token = %v, %v; want %v, true", got, ok, second)
	}
	if got := s.Budget("demo"); got != dataQuota {
		t.Fatalf("budget after replacement = %d, want %d", got, dataQuota)
	}
	other := s.Place("demo", Key{X: 2})
	if other.Generation == first.Generation || other.Generation == second.Generation {
		t.Fatalf("generation %d reused", other.Generation)
	}
}

func TestStoreRemoveReturnsPreviousData(t *testing.T) {
	s := openTestStore(t, t.TempDir())
	k := Key{X: 5}
	s.Place("demo", k)
	if err := s.SetData("demo", k, []byte("hello"), true); err != nil {
		t.Fatal(err)
	}

	prev, had := s.Remove("demo", k)
	if !had || string(prev) != "hello" {
		t.Fatalf("Remove = %q, %v; want hello, true", prev, had)
	}
	if _, ok := s.Token("demo", k); ok {
		t.Fatal("token survives Remove")
	}
	if _, ok := s.Data("demo", k); ok {
		t.Fatal("data survives Remove")
	}
	if got := s.Budget("demo"); got != dataQuota {
		t.Fatalf("budget after Remove = %d, want %d", got, dataQuota)
	}
	if prev, had := s.Remove("demo", k); had || prev != nil {
		t.Fatalf("second Remove = %v, %v; want nil, false", prev, had)
	}

	s.Place("demo", k)
	if prev, had := s.Remove("demo", k); had || prev != nil {
		t.Fatalf("Remove without data = %v, %v; want nil, false", prev, had)
	}
}

func TestStoreAbsentVersusEmpty(t *testing.T) {
	dir := t.TempDir()
	s := openTestStore(t, dir)
	absent, empty := Key{X: 1}, Key{X: 2}
	s.Place("demo", absent)
	s.Place("demo", empty)
	if err := s.SetData("demo", empty, []byte{}, true); err != nil {
		t.Fatal(err)
	}

	check := func(s *Store, when string) {
		t.Helper()
		if data, ok := s.Data("demo", absent); ok || data != nil {
			t.Fatalf("%s: absent = %v, %v; want nil, false", when, data, ok)
		}
		if data, ok := s.Data("demo", empty); !ok || data == nil || len(data) != 0 {
			t.Fatalf("%s: empty = %#v, %v; want []byte{}, true", when, data, ok)
		}
	}
	check(s, "live")

	if err := s.Flush(); err != nil {
		t.Fatal(err)
	}
	raw, err := os.ReadFile(filepath.Join(dir, "demo.json"))
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Contains(raw, []byte(`"data":null`)) || !bytes.Contains(raw, []byte(`"data":""`)) {
		t.Fatalf("file does not distinguish null from empty: %s", raw)
	}
	check(openTestStore(t, dir), "reopened")

	if err := s.SetData("demo", empty, nil, false); err != nil {
		t.Fatal(err)
	}
	if data, ok := s.Data("demo", empty); ok || data != nil {
		t.Fatalf("cleared = %v, %v; want nil, false", data, ok)
	}
}

func TestStoreRevisionAdvancesOnWrite(t *testing.T) {
	s := openTestStore(t, t.TempDir())
	k := Key{Y: 10}
	placed := s.Place("demo", k)

	for i, write := range []struct {
		data    []byte
		present bool
	}{{[]byte{1}, true}, {[]byte{1}, true}, {nil, false}} {
		if err := s.SetData("demo", k, write.data, write.present); err != nil {
			t.Fatal(err)
		}
		got, _ := s.Token("demo", k)
		want := Token{Generation: placed.Generation, Revision: uint64(i + 1)}
		if got != want {
			t.Fatalf("after write %d: token %v, want %v", i+1, got, want)
		}
	}

	if err := s.SetData("demo", Key{Y: 11}, []byte{1}, true); !errors.Is(err, ErrNotPlaced) {
		t.Fatalf("SetData without placement = %v, want ErrNotPlaced", err)
	}
	if _, ok := s.Token("demo", Key{Y: 11}); ok {
		t.Fatal("SetData without placement created an entry")
	}
}

func TestStoreQuotaRejectsAndLeavesStateUnchanged(t *testing.T) {
	s := openTestStore(t, t.TempDir())
	a, b := Key{X: 1}, Key{X: 2}
	s.Place("demo", a)
	s.Place("demo", b)

	if err := s.SetData("demo", a, make([]byte, dataQuota-10), true); err != nil {
		t.Fatal(err)
	}
	if got := s.Budget("demo"); got != 10 {
		t.Fatalf("budget = %d, want 10", got)
	}
	if err := s.SetData("demo", b, []byte("0123456789"), true); err != nil {
		t.Fatalf("write filling the quota exactly: %v", err)
	}
	if got := s.Budget("demo"); got != 0 {
		t.Fatalf("budget = %d, want 0", got)
	}

	before, _ := s.Token("demo", b)
	if err := s.SetData("demo", b, []byte("0123456789A"), true); !errors.Is(err, ErrQuota) {
		t.Fatalf("over-quota write = %v, want ErrQuota", err)
	}
	if after, _ := s.Token("demo", b); after != before {
		t.Fatalf("rejected write changed token %v -> %v", before, after)
	}
	if data, _ := s.Data("demo", b); string(data) != "0123456789" {
		t.Fatalf("rejected write changed data to %q", data)
	}
	if got := s.Budget("demo"); got != 0 {
		t.Fatalf("budget after rejection = %d, want 0", got)
	}

	if err := s.SetData("demo", a, make([]byte, dataQuota-11), true); err != nil {
		t.Fatalf("shrinking write: %v", err)
	}
	if err := s.SetData("demo", b, []byte("0123456789A"), true); err != nil {
		t.Fatalf("write within freed budget: %v", err)
	}
	if got := s.Budget("other"); got != dataQuota {
		t.Fatalf("other experience budget = %d, want %d", got, dataQuota)
	}
}

func TestStoreIsolationByExperienceAndDimension(t *testing.T) {
	s := openTestStore(t, t.TempDir())
	over := Key{Dim: 0, X: 7, Y: 7, Z: 7}
	nether := Key{Dim: 1, X: 7, Y: 7, Z: 7}

	s.Place("alpha", over)
	if err := s.SetData("alpha", over, []byte("alpha"), true); err != nil {
		t.Fatal(err)
	}
	if _, ok := s.Token("beta", over); ok {
		t.Fatal("beta sees alpha's entry")
	}
	if _, ok := s.Token("alpha", nether); ok {
		t.Fatal("nether sees overworld entry")
	}

	s.Place("beta", over)
	s.Place("alpha", nether)
	if err := s.SetData("beta", over, []byte("beta"), true); err != nil {
		t.Fatal(err)
	}
	if err := s.SetData("alpha", nether, []byte("nether"), true); err != nil {
		t.Fatal(err)
	}
	for _, c := range []struct {
		exp  string
		k    Key
		want string
	}{{"alpha", over, "alpha"}, {"beta", over, "beta"}, {"alpha", nether, "nether"}} {
		if data, _ := s.Data(c.exp, c.k); string(data) != c.want {
			t.Fatalf("%s %v = %q, want %q", c.exp, c.k, data, c.want)
		}
	}
	if got := s.Budget("beta"); got != dataQuota-4 {
		t.Fatalf("beta budget = %d, want %d", got, dataQuota-4)
	}

	if _, had := s.Remove("beta", over); !had {
		t.Fatal("beta remove found nothing")
	}
	if data, _ := s.Data("alpha", over); string(data) != "alpha" {
		t.Fatalf("beta remove affected alpha: %q", data)
	}
}

func TestStoreFlushAndReopenRestoresEverything(t *testing.T) {
	dir := t.TempDir()
	s := openTestStore(t, dir)
	keys := []Key{{Dim: 0, X: -1, Y: -64, Z: 1 << 20}, {Dim: 1, X: 2, Y: 3, Z: 4}, {Dim: 2, X: 0, Y: 0, Z: 0}}
	tokens := map[Key]Token{}
	for i, k := range keys {
		s.Place("alpha", k)
		if i < 2 {
			if err := s.SetData("alpha", k, []byte{byte(i), 0xff, 0x0a}, true); err != nil {
				t.Fatal(err)
			}
		}
		tokens[k], _ = s.Token("alpha", k)
	}
	s.Place("beta", keys[0])
	betaToken, _ := s.Token("beta", keys[0])
	if err := s.Flush(); err != nil {
		t.Fatal(err)
	}

	r := openTestStore(t, dir)
	for i, k := range keys {
		if got, ok := r.Token("alpha", k); !ok || got != tokens[k] {
			t.Fatalf("alpha %v token = %v, %v; want %v", k, got, ok, tokens[k])
		}
		data, ok := r.Data("alpha", k)
		if i < 2 && (!ok || !bytes.Equal(data, []byte{byte(i), 0xff, 0x0a})) {
			t.Fatalf("alpha %v data = %x, %v", k, data, ok)
		}
		if i == 2 && ok {
			t.Fatalf("alpha %v absent data came back as %x", k, data)
		}
	}
	if got, ok := r.Token("beta", keys[0]); !ok || got != betaToken {
		t.Fatalf("beta token = %v, %v; want %v", got, ok, betaToken)
	}
	if got := r.Budget("alpha"); got != dataQuota-6 {
		t.Fatalf("reopened budget = %d, want %d", got, dataQuota-6)
	}

	used := map[uint64]bool{}
	for _, tok := range tokens {
		used[tok.Generation] = true
	}
	used[betaToken.Generation] = true
	if fresh := r.Place("gamma", Key{X: 99}); used[fresh.Generation] {
		t.Fatalf("reopened store reused generation %d", fresh.Generation)
	}

	if _, had := r.Remove("alpha", keys[0]); !had {
		t.Fatal("remove after reopen found nothing")
	}
	if err := r.Flush(); err != nil {
		t.Fatal(err)
	}
	if _, ok := openTestStore(t, dir).Token("alpha", keys[0]); ok {
		t.Fatal("removed entry came back after flush and reopen")
	}
}

func TestStoreFlushIsAtomic(t *testing.T) {
	dir := t.TempDir()
	s := openTestStore(t, dir)
	k := Key{X: 1}
	s.Place("demo", k)
	if err := s.SetData("demo", k, []byte("old"), true); err != nil {
		t.Fatal(err)
	}
	if err := s.Flush(); err != nil {
		t.Fatal(err)
	}

	// A crash mid-write leaves a partial temp file beside the last good file.
	stray := filepath.Join(dir, "demo.json.tmp")
	if err := os.WriteFile(stray, []byte(`{"schema":1,"entr`), 0o644); err != nil {
		t.Fatal(err)
	}
	r := openTestStore(t, dir)
	if data, _ := r.Data("demo", k); string(data) != "old" {
		t.Fatalf("reopened data = %q, want old", data)
	}
	if _, err := os.Stat(stray); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("stray temp file not cleaned on open: %v", err)
	}

	// A write that cannot complete leaves the previous file intact and the store dirty.
	if err := r.SetData("demo", k, []byte("new"), true); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(stray, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := r.Flush(); err == nil {
		t.Fatal("Flush succeeded although its temp file could not be created")
	}
	if err := os.Remove(stray); err != nil {
		t.Fatal(err)
	}
	if data, _ := openTestStore(t, dir).Data("demo", k); string(data) != "old" {
		t.Fatalf("failed flush changed the file: %q", data)
	}
	if err := r.Flush(); err != nil {
		t.Fatal(err)
	}
	if data, _ := openTestStore(t, dir).Data("demo", k); string(data) != "new" {
		t.Fatalf("retried flush data = %q, want new", data)
	}
	if _, err := os.Stat(stray); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("flush left its temp file: %v", err)
	}
}

func TestStoreInstalledRoundTrip(t *testing.T) {
	dir := t.TempDir()
	s := openTestStore(t, dir)
	if got := s.Installed(); len(got) != 0 {
		t.Fatalf("fresh Installed = %v, want empty", got)
	}
	if err := s.SetInstalled([]Loaded{{ID: "beta"}, {ID: "alpha"}, {ID: "beta"}}); err != nil {
		t.Fatal(err)
	}
	want := []string{"alpha", "beta"}
	if got := s.Installed(); !slices.Equal(got, want) {
		t.Fatalf("Installed = %v, want %v", got, want)
	}
	r := openTestStore(t, dir)
	if got := r.Installed(); !slices.Equal(got, want) {
		t.Fatalf("reopened Installed = %v, want %v", got, want)
	}

	got := r.Installed()
	got[0] = "mutated"
	if again := r.Installed(); !slices.Equal(again, want) {
		t.Fatalf("Installed exposes internal state: %v", again)
	}
}

func TestStoreLoadedInstalledIsNormalized(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(filepath.Join(dir, "_installed.json"), []byte(`{"schema":1,"ids":["beta","alpha","beta"]}`), 0o644); err != nil {
		t.Fatal(err)
	}
	if got, want := openTestStore(t, dir).Installed(), []string{"alpha", "beta"}; !slices.Equal(got, want) {
		t.Fatalf("Installed = %v, want %v", got, want)
	}
}

func TestStoreExperienceNamedInstalledKeepsItsData(t *testing.T) {
	dir := t.TempDir()
	s := openTestStore(t, dir)
	k := Key{X: 3}
	s.Place("installed", k)
	if err := s.SetData("installed", k, []byte("mine"), true); err != nil {
		t.Fatal(err)
	}
	if err := s.SetInstalled([]Loaded{{ID: "installed"}, {ID: "alpha"}}); err != nil {
		t.Fatal(err)
	}
	if err := s.Flush(); err != nil {
		t.Fatal(err)
	}

	r := openTestStore(t, dir)
	if data, ok := r.Data("installed", k); !ok || string(data) != "mine" {
		t.Fatalf("installed data = %q, %v; want mine, true", data, ok)
	}
	if got, want := r.Installed(), []string{"alpha", "installed"}; !slices.Equal(got, want) {
		t.Fatalf("Installed = %v, want %v", got, want)
	}
}

func TestStoreCorruptFileFailsOpenNamingFile(t *testing.T) {
	emptyData := `{"schema":1,"next_generation":1,"entries":[]}`
	for name, content := range map[string]string{
		"syntax.json":     `{"schema":1,"next_generation":`,
		"schema.json":     `{"schema":2,"next_generation":1,"entries":[]}`,
		"hex.json":        `{"schema":1,"next_generation":2,"entries":[{"dim":0,"x":0,"y":0,"z":0,"generation":1,"revision":0,"data":"zz"}]}`,
		"dup.json":        `{"schema":1,"next_generation":3,"entries":[{"dim":0,"x":0,"y":0,"z":0,"generation":1,"revision":0,"data":null},{"dim":0,"x":0,"y":0,"z":0,"generation":2,"revision":0,"data":null}]}`,
		"_installed.json": `{"schema":1,"ids":`,
		"Upper.json":      emptyData,
		"9lives.json":     emptyData,
		"notes.txt":       emptyData,
	} {
		t.Run(name, func(t *testing.T) {
			dir := t.TempDir()
			if err := os.WriteFile(filepath.Join(dir, name), []byte(content), 0o644); err != nil {
				t.Fatal(err)
			}
			_, err := OpenStore(dir)
			if err == nil {
				t.Fatal("OpenStore accepted a corrupt file")
			}
			if !strings.Contains(err.Error(), name) {
				t.Fatalf("error %q does not name %s", err, name)
			}
		})
	}
}
