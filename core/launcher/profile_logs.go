package launcher

import (
	"context"
	"errors"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

const profileLogInterval = 30 * time.Second

// logProfileRequest bounds repeated diagnostics independently for each facet and outcome.
// Only fixed stage/status names and elapsed time are logged, with no account or auth data.
func (s *Service) logProfileRequest(event catalog.ProfileRequestEvent) {
	now := time.Now()
	key := event.Facet + "/" + event.Outcome
	s.profileLogMu.Lock()
	if s.profileLogs == nil {
		s.profileLogs = make(map[string]time.Time)
	}
	if previous, ok := s.profileLogs[key]; ok && now.Sub(previous) < profileLogInterval {
		s.profileLogMu.Unlock()
		return
	}
	s.profileLogs[key] = now
	s.profileLogMu.Unlock()
	fields := []any{"facet", event.Facet, "outcome", event.Outcome, "elapsed_ms", event.Elapsed.Milliseconds()}
	if event.Reason != "" {
		fields = append(fields, "reason", event.Reason)
	}
	if event.Facet == "artwork" {
		fields = append(fields, "requested", event.Requested, "loaded", event.Loaded, "missing", event.Missing)
	}
	s.logger.Info("profile facet", fields...)
}

// profileArtworkOutcome counts only images with a URL; absent optional art is not a failure.
func profileArtworkOutcome(images []*catalog.Image, err error, enabled bool) catalog.ProfileRequestEvent {
	event := catalog.ProfileRequestEvent{Facet: "artwork", Outcome: "ok"}
	for _, image := range images {
		if image.URL != "" {
			event.Requested++
			if image.Path != "" {
				event.Loaded++
			} else {
				event.Missing++
			}
		}
	}
	switch {
	case !enabled:
		event.Outcome, event.Reason = "skipped", "cache_disabled"
	case errors.Is(err, context.DeadlineExceeded):
		event.Outcome = "timed_out"
	case errors.Is(err, context.Canceled):
		event.Outcome = "cancelled"
	case event.Missing > 0 && event.Loaded > 0:
		event.Outcome = "partial"
	case event.Missing > 0:
		event.Outcome = "unavailable"
	}
	return event
}
