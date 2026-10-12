package control

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"
)

type stubWorlds struct {
	reports []LocalWorld
	err     error
}

func (s *stubWorlds) SetLocal(report LocalWorld) error {
	if s.err != nil {
		return s.err
	}
	s.reports = append(s.reports, report)
	return nil
}

// hostedWorlds also serves world_invite.v1.
type hostedWorlds struct {
	*stubWorlds
	invite func(context.Context, string) error
}

func (w hostedWorlds) Invite(ctx context.Context, xuid string) error { return w.invite(ctx, xuid) }

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

func TestLocalWorldReportsReachTheWorlds(t *testing.T) {
	stub := &stubWorlds{}
	dir := startWorlds(t, stub)
	payload := call(t, dir, methodLocalWorld, `{"state":"running","world":{"id":"0123456789abcdef","name":"Home","game_mode":"creative","backend":"bds"},`+
		`"target":{"address":"127.0.0.1:5000","transport":"nethernet-lan","lan_address":"127.0.0.1:7551","level_name":"0123456789abcdef"}}`)
	if !strings.Contains(string(payload), `"schema_version":1`) {
		t.Fatalf("report = %s", payload)
	}
	if len(stub.reports) != 1 {
		t.Fatalf("reports = %+v", stub.reports)
	}
	report := stub.reports[0]
	if report.State != "running" || report.World.Name != "Home" || report.Target.Transport != "nethernet-lan" || report.Target.LevelName != "0123456789abcdef" {
		t.Fatalf("report = %+v %+v %+v", report, report.World, report.Target)
	}
	call(t, dir, methodLocalWorld, `{"state":"idle"}`)
	if len(stub.reports) != 2 || stub.reports[1].State != "idle" || stub.reports[1].World != nil {
		t.Fatalf("reports = %+v", stub.reports)
	}
}

func TestLocalWorldParamValidation(t *testing.T) {
	dir := startWorlds(t, &stubWorlds{})
	for _, params := range []string{"", `{"state":5}`, `{"state":"idle","bogus":1}`, `{"state":"running","target":{"address":"a","transport":"raknet","port":1}}`} {
		assertRPCError(t, call(t, dir, methodLocalWorld, params), -32602)
	}
}

func TestWorldErrorsMapToCodesWithoutLeakingDetail(t *testing.T) {
	for _, tc := range []struct {
		err  error
		code int
		msg  string
	}{
		{fmt.Errorf("%w: unknown state", ErrWorldInvalid), -32602, "unknown state"},
		{ErrWorldNotOpen, codeWorldBusy, "no world is open"},
		{errors.New("open /Users/secret/worlds: denied"), codeWorldFailed, "world operation failed"},
	} {
		dir := startWorlds(t, &stubWorlds{err: tc.err})
		payload := call(t, dir, methodLocalWorld, `{"state":"idle"}`)
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

// The launcher runs worlds now; the old world_* methods are unknown to the core.
func TestWorldMethodsAreUnknownWithoutService(t *testing.T) {
	dir := t.TempDir()
	server, err := Start(dir, NewStore())
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { _ = server.Close() })
	assertRPCError(t, call(t, dir, methodLocalWorld, ""), -32601)
	served := startWorlds(t, &stubWorlds{})
	assertRPCError(t, call(t, served, "world_list.v1", ""), -32601)
}

func TestClearTransferWithdrawsPendingTransfer(t *testing.T) {
	store := NewStore()
	store.ObserveTransfer("a", 1)
	store.ClearTransfer()
	if store.Status().Transfer != nil {
		t.Fatal("transfer still pending after ClearTransfer")
	}
}
