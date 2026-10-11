package control

import (
	"context"
	"errors"
	"fmt"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

type stubWorlds struct {
	reports []localworld.Status
	opened  bool
	err     error
}

func (s *stubWorlds) SetLocal(status localworld.Status) (bool, error) {
	if s.err != nil {
		return false, s.err
	}
	s.reports = append(s.reports, status)
	return s.opened, nil
}

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

func TestLocalWorldReportsReachTheRoute(t *testing.T) {
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
	if report.State != localworld.StateRunning || report.World.Name != "Home" || report.Target.Transport != localworld.TransportNetherNetLAN || report.Target.LevelName != "0123456789abcdef" {
		t.Fatalf("report = %+v %+v %+v", report, report.World, report.Target)
	}
	if call(t, dir, methodLocalWorld, `{"state":"idle"}`); len(stub.reports) != 2 || stub.reports[1].State != localworld.StateIdle {
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
		{fmt.Errorf("%w: unknown state", localworld.ErrInvalid), -32602, "unknown state"},
		{localworld.ErrNotOpen, codeWorldBusy, "no world is open"},
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
	hosted := WithInvites(&stubWorlds{}, func(_ context.Context, xuid string) error {
		invited = append(invited, xuid)
		return nil
	})
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

// The core no longer runs worlds; the launcher's world methods are unknown here.
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

func TestOpenHookRunsOnlyWhenAWorldBeginsOpening(t *testing.T) {
	stub := &stubWorlds{}
	opened := 0
	worlds := WithOpenHook(stub, func() { opened++ })
	if _, err := worlds.SetLocal(localworld.Status{State: localworld.StateIdle}); err != nil || opened != 0 {
		t.Fatalf("SetLocal() = %v, hook calls %d", err, opened)
	}
	stub.opened = true
	if _, err := worlds.SetLocal(localworld.Status{State: localworld.StateStarting}); err != nil || opened != 1 {
		t.Fatalf("SetLocal() = %v, hook calls %d", err, opened)
	}
	stub.err = errors.New("invalid")
	if _, err := worlds.SetLocal(localworld.Status{State: localworld.StateStarting}); err == nil || opened != 1 {
		t.Fatalf("a failed report ran the hook: err=%v calls=%d", err, opened)
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
