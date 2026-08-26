package authcache

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"strings"
	"testing"

	"golang.org/x/oauth2"
)

// makeCacheFileUnprivate makes an existing cache file violate its platform's
// private-file contract using only unprivileged operations: group/world
// access bits on Unix, an Everyone allow ACE on Windows.
func makeCacheFileUnprivate(t *testing.T, path string) {
	t.Helper()
	switch runtime.GOOS {
	case "windows":
		output, err := exec.Command("icacls", path, "/grant", "*S-1-1-0:F").CombinedOutput()
		if err != nil {
			t.Skipf("broaden ACL with icacls requires the utility on PATH: %v: %s", err, output)
		}
	default:
		if err := os.Chmod(path, 0o644); err != nil {
			t.Fatal(err)
		}
	}
}

// expectedUnprivateCode reports the fixed rejection code makeCacheFileUnprivate
// provokes on this platform.
func expectedUnprivateCode(t *testing.T) string {
	t.Helper()
	if runtime.GOOS == "windows" {
		return "broad_acl"
	}
	return "group_or_world_access"
}

func TestSourceQuarantinesUnprivateCacheAndReauthenticates(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	original := token("cached-secret", "cached-refresh-secret")
	writeToken(t, path, original)
	originalBytes := readFileBytes(t, path)
	makeCacheFileUnprivate(t, path)
	requests := 0
	var notice bytes.Buffer

	source, err := Source(context.Background(), Config{
		Path:   path,
		Writer: &notice,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return token("new-access", "new-refresh"), nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatalf("Source() error = %v", err)
	}
	if requests != 1 {
		t.Fatalf("request calls = %d, want 1", requests)
	}

	quarantinePath := path + invalidCacheSuffix
	assertFileContents(t, quarantinePath, originalBytes)
	// The quarantined sibling is stamped private after the move: it must
	// satisfy this package's own private-file contract instead of preserving
	// the group/world access or ambient grant that provoked the rejection.
	assertQuarantinePrivateBySecurityCheck(t, quarantinePath)
	if _, err := os.Lstat(path); errors.Is(err, os.ErrNotExist) {
		t.Fatal("replacement cache was never written")
	} else if err != nil {
		t.Fatal(err)
	}
	assertCacheAcceptedBySecurityCheck(t, path)

	line := notice.String()
	wantCode := expectedUnprivateCode(t)
	wantPath, err := canonicalizeCachePath(path)
	if err != nil {
		t.Fatalf("canonicalizeCachePath(%q) error = %v", path, err)
	}
	if !strings.HasPrefix(line, "AUTH_CACHE_QUARANTINED code="+wantCode+" ") ||
		!strings.HasSuffix(line, "path="+wantPath+"\n") {
		t.Fatalf("quarantine notice = %q, want AUTH_CACHE_QUARANTINED code=%s line", line, wantCode)
	}
	for _, secret := range []string{"cached-secret", "cached-refresh-secret"} {
		if strings.Contains(line, secret) {
			t.Fatalf("quarantine notice contains token material: %q", line)
		}
	}

	got, err := source.Token()
	if err != nil {
		t.Fatalf("Token() error = %v", err)
	}
	if got.RefreshToken != "new-refresh" {
		t.Fatalf("Token().RefreshToken = %q, want replacement sentinel", got.RefreshToken)
	}
	assertCachedToken(t, path, token("new-access", "new-refresh"))
}

func TestSourceQuarantineReplacesEarlierQuarantine(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	requestStub := func(context.Context, io.Writer) (*oauth2.Token, error) {
		return token("recovered-access", "recovered-refresh"), nil
	}

	first := token("first-secret", "first-refresh-secret")
	writeToken(t, path, first)
	makeCacheFileUnprivate(t, path)
	if _, err := Source(context.Background(), Config{Path: path, Request: requestStub, Refresh: staticRefresh}); err != nil {
		t.Fatalf("first Source() error = %v", err)
	}
	firstQuarantine, err := os.ReadFile(path + invalidCacheSuffix)
	if err != nil {
		t.Fatal(err)
	}

	second := token("second-secret", "second-refresh-secret")
	writeToken(t, path, second)
	makeCacheFileUnprivate(t, path)
	requests := 0
	countingStub := func(context.Context, io.Writer) (*oauth2.Token, error) {
		requests++
		return token("recovered-access", "recovered-refresh"), nil
	}
	if _, err := Source(context.Background(), Config{Path: path, Request: countingStub, Refresh: staticRefresh}); err != nil {
		t.Fatalf("second Source() error = %v", err)
	}
	if requests != 1 {
		t.Fatalf("request calls = %d, want 1", requests)
	}
	secondQuarantine, err := os.ReadFile(path + invalidCacheSuffix)
	if err != nil {
		t.Fatal(err)
	}
	if bytes.Equal(firstQuarantine, secondQuarantine) {
		t.Fatal("earlier quarantine was not replaced by the newest rejected cache")
	}
	if !bytes.Contains(secondQuarantine, []byte("second-refresh-secret")) {
		t.Fatal("newest rejected cache bytes were not preserved in the quarantine")
	}
	assertNoTokenMaterialInDirectory(t, filepath.Dir(path), "first-secret", "first-refresh-secret")
}

func TestSourceTreatsQuarantinedCacheAsAbsentWhilePreservingIt(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	quarantinePath := path + invalidCacheSuffix
	preserved := []byte(`{"access_token":"old-secret","refresh_token":"old-refresh-secret"}`)
	writeFile(t, quarantinePath, preserved)
	requests := 0

	source, err := Source(context.Background(), Config{
		Path: path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return token("fresh-access", "fresh-refresh"), nil
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatalf("Source() error = %v", err)
	}
	if requests != 1 {
		t.Fatalf("request calls = %d, want 1", requests)
	}
	assertFileContents(t, quarantinePath, preserved)
	got, err := source.Token()
	if err != nil {
		t.Fatalf("Token() error = %v", err)
	}
	if got.RefreshToken != "fresh-refresh" {
		t.Fatalf("Token().RefreshToken = %q, want fresh sentinel", got.RefreshToken)
	}
}

func TestSourceValidCacheCreatesNoQuarantine(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeToken(t, path, token("valid-access", "valid-refresh"))

	_, err := Source(context.Background(), Config{
		Path: path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			return nil, errors.New("unexpected request")
		},
		Refresh: staticRefresh,
	})
	if err != nil {
		t.Fatalf("Source() error = %v", err)
	}
	if _, err := os.Lstat(path + invalidCacheSuffix); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("Lstat(quarantine) error = %v, want not exist", err)
	}
}

func TestQuarantineRenameFailureScrubsRejectedCacheAndDoesNotReauthenticate(t *testing.T) {
	parent := t.TempDir()
	path := filepath.Join(parent, "microsoft-token.json")
	writeToken(t, path, token("blocked-secret", "blocked-refresh-secret"))
	makeCacheFileUnprivate(t, path)
	if err := os.Mkdir(path+invalidCacheSuffix, 0o700); err != nil {
		t.Fatal(err)
	}
	requests := 0

	_, err := Source(context.Background(), Config{
		Path: path,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return token("unused", "unused"), nil
		},
		Refresh: staticRefresh,
	})
	if err == nil || !strings.Contains(err.Error(), "quarantine Microsoft auth cache") {
		t.Fatalf("Source() error = %v, want bounded quarantine failure", err)
	}
	if requests != 0 {
		t.Fatalf("request calls = %d, want 0 while the rejected cache cannot be moved aside", requests)
	}
	if _, statErr := os.Lstat(path); !errors.Is(statErr, os.ErrNotExist) {
		t.Fatalf("Lstat(rejected cache) error = %v, want scrubbed identity removed", statErr)
	}
	assertNoTokenMaterialInDirectory(t, parent, "blocked-secret", "blocked-refresh-secret")
}

func TestQuarantinePrivacyFailureRemovesRejectedBytesAndFails(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeToken(t, path, token("exposed-secret", "exposed-refresh-secret"))
	makeCacheFileUnprivate(t, path)

	_, err := quarantineCacheFileWith(path, quarantineHooks{protect: func(*os.File) error {
		return errors.New("forced privacy failure containing exposed-refresh-secret")
	}})
	if err == nil || !strings.Contains(err.Error(), "secure rejected auth cache") {
		t.Fatalf("quarantineCacheFileWith() error = %v, want bounded privacy failure", err)
	}
	if strings.Contains(err.Error(), "exposed-refresh-secret") {
		t.Fatalf("quarantineCacheFileWith() error contains token material: %v", err)
	}
	for _, candidate := range []string{path, path + invalidCacheSuffix} {
		if _, statErr := os.Lstat(candidate); !errors.Is(statErr, os.ErrNotExist) {
			t.Fatalf("Lstat(%q) error = %v, want unsafe bytes removed", candidate, statErr)
		}
	}
}

func TestQuarantineLeafSwapScrubsOriginalAndPreservesReplacement(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeToken(t, path, token("exposed-secret", "exposed-refresh-secret"))
	makeCacheFileUnprivate(t, path)
	moved := path + ".moved"
	replacement := []byte("foreign replacement")

	_, err := quarantineCacheFileWith(path, quarantineHooks{afterRename: func(target string) {
		if renameErr := os.Rename(target, moved); renameErr != nil {
			t.Fatal(renameErr)
		}
		if writeErr := os.WriteFile(target, replacement, 0o600); writeErr != nil {
			t.Fatal(writeErr)
		}
	}})
	if err == nil || !strings.Contains(err.Error(), "changed during quarantine") {
		t.Fatalf("quarantineCacheFileWith() error = %v, want identity-change failure", err)
	}
	assertFileContents(t, path+invalidCacheSuffix, replacement)
	if _, statErr := os.Lstat(moved); !errors.Is(statErr, os.ErrNotExist) {
		t.Fatalf("Lstat(moved rejected cache) error = %v, want scrubbed identity removed", statErr)
	}
	assertNoTokenMaterialInDirectory(t, filepath.Dir(path), "exposed-secret", "exposed-refresh-secret")
}

func TestSourcePrivacyFailureDoesNotNotifyOrReauthenticate(t *testing.T) {
	path := filepath.Join(t.TempDir(), "microsoft-token.json")
	writeToken(t, path, token("exposed-secret", "exposed-refresh-secret"))
	makeCacheFileUnprivate(t, path)
	requests := 0
	var notice bytes.Buffer

	_, err := sourceWithQuarantine(context.Background(), Config{
		Path:   path,
		Writer: &notice,
		Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
			requests++
			return token("unused", "unused"), nil
		},
		Refresh: staticRefresh,
	}, func(path string) (string, error) {
		return quarantineCacheFileWith(path, quarantineHooks{protect: func(*os.File) error {
			return errors.New("forced privacy failure containing exposed-refresh-secret")
		}})
	})
	if err == nil || !strings.Contains(err.Error(), "quarantine Microsoft auth cache") {
		t.Fatalf("sourceWithQuarantine() error = %v, want bounded quarantine failure", err)
	}
	if strings.Contains(err.Error(), "exposed-refresh-secret") {
		t.Fatalf("sourceWithQuarantine() error contains token material: %v", err)
	}
	if requests != 0 {
		t.Fatalf("request calls = %d, want 0", requests)
	}
	if notice.Len() != 0 {
		t.Fatalf("quarantine notice = %q, want none", notice.String())
	}
}

// readFileBytes reads a file's exact bytes without ever logging them.
func readFileBytes(t *testing.T, path string) []byte {
	t.Helper()
	contents, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	return contents
}

func assertUnsafePermissionsCode(t *testing.T, err error, wantCode string) {
	t.Helper()
	if err == nil {
		t.Fatalf("security check error = nil, want rejection %q", wantCode)
	}
	var unsafe *unsafePermissionsError
	if !errors.As(err, &unsafe) {
		t.Fatalf("security check error = %v, want typed unsafe-permission failure", err)
	}
	if unsafe.code != wantCode {
		t.Fatalf("rejection code = %q, want %q", unsafe.code, wantCode)
	}
}

func assertCacheAcceptedBySecurityCheck(t *testing.T, path string) {
	t.Helper()
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := checkCacheSecurityByPath(path, info); err != nil {
		t.Fatalf("checkCacheSecurityByPath(%q) error = %v, want acceptance", path, err)
	}
}

// assertQuarantinePrivateBySecurityCheck proves the .invalid sibling passes
// the same private-file contract enforced against live caches after
// quarantineCacheFile restamps it.
func assertQuarantinePrivateBySecurityCheck(t *testing.T, path string) {
	t.Helper()
	info, err := os.Lstat(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := checkCacheSecurityByPath(path, info); err != nil {
		t.Fatalf("quarantined cache %q failed the private-file contract: %v", path, err)
	}
}
