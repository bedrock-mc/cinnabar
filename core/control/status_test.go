package control

import "testing"

// Offers the relay reports; the store passes them through.
const (
	offerOptional = "optional"
	offerRequired = "required"
)

func snapshot(id uint64, offer string) PackAdmission {
	return PackAdmission{
		AttemptID: id, Offer: offer, Acquisition: packNone,
		DownstreamOutcome: packNone,
		Application:       packApplicationUnavailable,
	}
}

func TestStoreResetsAtNewAttemptAndRejectsStaleFinal(t *testing.T) {
	store := NewStore()
	initial := store.Status()
	if initial.SchemaVersion != 1 || initial.Lifecycle != LifecycleStarting || initial.PackAdmission.AttemptID != 0 || initial.PackAdmission.Offer != packNone {
		t.Fatalf("initial status = %+v", initial)
	}

	store.Observe(snapshot(1, offerOptional))
	store.Observe(snapshot(2, packNone))
	store.Observe(snapshot(1, offerRequired))
	if got := store.Status().PackAdmission; got.AttemptID != 2 || got.Offer != packNone {
		t.Fatalf("stale attempt replaced latest reset: %+v", got)
	}

	final := snapshot(2, offerRequired)
	final.PackCount = 3
	store.Observe(final)
	if got := store.Status().PackAdmission; got.PackCount != 3 || got.Offer != offerRequired {
		t.Fatalf("same-attempt final did not replace reset: %+v", got)
	}
}

func TestApplicationIsReportedOnlyForTheHandedOffLatestAttempt(t *testing.T) {
	store := NewStore()
	handed := snapshot(3, offerOptional)
	handed.DownstreamOutcome = packHandedOffOptional
	store.Observe(handed)

	store.SetApplied(2, true)
	if got := store.Status().PackAdmission.Application; got != packApplicationUnavailable {
		t.Fatalf("stale attempt applied: %q", got)
	}
	store.SetApplied(3, true)
	if got := store.Status().PackAdmission.Application; got != packApplicationApplied {
		t.Fatalf("application = %q, want applied", got)
	}
	store.SetApplied(3, false)
	if got := store.Status().PackAdmission.Application; got != packApplicationUnavailable {
		t.Fatalf("revert kept applied: %q", got)
	}

	store.SetApplied(3, true)
	store.Observe(snapshot(4, offerOptional))
	if got := store.Status().PackAdmission.Application; got != packApplicationUnavailable {
		t.Fatalf("new attempt inherited application: %q", got)
	}
	store.SetApplied(4, true)
	if got := store.Status().PackAdmission.Application; got != packApplicationUnavailable {
		t.Fatalf("attempt without a handoff applied: %q", got)
	}
}

func TestTransferIsPendingUntilNextAttemptBegins(t *testing.T) {
	store := NewStore()
	if store.Status().Transfer != nil {
		t.Fatal("fresh store reports a transfer")
	}
	store.Observe(snapshot(1, packNone))
	store.ObserveTransfer("a.example", 19132)
	store.ObserveTransfer("b.example", 19133)
	got := store.Status().Transfer
	if got == nil || got.Host != "b.example" || got.Port != 19133 || got.Sequence != 2 {
		t.Fatalf("transfer = %+v, want newest with sequence 2", got)
	}
	store.Observe(snapshot(1, offerRequired))
	if store.Status().Transfer == nil {
		t.Fatal("same-attempt update cleared the pending transfer")
	}
	store.Observe(snapshot(2, packNone))
	if store.Status().Transfer != nil {
		t.Fatal("reconnect attempt kept the pending transfer")
	}
}
