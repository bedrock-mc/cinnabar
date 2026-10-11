package launcher

import (
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
	s.logger.Info("profile facet", fields...)
}
