package authcache

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"testing"

	"golang.org/x/oauth2"
)

func TestSourceRecoversInvalidCacheContents(t *testing.T) {
	for _, contents := range []string{
		"", `{"access_token":`, `{"refresh_token":"old"} {}`,
		`{"refresh_token":123}`, `{"access_token":"old"}`, `null`,
		`{"refresh_token":"old","expiry":"invalid"}`,
	} {
		t.Run(contents, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "token.json")
			writeFile(t, path, []byte(contents))
			want := token("replacement", "replacement-refresh")
			requests := 0
			source, err := Source(context.Background(), Config{
				Path: path, Refresh: staticRefresh,
				Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
					requests++
					before, err := os.ReadFile(path)
					if err != nil || !bytes.Equal(before, []byte(contents)) {
						t.Fatal("cache changed before sign-in completed")
					}
					return want, nil
				},
			})
			if err != nil {
				t.Fatalf("recovery failed: %v", err)
			}
			if requests != 1 {
				t.Fatalf("sign-in requests = %d, want 1", requests)
			}
			assertCachedToken(t, path, want)
			assertPrivateFile(t, path)
			if _, err := source.Token(); err != nil {
				t.Fatalf("recovered account cannot refresh: %v", err)
			}
		})
	}
}

func TestInvalidCacheSurvivesFailedOrCancelledRecovery(t *testing.T) {
	for _, cancelled := range []bool{false, true} {
		t.Run(map[bool]string{false: "declined", true: "cancelled"}[cancelled], func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "token.json")
			contents := []byte(`{"refresh_token":"old"`)
			writeFile(t, path, contents)
			ctx, cancel := context.WithCancel(context.Background())
			defer cancel()
			requested := false
			_, err := Source(ctx, Config{
				Path: path, Refresh: staticRefresh,
				Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
					requested = true
					if cancelled {
						cancel()
						return token("replacement", "replacement-refresh"), nil
					}
					return nil, errors.New("sign-in declined")
				},
			})
			if !requested || err == nil {
				t.Fatalf("recovery requested=%v err=%v", requested, err)
			}
			if cancelled && !errors.Is(err, context.Canceled) {
				t.Fatalf("cancelled recovery returned %v", err)
			}
			after, readErr := os.ReadFile(path)
			if readErr != nil || !bytes.Equal(after, contents) {
				t.Fatal("unsuccessful recovery replaced the saved cache")
			}
		})
	}
}

func TestRecoveryReplacesThePreviousSignInGeneration(t *testing.T) {
	path := filepath.Join(t.TempDir(), "token.json")
	previous, err := Source(context.Background(), Config{
		Path: path, Refresh: staticRefresh,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			return token("previous", "previous-refresh"), nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	writeFile(t, path, []byte(`{"refresh_token":`))
	_, err = Source(context.Background(), Config{
		Path: path, Refresh: staticRefresh,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			return token("replacement", "replacement-refresh"), nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if _, err := previous.Token(); !errors.Is(err, errAccountChanged) {
		t.Fatalf("previous session adopted the recovered sign-in: %v", err)
	}
}
