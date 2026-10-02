package authcache

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"testing"
	"time"

	"golang.org/x/oauth2"
)

func TestSourceMissingCacheRequestsAndPublishes(t *testing.T) {
	path := filepath.Join(t.TempDir(), "nested", "microsoft-token.json")
	want := token("requested", "requested-refresh")
	requests := 0

	source, err := Source(context.Background(), Config{
		Path: path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return want, nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatalf("Source() error = %v", err)
	}
	if requests != 1 {
		t.Fatalf("request calls = %d, want 1", requests)
	}
	assertCachedToken(t, path, want)
	assertPrivateFile(t, path)

	got, err := source.Token()
	if err != nil {
		t.Fatalf("Token() error = %v", err)
	}
	if got.AccessToken != want.AccessToken || got.RefreshToken != want.RefreshToken {
		t.Fatalf("Token() = %#v, want access/refresh sentinel", got)
	}
}

func TestSourceValidCacheRefreshesAndPersistsRotation(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeToken(t, path, token("cached", "cached-refresh"))
	validated := token("validated", "rotated-refresh-1")
	rotated := token("rotated", "rotated-refresh-2")
	sourceTokens := []*oauth2.Token{validated, rotated}
	refreshes := 0
	requests := 0

	source, err := Source(context.Background(), Config{
		Path: path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return nil, errors.New("unexpected request")
		},
		Refresh: func(cached *oauth2.Token, _ io.Writer) oauth2.TokenSource {
			refreshes++
			if cached.RefreshToken != "cached-refresh" {
				t.Fatalf("refresh token = %q, want cached sentinel", cached.RefreshToken)
			}
			return tokenSequence(sourceTokens...)
		},
	})
	if err != nil {
		t.Fatalf("Source() error = %v", err)
	}
	if refreshes != 1 || requests != 0 {
		t.Fatalf("refresh/request calls = %d/%d, want 1/0", refreshes, requests)
	}
	assertCachedToken(t, path, validated)

	got, err := source.Token()
	if err != nil {
		t.Fatalf("Token() error = %v", err)
	}
	if got.AccessToken != rotated.AccessToken {
		t.Fatalf("Token().AccessToken = %q, want %q", got.AccessToken, rotated.AccessToken)
	}
	assertCachedToken(t, path, rotated)
}

func TestSourceOversizedRefreshedTokenDoesNotReplaceExistingCache(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeToken(t, path, token("cached", "cached-refresh"))
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	oversized := token("rotated", strings.Repeat("\x00", maxCacheSize/6))

	_, err = Source(context.Background(), Config{
		Path: path,
		Refresh: func(*oauth2.Token, io.Writer) oauth2.TokenSource {
			return oauth2.StaticTokenSource(oversized)
		},
	})
	if err == nil {
		t.Fatal("Source() error = nil, want oversized serialized token rejection")
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(after, before) {
		t.Fatal("oversized refreshed token replaced the existing cache")
	}
}

func TestSourceExpiredRefreshRequestsOnce(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeToken(t, path, token("expired", "expired-refresh"))
	want := token("replacement", "replacement-refresh")
	requests := 0
	refreshes := 0

	source, err := Source(context.Background(), Config{
		Path: path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return want, nil
		},
		Refresh: func(cached *oauth2.Token, _ io.Writer) oauth2.TokenSource {
			refreshes++
			if cached.RefreshToken == "expired-refresh" {
				return oauth2.StaticTokenSource(nil)
			}
			return oauth2.StaticTokenSource(cached)
		},
	})
	if err != nil {
		t.Fatalf("Source() error = %v", err)
	}
	if requests != 1 || refreshes != 2 {
		t.Fatalf("request/refresh calls = %d/%d, want 1/2", requests, refreshes)
	}
	assertCachedToken(t, path, want)
	if _, err := source.Token(); err != nil {
		t.Fatalf("Token() error = %v", err)
	}
}

func TestSourceRejectsMalformedOversizedAndLinkedCaches(t *testing.T) {
	tests := []struct {
		name  string
		setup func(t *testing.T, path string)
	}{
		{
			name: "malformed",
			setup: func(t *testing.T, path string) {
				writeFile(t, path, []byte(`{"access_token":`))
			},
		},
		{
			name: "trailing JSON",
			setup: func(t *testing.T, path string) {
				writeFile(t, path, []byte(`{"refresh_token":"secret"} {}`))
			},
		},
		{
			name: "missing refresh token",
			setup: func(t *testing.T, path string) {
				writeFile(t, path, []byte(`{"access_token":"secret"}`))
			},
		},
		{
			name: "oversized",
			setup: func(t *testing.T, path string) {
				writeFile(t, path, bytes.Repeat([]byte("x"), 64*1024+1))
			},
		},
		{
			name: "directory",
			setup: func(t *testing.T, path string) {
				if err := os.Mkdir(path, 0o700); err != nil {
					t.Fatal(err)
				}
			},
		},
		{
			name: "symbolic link",
			setup: func(t *testing.T, path string) {
				target := filepath.Join(filepath.Dir(path), "target.json")
				writeToken(t, target, token("secret-access", "secret-refresh"))
				if err := os.Symlink(target, path); err != nil {
					if runtime.GOOS == "windows" {
						t.Skipf("creating symlink requires Windows Developer Mode: %v", err)
					}
					t.Fatal(err)
				}
			},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			path := filepath.Join(t.TempDir(), "microsoft-token.json")
			tt.setup(t, path)
			requests := 0
			_, err := Source(context.Background(), Config{
				Path: path,
				Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
					requests++
					return token("new-access", "new-refresh"), nil
				},
				Refresh: staticRefresh,
			})
			if err == nil {
				t.Fatal("Source() error = nil, want unsafe cache rejection")
			}
			if requests != 0 {
				t.Fatalf("request calls = %d, want 0", requests)
			}
		})
	}
}

func TestSourceCancellationDoesNotPublish(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	requests := 0

	_, err := Source(ctx, Config{
		Path: path,
		Request: func(ctx context.Context, _ io.Writer) (*oauth2.Token, error) {
			requests++
			return nil, ctx.Err()
		},
		Refresh: staticRefresh,
	})
	if !errors.Is(err, context.Canceled) {
		t.Fatalf("Source() error = %v, want context.Canceled", err)
	}
	if requests != 0 {
		t.Fatalf("request calls = %d, want 0", requests)
	}
	if _, err := os.Lstat(path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("Lstat(cache) error = %v, want not exist", err)
	}
	entries, err := os.ReadDir(filepath.Dir(path))
	if err != nil && !errors.Is(err, os.ErrNotExist) {
		t.Fatal(err)
	}
	if len(entries) != 0 {
		t.Fatalf("cache directory entries = %v, want none", entries)
	}
}

func token(access, refresh string) *oauth2.Token {
	return &oauth2.Token{
		AccessToken:  access,
		RefreshToken: refresh,
		TokenType:    "Bearer",
		Expiry:       time.Now().Add(time.Hour).UTC().Truncate(time.Second),
	}
}

func staticRefresh(tok *oauth2.Token, _ io.Writer) oauth2.TokenSource {
	return oauth2.StaticTokenSource(tok)
}

type sequenceSource struct {
	tokens []*oauth2.Token
}

func tokenSequence(tokens ...*oauth2.Token) oauth2.TokenSource {
	return &sequenceSource{tokens: tokens}
}

func (s *sequenceSource) Token() (*oauth2.Token, error) {
	if len(s.tokens) == 0 {
		return nil, errors.New("token sequence exhausted")
	}
	tok := s.tokens[0]
	s.tokens = s.tokens[1:]
	return tok, nil
}

func writeToken(t *testing.T, path string, tok *oauth2.Token) {
	t.Helper()
	b, err := json.Marshal(tok)
	if err != nil {
		t.Fatal(err)
	}
	writeFile(t, path, b)
}

func writeFile(t *testing.T, path string, contents []byte) {
	t.Helper()
	if err := os.WriteFile(path, contents, 0o600); err != nil {
		t.Fatal(err)
	}
	stampPrivateACL(t, path)
}

func assertCachedToken(t *testing.T, path string, want *oauth2.Token) {
	t.Helper()
	b, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	var got oauth2.Token
	if err := json.Unmarshal(b, &got); err != nil {
		t.Fatalf("decode cache: %v", err)
	}
	if got.AccessToken != want.AccessToken || got.RefreshToken != want.RefreshToken {
		t.Fatalf("cached token = %#v, want access/refresh sentinel", &got)
	}
	if strings.Contains(string(b), "unexpected") {
		t.Fatalf("cache contains unexpected data: %s", b)
	}
}

func assertPrivateFile(t *testing.T, path string) {
	t.Helper()
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}
	if !info.Mode().IsRegular() || info.Mode()&os.ModeSymlink != 0 {
		t.Fatalf("cache mode = %v, want regular non-link", info.Mode())
	}
	if runtime.GOOS != "windows" && info.Mode().Perm() != 0o600 {
		t.Fatalf("cache permissions = %o, want 600", info.Mode().Perm())
	}
}
