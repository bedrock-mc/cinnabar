package control

import (
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

func snapshot(id uint64, offer proxy.ResourcePackOffer) proxy.ResourcePackAdmissionSnapshot {
	return proxy.ResourcePackAdmissionSnapshot{
		AttemptID: id, Offer: offer, Acquisition: proxy.ResourcePackAcquisitionNone,
		DownstreamOutcome: proxy.ResourcePackDownstreamNone,
		Application:       proxy.ResourcePackApplicationUnavailable,
	}
}

func TestStoreResetsAtNewAttemptAndRejectsStaleFinal(t *testing.T) {
	store := NewStore()
	initial := store.Status()
	if initial.SchemaVersion != 1 || initial.Lifecycle != LifecycleStarting || initial.PackAdmission.AttemptID != 0 || initial.PackAdmission.Offer != proxy.ResourcePackOfferNone {
		t.Fatalf("initial status = %+v", initial)
	}

	store.Observe(snapshot(1, proxy.ResourcePackOfferOptional))
	store.Observe(snapshot(2, proxy.ResourcePackOfferNone))
	store.Observe(snapshot(1, proxy.ResourcePackOfferRequired))
	if got := store.Status().PackAdmission; got.AttemptID != 2 || got.Offer != proxy.ResourcePackOfferNone {
		t.Fatalf("stale attempt replaced latest reset: %+v", got)
	}

	final := snapshot(2, proxy.ResourcePackOfferRequired)
	final.PackCount = 3
	store.Observe(final)
	if got := store.Status().PackAdmission; got.PackCount != 3 || got.Offer != proxy.ResourcePackOfferRequired {
		t.Fatalf("same-attempt final did not replace reset: %+v", got)
	}
}
