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
	if requests != 1 {
		t.Fatalf("request calls = %d, want 1", requests)
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

func TestSourceRejectsExistingCacheThroughLinkedParent(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "attacker")
	if err := os.Mkdir(target, 0o700); err != nil {
		t.Fatal(err)
	}
	want := token("existing-secret", "existing-refresh-secret")
	targetCache := filepath.Join(target, "microsoft-token.json")
	writeToken(t, targetCache, want)
	before, err := os.ReadFile(targetCache)
	if err != nil {
		t.Fatal(err)
	}
	linkedParent := filepath.Join(root, "linked-parent")
	makeLinkedDirectory(t, linkedParent, target)
	requests := 0

	_, err = Source(context.Background(), Config{
		Path: filepath.Join(linkedParent, "microsoft-token.json"),
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return token("new-secret", "new-refresh-secret"), nil
		},
		Refresh: staticRefresh,
	})
	if err == nil {
		t.Fatal("Source() error = nil, want linked parent rejection")
	}
	if strings.Contains(err.Error(), "existing-secret") || strings.Contains(err.Error(), "existing-refresh-secret") {
		t.Fatalf("Source() error contains token material: %v", err)
	}
	if requests != 0 {
		t.Fatalf("request calls = %d, want 0", requests)
	}
	after, err := os.ReadFile(targetCache)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(after, before) {
		t.Fatal("existing target cache changed through linked parent")
	}
}

func TestSourceRejectsMissingCacheThroughLinkedParent(t *testing.T) {
	root := t.TempDir()
	target := filepath.Join(root, "attacker")
	if err := os.Mkdir(target, 0o700); err != nil {
		t.Fatal(err)
	}
	linkedParent := filepath.Join(root, "linked-parent")
	makeLinkedDirectory(t, linkedParent, target)
	targetCache := filepath.Join(target, "nested", "microsoft-token.json")
	requests := 0

	_, err := Source(context.Background(), Config{
		Path: filepath.Join(linkedParent, "nested", "microsoft-token.json"),
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return token("new-secret", "new-refresh-secret"), nil
		},
		Refresh: staticRefresh,
	})
	if err == nil {
		t.Fatal("Source() error = nil, want linked parent rejection")
	}
	if strings.Contains(err.Error(), "new-secret") || strings.Contains(err.Error(), "new-refresh-secret") {
		t.Fatalf("Source() error contains token material: %v", err)
	}
	if requests != 0 {
		t.Fatalf("request calls = %d, want 0", requests)
	}
	if _, err := os.Lstat(targetCache); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("Lstat(attacker target) error = %v, want not exist", err)
	}
	if _, err := os.Lstat(filepath.Dir(targetCache)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("Lstat(attacker directory) error = %v, want not exist", err)
	}
}

func TestSaveParentSwapCleanupIsIdentitySafe(t *testing.T) {
	root := t.TempDir()
	parent := filepath.Join(root, "auth")
	movedParent := filepath.Join(root, "moved-auth")
	attacker := filepath.Join(root, "attacker")
	if err := os.Mkdir(parent, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := os.Mkdir(attacker, 0o700); err != nil {
		t.Fatal(err)
	}
	marker := []byte("attacker-owned-marker")
	var attackerReplacement string

	err := saveWithHooks(filepath.Join(parent, "microsoft-token.json"), token("access-secret", "refresh-secret"), saveHooks{
		afterTokenSync: func(tempPath string) error {
			if err := os.Rename(parent, movedParent); err != nil {
				return err
			}
			makeLinkedDirectory(t, parent, attacker)
			t.Cleanup(func() { _ = os.Remove(parent) })
			attackerReplacement = filepath.Join(attacker, filepath.Base(tempPath))
			if err := os.WriteFile(attackerReplacement, marker, 0o600); err != nil {
				t.Fatal(err)
			}
			return nil
		},
	})
	if err == nil {
		t.Fatal("saveWithHooks() error = nil, want parent instability rejection")
	}
	if attackerReplacement != "" {
		assertFileContents(t, attackerReplacement, marker)
		assertNoTokenMaterialInDirectory(t, movedParent, "access-secret", "refresh-secret")
		assertDirectoryFilesEmpty(t, movedParent)
	} else {
		// Windows prevents renaming the cache parent while os.Root holds its
		// directory handle. Returning the sharing error still exercises secure
		// cleanup at the deterministic post-sync failure point.
		assertNoTokenMaterialInDirectory(t, parent, "access-secret", "refresh-secret")
		assertDirectoryFilesEmpty(t, parent)
	}
	if _, err := os.Lstat(filepath.Join(attacker, "microsoft-token.json")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("Lstat(attacker cache) error = %v, want not exist", err)
	}
}

func TestSaveTempLeafSwapCleanupIsIdentitySafe(t *testing.T) {
	parent := t.TempDir()
	marker := []byte("attacker-owned-marker")
	var replacement, movedTemp string

	err := saveWithHooks(filepath.Join(parent, "microsoft-token.json"), token("access-secret", "refresh-secret"), saveHooks{
		afterTokenSync: func(tempPath string) error {
			replacement = tempPath
			movedTemp = filepath.Join(parent, "moved-original.tmp")
			if err := os.Rename(tempPath, movedTemp); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(replacement, marker, 0o600); err != nil {
				t.Fatal(err)
			}
			return nil
		},
	})
	if err == nil {
		t.Fatal("saveWithHooks() error = nil, want temporary leaf instability rejection")
	}
	assertFileContents(t, replacement, marker)
	assertFileContents(t, movedTemp, nil)
	if _, err := os.Lstat(filepath.Join(parent, "microsoft-token.json")); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("Lstat(cache) error = %v, want not exist", err)
	}
}

func TestSaveCleanupRaceDoesNotDeleteForeignReplacementWhenScrubFails(t *testing.T) {
	parent := t.TempDir()
	marker := []byte("foreign-replacement")
	movedOriginal := filepath.Join(parent, "moved-original.tmp")
	var replacement string

	err := saveWithHooks(filepath.Join(parent, "microsoft-token.json"), token("access-secret", "refresh-secret"), saveHooks{
		afterTokenSync: func(string) error {
			return errors.New("force cleanup")
		},
		scrubTemp: func(*os.File) error {
			return errors.New("forced scrub failure")
		},
		afterCleanupIdentityCheck: func(tempPath string) {
			replacement = tempPath
			if err := os.Rename(tempPath, movedOriginal); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(tempPath, marker, 0o600); err != nil {
				t.Fatal(err)
			}
		},
	})
	if err == nil || !strings.Contains(err.Error(), "secure auth cache cleanup failed") {
		t.Fatalf("saveWithHooks() error = %v, want secure cleanup failure", err)
	}
	assertFileContents(t, replacement, marker)
	contents, readErr := os.ReadFile(movedOriginal)
	if readErr != nil {
		t.Fatal(readErr)
	}
	if !bytes.Contains(contents, []byte("refresh-secret")) {
		t.Fatal("forced scrub failure did not preserve the original test sentinel")
	}
}

func TestSaveCleanupRaceDoesNotDeleteForeignReplacementAfterScrub(t *testing.T) {
	parent := t.TempDir()
	marker := []byte("foreign-replacement")
	movedOriginal := filepath.Join(parent, "moved-original.tmp")
	var replacement string

	err := saveWithHooks(filepath.Join(parent, "microsoft-token.json"), token("access-secret", "refresh-secret"), saveHooks{
		afterTokenSync: func(string) error {
			return errors.New("force cleanup")
		},
		afterCleanupIdentityCheck: func(tempPath string) {
			replacement = tempPath
			if err := os.Rename(tempPath, movedOriginal); err != nil {
				t.Fatal(err)
			}
			if err := os.WriteFile(tempPath, marker, 0o600); err != nil {
				t.Fatal(err)
			}
		},
	})
	if err == nil {
		t.Fatal("saveWithHooks() error = nil, want forced publication failure")
	}
	assertFileContents(t, replacement, marker)
	assertFileContents(t, movedOriginal, nil)
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

func assertFileContents(t *testing.T, path string, want []byte) {
	t.Helper()
	got, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if !bytes.Equal(got, want) {
		t.Fatalf("contents of %q = %q, want %q", path, got, want)
	}
}

func assertNoTokenMaterialInDirectory(t *testing.T, dir string, secrets ...string) {
	t.Helper()
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if entry.IsDir() {
			continue
		}
		contents, err := os.ReadFile(filepath.Join(dir, entry.Name()))
		if err != nil {
			t.Fatal(err)
		}
		for _, secret := range secrets {
			if bytes.Contains(contents, []byte(secret)) {
				t.Fatalf("%q contains token material", entry.Name())
			}
		}
	}
}

func assertDirectoryFilesEmpty(t *testing.T, dir string) {
	t.Helper()
	entries, err := os.ReadDir(dir)
	if err != nil {
		t.Fatal(err)
	}
	for _, entry := range entries {
		if entry.IsDir() {
			t.Fatalf("directory %q contains unexpected directory %q", dir, entry.Name())
		}
		assertFileContents(t, filepath.Join(dir, entry.Name()), nil)
	}
}
