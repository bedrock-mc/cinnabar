package experience

import (
	"math"
	"strings"
	"testing"
)

// TestNetworkFrameIncludesScopedRecords delivers a network beside all fixed callback fields.
func TestNetworkFrameIncludesScopedRecords(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	sup.registered.Worlds = &WorldRegistration{}
	f := newIdleFixture(t, log, map[string]*Supervisor{"probe": sup})
	t.Cleanup(func() { f.host.Close() })
	f.placeLine(probeNode, 0, 15)
	data := make([]byte, maxBlockDataBytes)
	for x := range maxNetworkDataBytes / maxBlockDataBytes {
		if err := f.store.SetData("probe", overworldKey(BlockPos{X: int32(x), Y: 64}), data, true); err != nil {
			t.Fatal(err)
		}
	}
	anchor := BlockPos{Y: 64}
	call := Call{Neighbor: &NeighborCall{Pos: anchor, Neighbor: anchor}}
	initial := f.snapshotOf(call, anchor)
	initial.req.Seq = math.MaxUint64
	body, err := encodeFrame(Request{Callback: &initial.req})
	if err != nil {
		t.Fatal(err)
	}
	// Fill the fixed metadata to exercise the frame boundary without extra world blocks.
	f.host.worldID += strings.Repeat("w", maxFrameBytes-len(body))
	actor := f.actorID()
	for _, address := range stateAddresses(f.instance.handle.ID, &actor) {
		if err := f.store.setState("probe", address, StoreKey{Scope: address.Scope, Schema: 1}, data, true); err != nil {
			t.Fatal(err)
		}
	}
	snap := f.snapshotOf(call, anchor)
	snap.req.Seq = math.MaxUint64
	if _, err := encodeFrame(Request{Callback: &snap.req}); err != nil {
		t.Fatalf("network and scoped records exceed the callback frame: %v", err)
	}
	if network := snap.req.Network; network == nil || !network.Truncated || len(network.Blocks) == 0 || len(network.Blocks) >= len(initial.req.Network.Blocks) {
		t.Fatalf("network was not truncated to a nonempty prefix: %+v", network)
	}
	if records := snap.req.Info.Context.Stores; len(records) != 3 {
		t.Fatalf("got %d scoped records, want all three", len(records))
	} else {
		for _, record := range records {
			if record.Data == nil || len(*record.Data) != 2*len(data) {
				t.Fatal("a scoped record was truncated")
			}
		}
	}
	if outcome, err := sup.Call(snap.req); err != nil || outcome.Committed == nil {
		t.Fatalf("truncated callback was not delivered: outcome %+v, error %v", outcome, err)
	}
}

// TestOversizedFixedCallbackPreservesGuestBudget refuses invalid host frames without a guest strike.
func TestOversizedFixedCallbackPreservesGuestBudget(t *testing.T) {
	log, _ := testLog(t)
	sup, _ := startFake(t, "ok", log, startOptions{})
	req := probeInteract(0)
	req.Info.WorldID = strings.Repeat("w", maxFrameBytes)
	for i := 1; i <= strikeLimit; i++ {
		_, err := sup.Call(req)
		if err == nil || !strings.Contains(err.Error(), "encoding callback") || !strings.Contains(err.Error(), "exceeds") {
			t.Fatalf("oversized fixed callback error = %v", err)
		}
		if len(sup.strikes) != 0 || sup.Quarantined() {
			t.Fatalf("failure %d: %d strikes, quarantined %v", i, len(sup.strikes), sup.Quarantined())
		}
	}
}
