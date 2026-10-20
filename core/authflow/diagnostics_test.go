package authflow

import (
	"bytes"
	"context"
	"errors"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"golang.org/x/oauth2"
)

func TestCacheFailureDiagnosticSurvivesSignInAndRetryWithoutLeakingCredentials(t *testing.T) {
	for _, test := range []struct {
		name       string
		diagnostic string
		setup      func(*testing.T, string)
	}{
		{"cache is a directory", "cache_load", func(t *testing.T, path string) {
			if err := os.Mkdir(path, 0o700); err != nil {
				t.Fatal(err)
			}
		}},
		{"lock is a directory", "cache_lock:os_", func(t *testing.T, path string) {
			if err := os.Mkdir(path+".lock", 0o700); err != nil {
				t.Fatal(err)
			}
		}},
	} {
		t.Run(test.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "private-path-sentinel.json")
			test.setup(t, path)
			for range 2 {
				var output, logs bytes.Buffer
				err := Run(context.Background(), Config{
					Path: path, Writer: &output, Logger: slog.New(slog.NewTextHandler(&logs, nil)),
					DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
						t.Fatal("cache failure unexpectedly requested device authentication")
						return nil, nil
					},
				})
				if err == nil {
					t.Fatal("invalid cache succeeded")
				}
				events := decodeEvents(t, output.Bytes())
				if got := events[len(events)-1]; got.Stage != "cache" || got.Message != "Could not validate the saved account." {
					t.Fatalf("terminal event = %#v", got)
				}
				if !strings.Contains(logs.String(), "diagnostic="+test.diagnostic) {
					t.Fatalf("missing cache diagnostic: %s", logs.String())
				}
				combined := output.String() + logs.String() + err.Error()
				for _, secret := range []string{"credential-sentinel", "private-path-sentinel"} {
					if strings.Contains(combined, secret) {
						t.Fatalf("failure exposed %s", secret)
					}
				}
			}
		})
	}
}

func TestUnknownSignInFailureIsNotFormattedInDiagnostics(t *testing.T) {
	var logs bytes.Buffer
	err := Run(context.Background(), Config{
		Path: "unused", Logger: slog.New(slog.NewTextHandler(&logs, &slog.HandlerOptions{Level: slog.LevelError})),
		CachedSource: func(context.Context, authcache.Config) (oauth2.TokenSource, error) {
			return nil, errors.New("provider-credential-sentinel")
		},
	})
	if err == nil || !strings.Contains(logs.String(), "diagnostic=unclassified") || strings.Contains(logs.String(), "provider-credential-sentinel") {
		t.Fatalf("unsafe diagnostic: %s", logs.String())
	}
}
