package proxy

import (
	"context"
	"errors"
	"fmt"
	"io"
	"slices"
	"strings"
	"testing"
	"unicode/utf8"

	"github.com/sandertv/gophertunnel/minecraft"
	"github.com/sandertv/gophertunnel/minecraft/protocol/packet"
)

func TestTransferStateRecordsNextUpstreamAndNotifies(t *testing.T) {
	var state TransferState
	var seen []TransferTarget
	state.OnTransfer = func(target TransferTarget) { seen = append(seen, target) }
	if got := state.Upstream("first:19132"); got != "first:19132" {
		t.Fatalf("Upstream() = %q before any transfer", got)
	}
	if err := state.Record(TransferTarget{Host: " [::1] ", Port: 19133}); err != nil {
		t.Fatal(err)
	}
	if got := state.Upstream("first:19132"); got != "[::1]:19133" {
		t.Fatalf("Upstream() = %q, want [::1]:19133", got)
	}
	if want := []TransferTarget{{Host: "::1", Port: 19133}}; !slices.Equal(seen, want) {
		t.Fatalf("notified %v, want %v", seen, want)
	}
}

func TestTransferStateIgnoresUnusableTargets(t *testing.T) {
	var state TransferState
	calls := 0
	state.OnTransfer = func(TransferTarget) { calls++ }
	for _, target := range []TransferTarget{{Port: 19132}, {Host: "h"}} {
		if err := state.Record(target); err == nil {
			t.Fatalf("Record(%v) accepted an unusable target", target)
		}
	}
	if calls != 0 || state.Upstream("first:1") != "first:1" {
		t.Fatal("unusable transfer changed state")
	}
}

func TestObserveTransfersRecordsAndStillRelays(t *testing.T) {
	up := newFakeUpstream(nil)
	transfer := &packet.Transfer{Address: "next.example", Port: 19140}
	bad := &packet.Transfer{Port: 19140}
	up.reads <- packetResult{packet: bad}
	up.reads <- packetResult{packet: transfer}
	up.reads <- packetResult{err: io.EOF}
	var state TransferState
	session := observeTransfers(up, &state, nil)
	for _, want := range []packet.Packet{bad, transfer} {
		batch, err := session.ReadBatchRaw(nil)
		if err != nil || len(batch) != 1 || packetFromRaw(batch[0].Data) != want {
			t.Fatalf("ReadBatchRaw() = %v, %v; want the packet unchanged", batch, err)
		}
	}
	if got := state.Upstream("first:1"); got != "next.example:19140" {
		t.Fatalf("Upstream() = %q, want the valid transfer", got)
	}
	if _, err := session.ReadBatchRaw(nil); !errors.Is(err, io.EOF) {
		t.Fatalf("ReadBatchRaw() error = %v, want EOF", err)
	}
}

func TestObserveTransfersWithoutStateIsPassThrough(t *testing.T) {
	up := newFakeUpstream(nil)
	if observeTransfers(up, nil, nil) != upstreamSession(up) {
		t.Fatal("nil state must not wrap the session")
	}
}

func TestConsumeTransferOnDialClearsOnlyAfterSuccessfulDial(t *testing.T) {
	var state TransferState
	if err := state.Record(TransferTarget{Host: "next.example", Port: 19133}); err != nil {
		t.Fatal(err)
	}
	up := newFakeUpstream(nil)
	var dialErr error
	dial := consumeTransferOnDial(func(context.Context, *resolvedUpstreamTarget, minecraft.Dialer) (upstreamSession, error) {
		if dialErr != nil {
			return nil, dialErr
		}
		return up, nil
	}, &state)
	target := &resolvedUpstreamTarget{address: "next.example:19133"}

	dialErr = errors.New("unreachable")
	if _, err := dial(context.Background(), target, minecraft.Dialer{}); err == nil {
		t.Fatal("dial error was swallowed")
	}
	if _, ok := state.Pending(); !ok {
		t.Fatal("failed dial consumed the transfer")
	}
	dialErr = nil
	if _, err := dial(context.Background(), &resolvedUpstreamTarget{address: "other:1"}, minecraft.Dialer{}); err != nil {
		t.Fatal(err)
	}
	if _, ok := state.Pending(); !ok {
		t.Fatal("dial to an unrelated target consumed the transfer")
	}
	if _, err := dial(context.Background(), target, minecraft.Dialer{}); err != nil {
		t.Fatal(err)
	}
	if _, ok := state.Pending(); ok {
		t.Fatal("successful dial left the transfer pending")
	}
}

func TestTransferStateClearDropsPending(t *testing.T) {
	var state TransferState
	_ = state.Record(TransferTarget{Host: "h", Port: 1})
	state.Clear()
	if got := state.Upstream("first:1"); got != "first:1" {
		t.Fatalf("Upstream() = %q after Clear", got)
	}
}

func TestSelectedTargetOutranksLocalAndFallsBack(t *testing.T) {
	var selector UpstreamSelector
	dial := func(_ context.Context, address string) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{address: address}, nil
	}
	next := func(context.Context) (*resolvedUpstreamTarget, error) {
		return &resolvedUpstreamTarget{address: "fallback:1"}, nil
	}
	resolve := withSelectedTarget(&selector, dial, next)
	if target, _ := resolve(context.Background()); target.address != "fallback:1" {
		t.Fatalf("unselected = %q", target.address)
	}
	selector.Set("realm_id/9")
	if target, _ := resolve(context.Background()); target.address != "realm_id/9" {
		t.Fatalf("selected = %q", target.address)
	}
	selector.Set("")
	if target, _ := resolve(context.Background()); target.address != "fallback:1" {
		t.Fatalf("cleared = %q", target.address)
	}
	if withSelectedTarget(nil, dial, next) == nil {
		t.Fatal("nil selector must pass the resolver through")
	}
}

func TestObserveDisconnectsReportsServerReasonAndKeepsError(t *testing.T) {
	up := newFakeUpstream(nil)
	reason := &minecraft.DisconnectPacketError{Reason: 5, Message: "You are banned"}
	up.reads <- packetResult{err: fmt.Errorf("read: %w", reason)}
	var got []DisconnectInfo
	session := observeDisconnects(up, func(info DisconnectInfo) { got = append(got, info) })
	if _, err := session.ReadBatchRaw(nil); !errors.Is(err, reason) {
		t.Fatalf("ReadBatchRaw() error = %v, want the original disconnect", err)
	}
	if want := []DisconnectInfo{{Reason: 5, Message: "You are banned"}}; !slices.Equal(got, want) {
		t.Fatalf("reported %v, want %v", got, want)
	}
}

func TestReportDisconnectIgnoresOtherErrorsAndBoundsMessage(t *testing.T) {
	calls := 0
	reportDisconnect(func(DisconnectInfo) { calls++ }, io.EOF)
	reportDisconnect(nil, &minecraft.DisconnectPacketError{Message: "x"})
	if calls != 0 {
		t.Fatalf("callback ran %d times for a non-disconnect", calls)
	}
	var info DisconnectInfo
	reportDisconnect(func(i DisconnectInfo) { info = i }, &minecraft.DisconnectPacketError{Message: strings.Repeat("é", 1000)})
	if len(info.Message) == 0 || len(info.Message) > maxDisconnectMessageBytes || !utf8.ValidString(info.Message) {
		t.Fatalf("message length %d valid=%t", len(info.Message), utf8.ValidString(info.Message))
	}
}
