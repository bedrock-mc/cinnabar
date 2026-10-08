package proxy

import (
	"context"
	"errors"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/localworld"
)

func onlineStub(address string) func(context.Context) (*resolvedUpstreamTarget, error) {
	return func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{address: address}, nil
	}
}

func TestWithLocalTargetRoutesLocalThenFallsBackOnline(t *testing.T) {
	selected := true
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: "127.0.0.1:5000", Transport: localworld.TransportRakNet}, selected, nil
	}, onlineStub("online:19132"))
	target, err := resolve(context.Background())
	if err != nil || target.address != "127.0.0.1:5000" || target.network == nil {
		t.Fatalf("local target = %+v, %v", target, err)
	}
	selected = false
	if target, err = resolve(context.Background()); err != nil || target.address != "online:19132" {
		t.Fatalf("online fallback = %+v, %v", target, err)
	}
}

func TestWithLocalTargetSurfacesLocalErrorsWithoutGoingOnline(t *testing.T) {
	boom := errors.New("local failed")
	resolve := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{}, false, boom
	}, onlineStub("online"))
	if _, err := resolve(context.Background()); !errors.Is(err, boom) {
		t.Fatalf("err = %v", err)
	}
}

func TestWithLocalTargetNilIsOnlineResolver(t *testing.T) {
	target, err := withLocalTarget(nil, onlineStub("online"))(context.Background())
	if err != nil || target.address != "online" {
		t.Fatalf("target = %+v, %v", target, err)
	}
}

func TestPendingTransferOutranksSelectedLocalWorld(t *testing.T) {
	var transfers TransferState
	local := withLocalTarget(func(context.Context) (localworld.ConnectionTarget, bool, error) {
		return localworld.ConnectionTarget{Address: "127.0.0.1:5000", Transport: localworld.TransportRakNet}, true, nil
	}, onlineStub("online:19132"))
	dial := func(_ context.Context, address string) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{address: address}, nil
	}
	resolve := withPendingTransfer(&transfers, dial, local)
	if target, err := resolve(context.Background()); err != nil || target.address != "127.0.0.1:5000" {
		t.Fatalf("before transfer = %+v, %v", target, err)
	}
	if err := transfers.Record(TransferTarget{Host: "next.example", Port: 19133}); err != nil {
		t.Fatal(err)
	}
	if target, err := resolve(context.Background()); err != nil || target.address != "next.example:19133" {
		t.Fatalf("after transfer = %+v, %v", target, err)
	}
}
