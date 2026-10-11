package control

import (
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"strings"
	"testing"
)

type stubWorlds struct {
	worlds              []World
	status              WorldStatus
	opened              string
	paused              *bool
	eula                bool
	redetect, dismissed bool
	view                int
	err                 error
}

func (s *stubWorlds) List() ([]World, error) { return s.worlds, s.err }
func (s *stubWorlds) Create(spec WorldSpec) (World, error) {
	if s.err != nil {
		return World{}, s.err
	}
	return World{ID: "0123456789abcdef", Name: spec.Name, GameMode: "survival"}, nil
}
func (s *stubWorlds) Update(id string, u WorldUpdate) (World, error) {
	world := World{ID: id}
	if u.Name != nil {
		world.Name = *u.Name
	}
	if u.Difficulty != nil {
		world.Difficulty = *u.Difficulty
	}
	return world, s.err
}
func (s *stubWorlds) Delete(string) error { return s.err }
func (s *stubWorlds) Prefs(_ context.Context, u WorldPrefsUpdate) (WorldPrefs, error) {
	s.redetect = u.Redetect
	if u.DockerPromptDismissed != nil {
		s.dismissed = *u.DockerPromptDismissed
	}
	return WorldPrefs{DockerPromptDismissed: s.dismissed}, s.err
}
func (s *stubWorlds) AcceptEULA() error { s.eula = true; return s.err }
func (s *stubWorlds) Open(id string, viewDistance int) error {
	s.opened, s.view = id, viewDistance
	s.status = WorldStatus{State: "starting", WorldID: id}
	return s.err
}
func (s *stubWorlds) Close() error {
	s.status = WorldStatus{State: "idle"}
	return s.err
}
func (s *stubWorlds) SetPaused(p bool) error {
	s.paused = &p
	return s.err
}
func (s *stubWorlds) Status() WorldStatus { return s.status }

// hostedWorlds also serves world_invite.v1.
type hostedWorlds struct {
	*stubWorlds
	invite func(context.Context, string) error
}

func (w hostedWorlds) Invite(ctx context.Context, xuid string) error { return w.invite(ctx, xuid) }

// bareWorlds offers only the required Worlds methods.
type bareWorlds struct{ Worlds }

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
	stub := &stubWorlds{worlds: []World{{ID: "0123456789abcdef", Name: "one"}}}
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
		{ErrWorldNotFound, codeWorldNotFound, "world not found"},
		{fmt.Errorf("%w: x", ErrWorldBusy), codeWorldBusy, "another world"},
		{ErrWorldInUse, codeWorldBusy, "world is open"},
		{ErrWorldRuntimePending, codeWorldBusy, "still checking"},
		{ErrEULARequired, codeEULARequired, "EULA"},
		{ErrWorldBackendUnavailable, codeBackendAbsent, "not available"},
		{fmt.Errorf("%w: unknown value", ErrWorldInvalid), -32602, "unknown value"},
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

func TestWorldInviteReachesTheHostOnlyWithAValidXUID(t *testing.T) {
	var invited []string
	hosted := hostedWorlds{stubWorlds: &stubWorlds{}, invite: func(_ context.Context, xuid string) error {
		invited = append(invited, xuid)
		return nil
	}}
	dir := startWorlds(t, hosted)
	for _, params := range []string{"", `{}`, `{"xuid":""}`, `{"xuid":"12a"}`, `{"xuid":"1","extra":1}`, `{"xuid":"123456789012345678901"}`} {
		assertRPCError(t, call(t, dir, methodWorldInvite, params), -32602)
	}
	if payload := call(t, dir, methodWorldInvite, `{"xuid":"2535400000000000"}`); !strings.Contains(string(payload), `"result"`) {
		t.Fatalf("invite = %s", payload)
	}
	if len(invited) != 1 || invited[0] != "2535400000000000" {
		t.Fatalf("invited = %v", invited)
	}
	unhosted := startWorlds(t, &stubWorlds{})
	assertRPCError(t, call(t, unhosted, methodWorldInvite, `{"xuid":"2535400000000000"}`), codeWorldBusy)
}

// Hosts without an editor or backend leave those methods unknown but still list, open and close.
func TestOptionalWorldMethodsAreUnknownWithoutTheirCapability(t *testing.T) {
	stub := &stubWorlds{worlds: []World{{ID: "0123456789abcdef"}}}
	dir := startWorlds(t, bareWorlds{stub})
	for _, tc := range []struct{ method, params string }{
		{methodWorldCreate, `{"name":"x"}`}, {methodWorldUpdate, `{"id":"a","name":"b"}`}, {methodWorldDelete, `{"id":"a"}`},
		{methodWorldPause, `{"paused":true}`}, {methodBDSEULA, `{"accepted":true}`}, {methodPrefs, ""},
	} {
		assertRPCError(t, call(t, dir, tc.method, tc.params), -32601)
	}
	if stub.paused != nil || stub.eula {
		t.Fatal("an unsupported method reached the host")
	}
	if list := call(t, dir, methodWorldList, ""); !strings.Contains(string(list), `"id":"0123456789abcdef"`) {
		t.Fatalf("list = %s", list)
	}
	if opened := call(t, dir, methodWorldOpen, `{"id":"0123456789abcdef"}`); !strings.Contains(string(opened), `"state":"starting"`) {
		t.Fatalf("open = %s", opened)
	}
	assertRPCError(t, call(t, dir, methodWorldInvite, `{"xuid":"2535400000000000"}`), codeWorldBusy)
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

func TestClearTransferWithdrawsPendingTransfer(t *testing.T) {
	store := NewStore()
	store.ObserveTransfer("a", 1)
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
