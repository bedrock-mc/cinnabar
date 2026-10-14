package control

import (
	"context"
	"encoding/json"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/cinnabarapi"
)

type fakeCinnabarAPI struct {
	configs chan cinnabarapi.Config
	lookups chan []string
	answer  []string
	err     error
}

func (api *fakeCinnabarAPI) Configure(config cinnabarapi.Config) { api.configs <- config }

func (api *fakeCinnabarAPI) Lookup(_ context.Context, xuids []string) ([]string, error) {
	api.lookups <- xuids
	return api.answer, api.err
}

func startCinnabarControl(t *testing.T, api CinnabarAPI) (string, *Server) {
	t.Helper()
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	if api != nil {
		server.SetCinnabarAPI(api)
	}
	return dir, server
}

func rpcError(t *testing.T, payload []byte) *responseError {
	t.Helper()
	var answer struct {
		Error *responseError `json:"error"`
	}
	if err := json.Unmarshal(payload, &answer); err != nil {
		t.Fatal(err)
	}
	return answer.Error
}

func TestCinnabarConfigurationReachesTheConnection(t *testing.T) {
	api := &fakeCinnabarAPI{configs: make(chan cinnabarapi.Config, 1)}
	dir, _ := startCinnabarControl(t, api)
	payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":1,"method":"cinnabar_api.v1","params":{"client_version":"0.1.18","channel":"beta","visible":false}}`))
	if err := rpcError(t, payload); err != nil {
		t.Fatalf("configuration rejected: %s", payload)
	}
	if got := <-api.configs; got != (cinnabarapi.Config{ClientVersion: "0.1.18", Channel: "beta", Visible: false}) {
		t.Fatalf("config = %+v", got)
	}
	for _, params := range []string{
		`{"client_version":"0.1.18","channel":"beta"}`,
		`{"client_version":"","channel":"beta","visible":true}`,
		`{"client_version":"0.1.18","channel":"beta","visible":true,"extra":1}`,
		`{"client_version":"` + strings.Repeat("9", 65) + `","channel":"beta","visible":true}`,
		`{"client_version":"0.1.18+42","channel":"beta","visible":true}`,
		`{"client_version":"0.1.18","channel":"canary","visible":true}`,
	} {
		payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":2,"method":"cinnabar_api.v1","params":`+params+`}`))
		if err := rpcError(t, payload); err == nil || err.Code != -32602 {
			t.Fatalf("malformed configuration accepted: %s", payload)
		}
	}
}

func TestCinnabarConfigurationIsAcceptedWithoutAConnection(t *testing.T) {
	dir, _ := startCinnabarControl(t, nil)
	payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":1,"method":"cinnabar_api.v1","params":{"client_version":"0.1.18","channel":"beta","visible":true}}`))
	if err := rpcError(t, payload); err != nil {
		t.Fatalf("offline configuration rejected: %s", payload)
	}
}

func TestCinnabarLookupForwardsValidBatches(t *testing.T) {
	api := &fakeCinnabarAPI{lookups: make(chan []string, 1), answer: []string{"22"}}
	dir, _ := startCinnabarControl(t, api)
	payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":1,"method":"cinnabar_lookup.v1","params":{"xuids":["11","22"]}}`))
	var answer struct {
		Result cinnabarLookupResultV1 `json:"result"`
	}
	if err := json.Unmarshal(payload, &answer); err != nil || len(answer.Result.Cinnabar) != 1 || answer.Result.Cinnabar[0] != "22" {
		t.Fatalf("lookup: %s", payload)
	}
	if got := <-api.lookups; len(got) != 2 {
		t.Fatalf("forwarded %v", got)
	}
	for _, params := range []string{`{}`, `{"xuids":["x"]}`, `{"xuids":["1"],"extra":true}`} {
		payload := exchange(t, dir, []byte(`{"jsonrpc":"2.0","id":2,"method":"cinnabar_lookup.v1","params":`+params+`}`))
		if err := rpcError(t, payload); err == nil || err.Code != -32602 {
			t.Fatalf("malformed lookup accepted: %s", payload)
		}
	}
}

func TestCinnabarLookupFailuresKeepTheirMeaning(t *testing.T) {
	dir, _ := startCinnabarControl(t, nil)
	request := []byte(`{"jsonrpc":"2.0","id":1,"method":"cinnabar_lookup.v1","params":{"xuids":["1"]}}`)
	if err := rpcError(t, exchange(t, dir, request)); err == nil || err.Code != codeServiceFailed {
		t.Fatalf("lookup without a connection: %+v", err)
	}
	limited := &fakeCinnabarAPI{lookups: make(chan []string, 1), err: cinnabarapi.ErrRateLimited}
	dir, _ = startCinnabarControl(t, limited)
	if err := rpcError(t, exchange(t, dir, request)); err == nil || err.Code != codeRateLimited {
		t.Fatalf("rate-limited lookup: %+v", err)
	}
}

func TestCinnabarReleaseAppearsInEvents(t *testing.T) {
	store := NewStore()
	if store.Events().CinnabarRelease != nil {
		t.Fatal("release before any announcement")
	}
	release := CinnabarReleaseV1{Channel: "beta", Version: "0.1.19", PublishedAt: "2026-10-13T00:00:00.000Z"}
	store.ObserveCinnabarRelease(release)
	if got := store.Events().CinnabarRelease; got == nil || *got != release {
		t.Fatalf("release = %+v", got)
	}
}
