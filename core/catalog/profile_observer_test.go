package catalog

import (
	"context"
	"errors"
	"fmt"
	"testing"
)

func TestProfileObserverReportsSafeOutcomes(t *testing.T) {
	for _, test := range []struct {
		name string
		err  error
	}{
		{"ok", nil},
		{"unavailable", errors.New("private service response")},
		{"timed_out", fmt.Errorf("private response: %w", context.DeadlineExceeded)},
		{"cancelled", fmt.Errorf("private response: %w", context.Canceled)},
		{"partial", ErrProfilePartial},
	} {
		t.Run(test.name, func(t *testing.T) {
			var events []ProfileRequestEvent
			ctx := WithProfileObserver(context.Background(), func(event ProfileRequestEvent) {
				events = append(events, event)
			})
			finish := ObserveProfileRequest(ctx, "statistics")
			if len(events) != 1 || events[0].Facet != "statistics" || events[0].Outcome != "request" {
				t.Fatalf("request not published before dependency finishes: %+v", events)
			}
			finish(test.err)
			if len(events) != 2 || events[1].Outcome != test.name || events[1].Elapsed < 0 {
				t.Fatalf("unexpected safe outcome: %+v", events)
			}
		})
	}
}

func TestProfileObserverReportsRejectedAccountWithoutTransport(t *testing.T) {
	var events []ProfileRequestEvent
	ctx := WithProfileObserver(context.Background(), func(event ProfileRequestEvent) {
		events = append(events, event)
	})
	if _, err := AccountProfile(ctx, nil); err == nil {
		t.Fatal("nil account accepted")
	}
	if len(events) != 2 || events[0].Facet != "xbox_auth" || events[0].Outcome != "request" || events[1].Outcome != "unavailable" {
		t.Fatalf("account failure not observable: %+v", events)
	}
	ObserveProfileRequest(context.Background(), "statistics")(nil)
}
