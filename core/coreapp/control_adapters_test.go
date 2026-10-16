package coreapp

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"reflect"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
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
	sameWire(t, "local world", localStatus)
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
		errors.Is(invalid, control.ErrWorldNotOpen) || invalid.Error() != "invalid local world report: unknown value" {
		t.Fatalf("classified invalid = %v", invalid)
	}
	if worldErr(nil) != nil {
		t.Fatal("nil error classified as a failure")
	}
	// World replies show a sentinel's own message, which must stay the one clients already see.
	for _, pair := range worldSentinels {
		if pair.reported != control.ErrWorldInvalid && pair.reported.Error() != pair.wrapped.Error() {
			t.Errorf("control reports %q for %q", pair.reported, pair.wrapped)
		}
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
