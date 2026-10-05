package control

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

type stubWorlds struct {
	worlds              []localworld.World
	status              localworld.Status
	opened              string
	paused              *bool
	eula                bool
	redetect, dismissed bool
	view                int
	err                 error
}

func (s *stubWorlds) List() ([]localworld.World, error) { return s.worlds, s.err }
func (s *stubWorlds) Create(spec localworld.Spec) (localworld.World, error) {
	if s.err != nil {
		return localworld.World{}, s.err
	}
	return localworld.World{ID: "0123456789abcdef", Name: spec.Name, GameMode: "survival"}, nil
}
func (s *stubWorlds) Update(id string, u localworld.Update) (localworld.World, error) {
	world := localworld.World{ID: id}
	if u.Name != nil {
		world.Name = *u.Name
	}
	if u.Difficulty != nil {
		world.Difficulty = *u.Difficulty
	}
	return world, s.err
}
func (s *stubWorlds) Delete(string) error { return s.err }
func (s *stubWorlds) Prefs(_ context.Context, u localworld.PrefsUpdate) (localworld.Prefs, error) {
	s.redetect = u.Redetect
	if u.DockerPromptDismissed != nil {
		s.dismissed = *u.DockerPromptDismissed
	}
	return localworld.Prefs{DockerPromptDismissed: s.dismissed}, s.err
}
func (s *stubWorlds) AcceptEULA() error { s.eula = true; return s.err }
func (s *stubWorlds) Open(id string, opts ...localworld.OpenOptions) error {
	s.opened = id
	if len(opts) > 0 {
		s.view = opts[0].ViewDistance
	}
	s.status = localworld.Status{State: localworld.StateStarting, WorldID: id}
	return s.err
}
func (s *stubWorlds) Close() error {
	s.status = localworld.Status{State: localworld.StateIdle}
	return s.err
}
func (s *stubWorlds) SetPaused(p bool) error {
	s.paused = &p
	return s.err
}
func (s *stubWorlds) Status() localworld.Status { return s.status }

func startWorlds(t *testing.T, worlds Worlds) string {
	t.Helper()
	dir := t.TempDir()
	server, err := StartWithWorlds(dir, NewStore(), worlds)
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	return dir
}

func call(t *testing.T, dir, method, params string) []byte {
	t.Helper()
	if params != "" {
		params = `,"params":` + params
	}
	return exchange(t, dir, []byte(fmt.Sprintf(`{"jsonrpc":"2.0","id":5,"method":%q%s}`, method, params)))
}

func TestWorldListCreateRenameDelete(t *testing.T) {
	stub := &stubWorlds{worlds: []localworld.World{{ID: "0123456789abcdef", Name: "one"}}}
	dir := startWorlds(t, stub)
	list := call(t, dir, methodWorldList, "")
	var decoded struct {
		Result WorldResultV1 `json:"result"`
	}
	if err := json.Unmarshal(list, &decoded); err != nil || len(decoded.Result.Worlds) != 1 || decoded.Result.SchemaVersion != 1 {
		t.Fatalf("list = %s (%v)", list, err)
	}
	created := call(t, dir, methodWorldCreate, `{"name":"fresh","seed":0}`)
	if !strings.Contains(string(created), `"name":"fresh"`) {
		t.Fatalf("create = %s", created)
	}
	renamed := call(t, dir, methodWorldUpdate, `{"id":"0123456789abcdef","name":"two","difficulty":"hard"}`)
	if !strings.Contains(string(renamed), `"name":"two"`) || !strings.Contains(string(renamed), `"difficulty":"hard"`) {
		t.Fatalf("rename = %s", renamed)
	}
	if deleted := call(t, dir, methodWorldDelete, `{"id":"0123456789abcdef"}`); !strings.Contains(string(deleted), `"result"`) {
		t.Fatalf("delete = %s", deleted)
	}
}

func TestWorldOpenPauseCloseReturnStatus(t *testing.T) {
	stub := &stubWorlds{}
	dir := startWorlds(t, stub)
	opened := call(t, dir, methodWorldOpen, `{"id":"0123456789abcdef"}`)
	if stub.opened != "0123456789abcdef" || !strings.Contains(string(opened), `"state":"starting"`) {
		t.Fatalf("open = %s", opened)
	}
	call(t, dir, methodWorldPause, `{"paused":true}`)
	if stub.paused == nil || !*stub.paused {
		t.Fatal("pause not forwarded")
	}
	if closed := call(t, dir, methodWorldClose, ""); !strings.Contains(string(closed), `"state":"idle"`) {
		t.Fatalf("close = %s", closed)
	}
	if status := call(t, dir, methodWorldStatus, ""); !strings.Contains(string(status), `"status"`) {
		t.Fatalf("status = %s", status)
	}
}

func TestOpenForwardsViewDistanceAndEULAAcceptanceIsExplicit(t *testing.T) {
	stub := &stubWorlds{}
	dir := startWorlds(t, stub)
	call(t, dir, methodWorldOpen, `{"id":"0123456789abcdef","view_distance":12}`)
	if stub.view != 12 {
		t.Fatalf("view distance = %d", stub.view)
	}
	assertRPCError(t, call(t, dir, methodWorldOpen, `{"id":"0123456789abcdef","view_distance":-1}`), -32602)
	assertRPCError(t, call(t, dir, methodBDSEULA, `{"accepted":false}`), -32602)
	assertRPCError(t, call(t, dir, methodBDSEULA, ""), -32602)
	if stub.eula {
		t.Fatal("EULA recorded without acceptance")
	}
	if payload := call(t, dir, methodBDSEULA, `{"accepted":true}`); !stub.eula || !strings.Contains(string(payload), `"status"`) {
		t.Fatalf("accept = %s", payload)
	}
}

func TestWorldParamValidation(t *testing.T) {
	dir := startWorlds(t, &stubWorlds{})
	for _, tc := range []struct{ method, params string }{
		{methodWorldCreate, ""}, {methodWorldCreate, `{"name":"x","bogus":1}`}, {methodWorldOpen, `{}`},
		{methodWorldUpdate, `{"id":"a"}`}, {methodWorldPause, `{"paused":"yes"}`},
		{methodWorldList, `{}`}, {methodWorldStatus, `{}`}, {methodWorldClose, `{}`},
	} {
		assertRPCError(t, call(t, dir, tc.method, tc.params), -32602)
	}
}

func TestWorldErrorsMapToCodesWithoutLeakingDetail(t *testing.T) {
	for _, tc := range []struct {
		err  error
		code int
		msg  string
	}{
		{localworld.ErrNotFound, codeWorldNotFound, "world not found"},
		{fmt.Errorf("%w: x", localworld.ErrBusy), codeWorldBusy, "another world"},
		{localworld.ErrInUse, codeWorldBusy, "world is open"},
		{localworld.ErrRuntimePending, codeWorldBusy, "still checking"},
		{localworld.ErrEULARequired, codeEULARequired, "EULA"},
		{localworld.ErrBackendUnavailable, codeBackendAbsent, "not available"},
		{localworld.ErrVanillaNeedsBDS, codeBackendAbsent, "superflat"},
		{fmt.Errorf("%w: unknown value", localworld.ErrInvalid), -32602, "unknown value"},
		{errors.New("open /Users/secret/worlds: denied"), codeWorldFailed, "world operation failed"},
	} {
		dir := startWorlds(t, &stubWorlds{err: tc.err})
		payload := call(t, dir, methodWorldDelete, `{"id":"0123456789abcdef"}`)
		assertRPCError(t, payload, tc.code)
		if !strings.Contains(string(payload), tc.msg) || strings.Contains(string(payload), "secret") {
			t.Fatalf("payload = %s", payload)
		}
	}
}

func TestWorldMethodsAreUnknownWithoutService(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	assertRPCError(t, call(t, dir, methodWorldList, ""), -32601)
}

func TestOpenHookRunsOnlyAfterSuccessfulOpen(t *testing.T) {
	stub := &stubWorlds{}
	opened := 0
	worlds := WithOpenHook(stub, func() { opened++ })
	if err := worlds.Open("0123456789abcdef"); err != nil || opened != 1 || stub.opened != "0123456789abcdef" {
		t.Fatalf("Open() = %v, hook calls %d", err, opened)
	}
	stub.err = errors.New("busy")
	if err := worlds.Open("0123456789abcdef"); err == nil || opened != 1 {
		t.Fatalf("failed Open ran the hook: err=%v calls=%d", err, opened)
	}
}

func TestClearTransferWithdrawsPendingTransfer(t *testing.T) {
	store := NewStore()
	store.ObserveTransfer(proxy.TransferTarget{Host: "a", Port: 1})
	store.ClearTransfer()
	if store.Status().Transfer != nil {
		t.Fatal("transfer still pending after ClearTransfer")
	}
}

func TestPrefsMethodReadsUpdatesAndRedetects(t *testing.T) {
	stub := &stubWorlds{}
	dir := startWorlds(t, stub)
	if payload := call(t, dir, methodPrefs, ""); !strings.Contains(string(payload), `"docker_prompt_dismissed":false`) {
		t.Fatalf("read = %s", payload)
	}
	payload := call(t, dir, methodPrefs, `{"docker_prompt_dismissed":true,"redetect":true}`)
	if !stub.dismissed || !stub.redetect || !strings.Contains(string(payload), `"docker_prompt_dismissed":true`) || !strings.Contains(string(payload), `"status"`) {
		t.Fatalf("update = %s", payload)
	}
	assertRPCError(t, call(t, dir, methodPrefs, `{"bogus":1}`), -32602)
}
