package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/hashimthearab/rust-mcbe/core/store"
)

// fill sets every exported field reachable from v to a distinct non-zero value.
func fill(v reflect.Value, seed *int) {
	*seed++
	switch v.Kind() {
	case reflect.String:
		v.SetString(fmt.Sprint("v", *seed))
	case reflect.Bool:
		v.SetBool(true)
	case reflect.Int, reflect.Int8, reflect.Int16, reflect.Int32, reflect.Int64:
		v.SetInt(int64(*seed))
	case reflect.Uint, reflect.Uint8, reflect.Uint16, reflect.Uint32, reflect.Uint64:
		v.SetUint(uint64(*seed))
	case reflect.Float32, reflect.Float64:
		v.SetFloat(float64(*seed) + 0.5)
	case reflect.Pointer:
		v.Set(reflect.New(v.Type().Elem()))
		fill(v.Elem(), seed)
	case reflect.Slice:
		v.Set(reflect.MakeSlice(v.Type(), 2, 2))
		for index := range v.Len() {
			fill(v.Index(index), seed)
		}
	case reflect.Array:
		for index := range v.Len() {
			fill(v.Index(index), seed)
		}
	case reflect.Map:
		key, value := reflect.New(v.Type().Key()).Elem(), reflect.New(v.Type().Elem()).Elem()
		fill(key, seed)
		fill(value, seed)
		v.Set(reflect.MakeMap(v.Type()))
		v.SetMapIndex(key, value)
	case reflect.Struct:
		for index := range v.NumField() {
			if v.Type().Field(index).IsExported() {
				fill(v.Field(index), seed)
			}
		}
	}
}

// sameWire fails when convert changes source's JSON, for a zero source and one with every field set.
func sameWire[S, W any](t *testing.T, name string, convert func(S) W) {
	t.Helper()
	var zero, full S
	fill(reflect.ValueOf(&full).Elem(), new(int))
	for _, source := range []S{zero, full} {
		want, err := json.Marshal(source)
		if err != nil {
			t.Fatal(err)
		}
		got, err := json.Marshal(convert(source))
		if err != nil {
			t.Fatal(err)
		}
		if !bytes.Equal(got, want) {
			t.Errorf("%s: control wire %s, source %s", name, got, want)
		}
	}
}

// The control plane's wire types must encode exactly as the packages behind them did, field for field.
func TestControlWireMatchesTheSourceJSON(t *testing.T) {
	sameWire(t, "realm", wireRealm)
	sameWire(t, "friend", func(friend catalog.Friend) control.Friend { return control.Friend(friend) })
	sameWire(t, "person", wirePerson)
	sameWire(t, "featured server", wireFeaturedServer)
	sameWire(t, "profile", wireProfile)
	sameWire(t, "ping", func(result catalog.PingResult) control.PingResult { return control.PingResult(result) })
	sameWire(t, "home", wireHome)
	sameWire(t, "message event", func(event control.MessageEvent) catalog.MessageEvent { return catalog.MessageEvent(event) })

	sameWire(t, "store page", wireStorePage)
	sameWire(t, "store search", wireStoreSearchResults)
	sameWire(t, "store offer", wireStoreOfferDetail)
	sameWire(t, "store row more", wireStoreRowMore)
	sameWire(t, "store balance", func(balance store.Balance) control.StoreBalance { return control.StoreBalance(balance) })
	sameWire(t, "store entitlements", func(owned store.Entitlements) control.StoreEntitlements { return control.StoreEntitlements(owned) })
	sameWire(t, "store purchase", func(result store.PurchaseResult) control.StorePurchaseResult {
		return control.StorePurchaseResult(result)
	})
	sameWire(t, "store query", func(query control.StoreSearchQuery) store.SearchQuery { return store.SearchQuery(query) })
	sameWire(t, "store purchase request", func(request control.StorePurchaseRequest) store.PurchaseRequest {
		return store.PurchaseRequest(request)
	})

	sameWire(t, "world", func(world localworld.World) control.World { return control.World(world) })
	sameWire(t, "world status", wireWorldStatus)
	sameWire(t, "world prefs", func(prefs localworld.Prefs) control.WorldPrefs { return control.WorldPrefs(prefs) })
	sameWire(t, "world spec", func(spec control.WorldSpec) localworld.Spec { return localworld.Spec(spec) })
	sameWire(t, "world update", func(update control.WorldUpdate) localworld.Update { return localworld.Update(update) })
	sameWire(t, "world prefs update", func(update control.WorldPrefsUpdate) localworld.PrefsUpdate {
		return localworld.PrefsUpdate(update)
	})

	sameWire(t, "pack admission", wirePackAdmission)
	sameWire(t, "connect progress", wireConnectProgress)
	sameWire(t, "server trust", func(prompt proxy.ServerTrustPrompt) control.ServerTrustPrompt {
		return control.ServerTrustPrompt(prompt)
	})
	sameWire(t, "position", func(position proxy.ForwardedPosition) control.Position { return control.Position(position) })
}

// The status store's idle and handed-off pack values must be the ones the relay reports.
func TestStatusStoreUnderstandsRelayAdmissions(t *testing.T) {
	idle := proxy.ResourcePackAdmissionSnapshot{
		Offer: proxy.ResourcePackOfferNone, Acquisition: proxy.ResourcePackAcquisitionNone,
		DownstreamOutcome: proxy.ResourcePackDownstreamNone, Application: proxy.ResourcePackApplicationUnavailable,
	}
	if got := control.NewStore().Status().PackAdmission; got != wirePackAdmission(idle) {
		t.Fatalf("idle admission = %+v, want %+v", got, wirePackAdmission(idle))
	}
	for _, outcome := range []proxy.ResourcePackDownstreamOutcome{proxy.ResourcePackDownstreamHandedOffOptional, proxy.ResourcePackDownstreamHandedOffRequired} {
		status := control.NewStore()
		handed := idle
		handed.AttemptID, handed.DownstreamOutcome = 1, outcome
		status.Observe(wirePackAdmission(handed))
		status.SetApplied(1, true)
		if got := status.Status().PackAdmission.Application; got != proxy.ResourcePackApplicationApplied {
			t.Fatalf("%s: application = %q", outcome, got)
		}
	}
}

// Classified errors keep their message and chain, and match only the sentinels they wrap.
func TestClassifiedErrorsKeepTheirMessageAndChain(t *testing.T) {
	invalid := worldErr(fmt.Errorf("%w: unknown value", localworld.ErrInvalid))
	if !errors.Is(invalid, control.ErrWorldInvalid) || !errors.Is(invalid, localworld.ErrInvalid) ||
		errors.Is(invalid, control.ErrWorldNotFound) || invalid.Error() != "invalid world settings: unknown value" {
		t.Fatalf("classified invalid = %v", invalid)
	}
	if worldErr(nil) != nil {
		t.Fatal("nil error classified as a failure")
	}
	signedOut := classify(fmt.Errorf("store: %w", control.ErrSignedOut), &storeSentinels)
	if !errors.Is(signedOut, control.ErrSignedOut) || errors.Is(signedOut, control.ErrPurchaseBusy) {
		t.Fatalf("classified sign-out = %v", signedOut)
	}
	// world_* replies show a sentinel's own message, which must stay the one clients already see.
	for _, pair := range worldSentinels {
		if pair.reported != control.ErrWorldInvalid && pair.reported.Error() != pair.wrapped.Error() {
			t.Errorf("control reports %q for %q", pair.reported, pair.wrapped)
		}
	}
}

// A malformed offer id or continuation is refused before the session checks the account.
func TestStoreControlRefusesMalformedIDsBeforeTheSession(t *testing.T) {
	checks := 0
	market := storeControl{store.NewSession(func() (*authcache.Account, error) {
		checks++
		return nil, control.ErrSignedOut
	})}
	if _, err := market.Offer(context.Background(), "bad id"); !errors.Is(err, control.ErrStoreInvalidRequest) {
		t.Fatalf("Offer() = %v", err)
	}
	if _, err := market.MoreOffers(context.Background(), ""); !errors.Is(err, control.ErrStoreInvalidRequest) {
		t.Fatalf("MoreOffers() = %v", err)
	}
	if checks != 0 {
		t.Fatalf("account checked %d times for malformed requests", checks)
	}
	if _, err := market.Balances(context.Background()); !errors.Is(err, control.ErrSignedOut) || checks != 1 {
		t.Fatalf("Balances() = %v after %d checks", err, checks)
	}
}

func TestPacketDelayControlReportsTheRelayLease(t *testing.T) {
	delay := packetDelayControl{new(proxy.PacketDelay)}
	if delay.Lease() != proxy.PacketDelayLease {
		t.Fatalf("lease = %v", delay.Lease())
	}
	if err := delay.SetWithPosition(proxy.MaxPacketDelayMS+1, false); err == nil {
		t.Fatal("delay above the relay limit accepted")
	}
	if session, position := delay.PositionSnapshot(); session != 0 || position != nil {
		t.Fatalf("snapshot = %d, %v", session, position)
	}
}
