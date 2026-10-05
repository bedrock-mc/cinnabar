package control

import (
	"context"
	"encoding/json"
	"errors"
	"maps"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

type stubServices struct {
	realms      []catalog.Realm
	friends     []catalog.Friend
	err         error
	kind, value string
	signedOut   bool
	prepared    bool
}

func (s *stubServices) Realms(context.Context) ([]catalog.Realm, error)   { return s.realms, s.err }
func (s *stubServices) Friends(context.Context) ([]catalog.Friend, error) { return s.friends, s.err }
func (s *stubServices) Connect(_ context.Context, kind, value string) error {
	s.kind, s.value = kind, value
	return s.err
}
func (s *stubServices) SignOut() error {
	s.signedOut = s.err == nil
	return s.err
}
func (s *stubServices) PrepareConnect(_ context.Context, kind, value string) error {
	s.prepared, s.kind, s.value = true, kind, value
	return s.err
}

func startServices(t *testing.T, store *Store, services Services) string {
	t.Helper()
	dir := t.TempDir()
	server, err := Start(dir, store)
	if err != nil {
		t.Fatal(err)
	}
	if services != nil {
		server.SetServices(services)
	}
	t.Cleanup(func() { _ = server.Close() })
	return dir
}

type rpcReply struct {
	Result json.RawMessage `json:"result"`
	Error  *responseError  `json:"error"`
}

func rpc(t *testing.T, dir, method, params string) rpcReply {
	t.Helper()
	var reply rpcReply
	if err := json.Unmarshal(call(t, dir, method, params), &reply); err != nil {
		t.Fatal(err)
	}
	return reply
}

func TestRealmsAndFriendsListReturnCatalogEntries(t *testing.T) {
	stub := &stubServices{
		realms:  []catalog.Realm{{Name: "R", State: "OPEN", Target: "realm_id/7"}},
		friends: []catalog.Friend{{Gamertag: "F", XUID: "123", WorldName: "W"}},
	}
	dir := startServices(t, NewStore(), stub)
	var realms realmsResultV1
	if reply := rpc(t, dir, methodRealmsList, ""); reply.Error != nil || json.Unmarshal(reply.Result, &realms) != nil ||
		realms.SchemaVersion != 1 || len(realms.Realms) != 1 || realms.Realms[0].Target != "realm_id/7" {
		t.Fatalf("realms = %+v / %+v", realms, reply.Error)
	}
	var friends friendsResultV1
	if reply := rpc(t, dir, methodFriendsList, ""); reply.Error != nil || json.Unmarshal(reply.Result, &friends) != nil ||
		len(friends.Friends) != 1 || friends.Friends[0].XUID != "123" {
		t.Fatalf("friends = %+v / %+v", friends, reply.Error)
	}
}

func TestEmptyListsEncodeAsArrays(t *testing.T) {
	dir := startServices(t, NewStore(), &stubServices{})
	if raw := string(call(t, dir, methodRealmsList, "")); !strings.Contains(raw, `"realms":[]`) {
		t.Fatalf("realms response = %s", raw)
	}
	if raw := string(call(t, dir, methodFriendsList, "")); !strings.Contains(raw, `"friends":[]`) {
		t.Fatalf("friends response = %s", raw)
	}
}

func TestServiceErrorsAreSanitized(t *testing.T) {
	for _, test := range []struct {
		err  error
		code int
	}{
		{ErrSignedOut, codeSignedOut},
		{ErrInvalidTarget, codeInvalidTarget},
		{errors.New(`open /home/user/.secret/token.json: permission denied`), codeServiceFailed},
	} {
		dir := startServices(t, NewStore(), &stubServices{err: test.err})
		reply := rpc(t, dir, methodRealmsList, "")
		if reply.Error == nil || reply.Error.Code != test.code || strings.Contains(reply.Error.Message, "/home") {
			t.Fatalf("error for %v = %+v", test.err, reply.Error)
		}
	}
}

func TestServiceMethodsWithoutServicesAreRejected(t *testing.T) {
	dir := startServices(t, NewStore(), nil)
	for _, method := range []string{methodRealmsList, methodFriendsList, methodSignOut} {
		if reply := rpc(t, dir, method, ""); reply.Error == nil || reply.Error.Code != codeServicesDisabled {
			t.Fatalf("%s error = %+v", method, reply.Error)
		}
	}
	// Account and events need only the store.
	if reply := rpc(t, dir, methodAccountStatus, ""); reply.Error != nil {
		t.Fatalf("account_status error = %+v", reply.Error)
	}
}

func TestConnectValidatesAndForwardsTarget(t *testing.T) {
	stub := &stubServices{}
	dir := startServices(t, NewStore(), stub)
	if reply := rpc(t, dir, methodConnect, `{"kind":"realm","value":"42"}`); reply.Error != nil || stub.kind != "realm" || stub.value != "42" {
		t.Fatalf("connect = %+v kind=%q value=%q", reply.Error, stub.kind, stub.value)
	}
	if reply := rpc(t, dir, methodConnect, `{"kind":"gathering","value":"5b0f2bd4-8a8e-4a6e-9d3c-0a1b2c3d4e5f"}`); reply.Error != nil || stub.kind != TargetGathering {
		t.Fatalf("gathering connect = %+v kind=%q", reply.Error, stub.kind)
	}
	for _, params := range []string{
		`{"kind":"bogus","value":"x"}`, `{"kind":"raknet","value":""}`,
		`{"kind":"raknet","value":"` + strings.Repeat("a", 300) + `"}`,
	} {
		if reply := rpc(t, dir, methodConnect, params); reply.Error == nil || reply.Error.Code != codeInvalidTarget {
			t.Fatalf("params %q error = %+v", params, reply.Error)
		}
	}
	for _, params := range []string{``, `{"kind":"raknet"}`, `{"kind":"raknet","value":"a:1","extra":1}`} {
		if reply := rpc(t, dir, methodConnect, params); reply.Error == nil || reply.Error.Code != -32602 {
			t.Fatalf("params %q error = %+v", params, reply.Error)
		}
	}
}

func TestPrepareConnectValidatesForwardsAndCancelsWithoutConnecting(t *testing.T) {
	stub := &stubServices{}
	dir := startServices(t, NewStore(), stub)
	if reply := rpc(t, dir, methodPrepareConnect, `{"kind":"raknet","value":"server.example:19132"}`); reply.Error != nil || !stub.prepared || stub.kind != TargetRakNet {
		t.Fatalf("prepare = %+v kind=%q", reply.Error, stub.kind)
	}
	if reply := rpc(t, dir, methodPrepareConnect, `{"kind":"","value":""}`); reply.Error != nil || stub.kind != "" || stub.value != "" {
		t.Fatalf("cancel preparation = %+v", reply.Error)
	}
	for _, method := range []string{methodConnect, methodPrepareConnect} {
		if reply := rpc(t, dir, method, `{"kind":"raknet","value":""}`); reply.Error == nil || reply.Error.Code != codeInvalidTarget {
			t.Fatalf("empty RakNet %s = %+v", method, reply.Error)
		}
	}
	if reply := rpc(t, dir, methodConnect, `{"kind":"","value":""}`); reply.Error == nil || reply.Error.Code != codeInvalidTarget {
		t.Fatal("prepare cancellation relaxed actual connect validation")
	}
	if reply := rpc(t, dir, methodPrepareConnect, `{"kind":"raknet","value":"server:1","extra":1}`); reply.Error == nil || reply.Error.Code != -32602 {
		t.Fatal("preparation accepted unknown parameters")
	}
}

func TestSignOutAndAccountStatusReflectStore(t *testing.T) {
	store := NewStore()
	store.SetAuth(AuthV1{State: AuthSignedIn, Gamertag: "Steve"})
	stub := &stubServices{}
	dir := startServices(t, store, stub)
	var account accountResultV1
	if reply := rpc(t, dir, methodAccountStatus, ""); json.Unmarshal(reply.Result, &account) != nil ||
		account.Account.State != AuthSignedIn || account.Account.Gamertag != "Steve" {
		t.Fatalf("account = %+v", account)
	}
	store.SetAuth(AuthV1{State: AuthSignedOut}) // the service publishes this itself
	if reply := rpc(t, dir, methodSignOut, ""); reply.Error != nil || !stub.signedOut ||
		json.Unmarshal(reply.Result, &account) != nil || account.Account.State != AuthSignedOut {
		t.Fatalf("sign_out = %+v account=%+v", reply.Error, account)
	}
	if reply := rpc(t, dir, methodSignOut, `{"x":1}`); reply.Error == nil || reply.Error.Code != -32602 {
		t.Fatalf("sign_out params error = %+v", reply.Error)
	}
}

func TestEventsCarryAuthDisconnectAndTransfer(t *testing.T) {
	store := NewStore()
	dir := startServices(t, store, nil)
	var events EventsV1
	if reply := rpc(t, dir, methodEvents, ""); json.Unmarshal(reply.Result, &events) != nil ||
		events.Auth.State != AuthSignedOut || events.Disconnect != nil || events.Transfer != nil {
		t.Fatalf("initial events = %+v", events)
	}
	store.SetAuth(AuthV1{State: AuthAwaitingCode, VerificationURI: "https://example.test/link", UserCode: "ABCD1234"})
	store.ObserveDisconnect(proxy.DisconnectInfo{Reason: 7, Message: "You are banned"})
	store.ObserveDisconnect(proxy.DisconnectInfo{Reason: 8, Message: "Server full"})
	store.ObserveTransfer(proxy.TransferTarget{Host: "next.example", Port: 19133})
	reply := rpc(t, dir, methodEvents, "")
	events = EventsV1{}
	if err := json.Unmarshal(reply.Result, &events); err != nil {
		t.Fatal(err)
	}
	if events.Auth.UserCode != "ABCD1234" || events.Auth.VerificationURI == "" ||
		events.Disconnect == nil || events.Disconnect.Message != "Server full" || events.Disconnect.Sequence != 2 ||
		events.Transfer == nil || events.Transfer.Host != "next.example" {
		t.Fatalf("events = %+v", events)
	}
	store.ObserveConnectProgress(proxy.ConnectProgress{Stage: proxy.ConnectStagePacks, PacksDone: 1, PacksTotal: 3, ReceivedBytes: 5, TotalBytes: 9})
	reply = rpc(t, dir, methodEvents, "")
	var wire struct {
		Connect map[string]any `json:"connect"`
	}
	if err := json.Unmarshal(reply.Result, &wire); err != nil {
		t.Fatal(err)
	}
	want := map[string]any{"stage": "packs", "packs_done": 1.0, "packs_total": 3.0, "received_bytes": 5.0, "total_bytes": 9.0}
	if !maps.Equal(wire.Connect, want) {
		t.Fatalf("connect wire = %v, want %v", wire.Connect, want)
	}
	store.ObserveConnectProgress(proxy.ConnectProgress{Stage: proxy.ConnectStageRealm})
	reply = rpc(t, dir, methodEvents, "")
	wire.Connect = nil
	if err := json.Unmarshal(reply.Result, &wire); err != nil {
		t.Fatal(err)
	}
	if !maps.Equal(wire.Connect, map[string]any{"stage": "realm"}) {
		t.Fatalf("realm stage wire = %v, want zero counters omitted", wire.Connect)
	}
	store.ObserveConnectProgress(proxy.ConnectProgress{})
	if got := store.Events().Connect; got != nil {
		t.Fatalf("withdrawn stage still published: %+v", got)
	}
	store.ObserveConnectProgress(proxy.ConnectProgress{Stage: proxy.ConnectStageConnecting})
	store.Observe(snapshot(1, proxy.ResourcePackOfferNone))
	store.Observe(snapshot(2, proxy.ResourcePackOfferNone))
	if got := store.Events(); got.Disconnect != nil || got.Transfer != nil || got.Connect != nil {
		t.Fatalf("new attempt kept stale events: %+v", got)
	}
}
