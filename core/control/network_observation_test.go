package control

import "testing"

func TestNetworkObservationDoesNotReuseOrWithdrawNewerSession(t *testing.T) {
	store := NewStore()
	store.Observe(PackAdmission{AttemptID: 1})
	var value uint32 = 42
	store.ObserveNetwork(NetworkObservation{AttemptID: 1, LocalRuntimeID: 7, RoundTripMillis: func() *uint32 { return &value }})
	if got := store.Status().Network; got == nil || got.RoundTripMillis == nil || *got.RoundTripMillis != 42 {
		t.Fatal("active upstream estimate missing")
	}
	store.Observe(PackAdmission{AttemptID: 2})
	if store.Status().Network != nil {
		t.Fatal("new join retained old upstream latency")
	}
	store.ObserveNetwork(NetworkObservation{AttemptID: 2, LocalRuntimeID: 8, RoundTripMillis: func() *uint32 { return nil }})
	store.ObserveNetwork(NetworkObservation{AttemptID: 1})
	if got := store.Status().Network; got == nil || got.LocalRuntimeID != 8 || got.RoundTripMillis != nil {
		t.Fatal("stale withdrawal removed newer estimator or unknown became zero")
	}
	store.ObserveNetwork(NetworkObservation{AttemptID: 2})
	if store.Status().Network != nil {
		t.Fatal("disconnect retained estimator")
	}
}
