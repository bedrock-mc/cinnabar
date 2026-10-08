package launcher

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
)

// profileLogRecords decodes fixture diagnostics without depending on slog's timestamp formatting.
func profileLogRecords(t *testing.T, buffer *bytes.Buffer) []map[string]any {
	t.Helper()
	decoder := json.NewDecoder(bytes.NewReader(buffer.Bytes()))
	var records []map[string]any
	for decoder.More() {
		var record map[string]any
		if err := decoder.Decode(&record); err != nil {
			t.Fatal(err)
		}
		records = append(records, record)
	}
	return records
}

func TestProfileFacetLogsRateLimitEachOutcome(t *testing.T) {
	var buffer bytes.Buffer
	s := New(Config{Logger: slog.New(slog.NewJSONHandler(&buffer, nil))})
	request := catalog.ProfileRequestEvent{Facet: "statistics", Outcome: "request"}
	s.logProfileRequest(request)
	s.logProfileRequest(request)
	s.logProfileRequest(catalog.ProfileRequestEvent{Facet: "statistics", Outcome: "timed_out", Elapsed: time.Second})
	s.logProfileRequest(catalog.ProfileRequestEvent{Facet: "avatar", Outcome: "request"})
	if records := profileLogRecords(t, &buffer); len(records) != 3 {
		t.Fatalf("repeated request not limited independently: %d records", len(records))
	}
	s.profileLogMu.Lock()
	s.profileLogs["statistics/request"] = time.Now().Add(-profileLogInterval)
	s.profileLogMu.Unlock()
	s.logProfileRequest(request)
	if records := profileLogRecords(t, &buffer); len(records) != 4 {
		t.Fatalf("request remained suppressed after interval: %d records", len(records))
	}
}

func TestProfileFacetLogsCancellationBeforeTerminalReturn(t *testing.T) {
	var buffer bytes.Buffer
	started := make(chan struct{})
	s := New(Config{
		Account: testAccount(), Logger: slog.New(slog.NewJSONHandler(&buffer, nil)),
		Profile: func(ctx context.Context, _ *authcache.Account) (catalog.Profile, error) {
			close(started)
			<-ctx.Done()
			return catalog.Profile{}, ctx.Err()
		},
	})
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	done := make(chan error, 1)
	go func() { _, err := s.Profile(ctx); done <- err }()
	select {
	case <-started:
	case <-time.After(time.Second):
		t.Fatal("profile fixture did not start")
	}
	// The fixture pauses all writes here, so the buffer can be checked safely.
	if records := profileLogRecords(t, &buffer); len(records) != 2 || records[1]["facet"] != "account" || records[1]["outcome"] != "request" {
		t.Fatalf("blocked facet missing before completion: %+v", records)
	}
	cancel()
	select {
	case err := <-done:
		if !errors.Is(err, context.Canceled) {
			t.Fatalf("cancellation was not returned: %v", err)
		}
	case <-time.After(time.Second):
		t.Fatal("cancelled dependency left Profile blocked")
	}
	records := profileLogRecords(t, &buffer)
	if len(records) != 4 || records[2]["outcome"] != "cancelled" || records[3]["outcome"] != "cancelled" {
		t.Fatalf("terminal cancellation missing: %+v", records)
	}
}

func TestProfileFacetLogsNeverIncludeServiceError(t *testing.T) {
	var buffer bytes.Buffer
	s := New(Config{
		Account: testAccount(), Logger: slog.New(slog.NewJSONHandler(&buffer, nil)),
		Profile: func(context.Context, *authcache.Account) (catalog.Profile, error) {
			return catalog.Profile{}, errors.New("private service response")
		},
	})
	for range 2 {
		if _, err := s.Profile(context.Background()); err == nil {
			t.Fatal("fixture error was discarded")
		}
	}
	if strings.Contains(buffer.String(), "private") || strings.Contains(buffer.String(), "response") {
		t.Fatal("service error entered facet diagnostics")
	}
	if records := profileLogRecords(t, &buffer); len(records) != 4 || records[2]["outcome"] != "unavailable" || records[3]["outcome"] != "unavailable" {
		t.Fatalf("failure logs not safe and rate limited: %+v", records)
	}
}

func TestProfileArtworkLogsFailedDownloadsAndDisabledCache(t *testing.T) {
	images := []*catalog.Image{{URL: "https://fixture.invalid/a", Path: "cached.img"}, {URL: "https://fixture.invalid/b"}, {}}
	for _, test := range []struct {
		name    string
		err     error
		enabled bool
		outcome string
	}{
		{"partial", nil, true, "partial"},
		{"disabled", nil, false, "skipped"},
		{"deadline", context.DeadlineExceeded, true, "timed_out"},
	} {
		t.Run(test.name, func(t *testing.T) {
			event := profileArtworkOutcome(images, test.err, test.enabled)
			if event.Outcome != test.outcome || event.Requested != 2 || event.Loaded != 1 || event.Missing != 1 {
				t.Fatalf("download result hidden: %+v", event)
			}
		})
	}
	var buffer bytes.Buffer
	s := New(Config{
		Account: testAccount(), Logger: slog.New(slog.NewJSONHandler(&buffer, nil)), ArtworkDir: t.TempDir(),
		Profile: func(context.Context, *authcache.Account) (catalog.Profile, error) {
			return catalog.Profile{Gamerpic: catalog.Image{URL: "https://fixture.invalid/a"}}, nil
		},
		CacheArt: func(context.Context, string, []*catalog.Image) {},
	})
	if _, err := s.Profile(context.Background()); err != nil {
		t.Fatalf("optional art failure prevented profile publication: %v", err)
	}
	records := profileLogRecords(t, &buffer)
	var art, overall map[string]any
	for _, record := range records {
		if record["facet"] == "artwork" && record["outcome"] != "request" {
			art = record
		}
		if record["facet"] == "profile" && record["outcome"] != "request" {
			overall = record
		}
	}
	if art["outcome"] != "unavailable" || art["missing"] != float64(1) || overall["outcome"] != "partial" {
		t.Fatalf("missing cached artwork looked successful: %+v", records)
	}
	if strings.Contains(buffer.String(), "fixture.invalid") {
		t.Fatal("image URL entered diagnostics")
	}
}
