package catalog

import (
	"context"
	"errors"
	"time"
)

// ProfileRequestEvent contains timing and status only, never account data or service errors.
type ProfileRequestEvent struct {
	Facet   string
	Outcome string
	Elapsed time.Duration
	Reason  string
}

// ErrProfilePartial marks a completed request whose optional facets were unavailable.
var ErrProfilePartial = errors.New("profile partially unavailable")

type profileObserverKey struct{}

// WithProfileObserver attaches an optional callback for identifying slow Profile dependencies.
func WithProfileObserver(ctx context.Context, observer func(ProfileRequestEvent)) context.Context {
	return context.WithValue(ctx, profileObserverKey{}, observer)
}

// ObserveProfileRequest reports a request now and returns its completion callback.
// Errors become fixed status names so credentials and service response bodies cannot enter logs.
func ObserveProfileRequest(ctx context.Context, facet string) func(error) {
	observer, _ := ctx.Value(profileObserverKey{}).(func(ProfileRequestEvent))
	if observer == nil {
		return func(error) {}
	}
	started := time.Now()
	observer(ProfileRequestEvent{Facet: facet, Outcome: "request"})
	return func(err error) {
		outcome := "ok"
		switch {
		case errors.Is(err, context.DeadlineExceeded):
			outcome = "timed_out"
		case errors.Is(err, context.Canceled):
			outcome = "cancelled"
		case errors.Is(err, ErrProfilePartial):
			outcome = "partial"
		case err != nil:
			outcome = "unavailable"
		}
		observer(ProfileRequestEvent{Facet: facet, Outcome: outcome, Elapsed: time.Since(started)})
	}
}
