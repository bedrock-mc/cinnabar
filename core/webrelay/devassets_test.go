package webrelay

import (
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"
)

func TestDevAssetsListsAndServesCarriers(t *testing.T) {
	dir := t.TempDir()
	for name, body := range map[string]string{
		"vanilla-v2193.mcbea": "world",
		"lang/en_US.json":     "{}",
		"vanilla-v1.mcbesnd":  "sounds",
	} {
		path := filepath.Join(dir, filepath.FromSlash(name))
		if err := os.MkdirAll(filepath.Dir(path), 0o755); err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path, []byte(body), 0o644); err != nil {
			t.Fatal(err)
		}
	}
	server := httptest.NewServer(DevAssets(dir))
	defer server.Close()

	response, err := http.Get(server.URL + DevAssetsPath + "index.json")
	if err != nil {
		t.Fatal(err)
	}
	var index struct{ Files []devAsset }
	if err := json.NewDecoder(response.Body).Decode(&index); err != nil {
		t.Fatal(err)
	}
	response.Body.Close()
	want := []devAsset{{Path: "lang/en_US.json", Size: 2}, {Path: "vanilla-v1.mcbesnd", Size: 6}, {Path: "vanilla-v2193.mcbea", Size: 5}}
	if len(index.Files) != len(want) || index.Files[0] != want[0] || index.Files[1] != want[1] || index.Files[2] != want[2] {
		t.Fatalf("index = %+v, want %+v", index.Files, want)
	}

	response, err = http.Get(server.URL + DevAssetsPath + "lang/en_US.json")
	if err != nil {
		t.Fatal(err)
	}
	body, _ := io.ReadAll(response.Body)
	response.Body.Close()
	if string(body) != "{}" {
		t.Fatalf("served %q", body)
	}
	response, err = http.Get(server.URL + DevAssetsPath + "../devassets_test.go")
	if err != nil {
		t.Fatal(err)
	}
	response.Body.Close()
	if response.StatusCode == http.StatusOK {
		t.Fatal("a path outside the carrier directory was served")
	}
}
