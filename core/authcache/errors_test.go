package authcache

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"syscall"
	"testing"
)

func TestFailureDiagnosticKeepsOnlyCacheOperationAndOSErrorNumber(t *testing.T) {
	underlying := &os.PathError{Op: "open", Path: "private-path-sentinel", Err: syscall.Errno(5)}
	err := &cacheFailure{operation: "lock", err: fmt.Errorf("secret-sentinel: %w", underlying)}
	if got := FailureDiagnostic(fmt.Errorf("outer-secret: %w", err)); got != "cache_lock:os_5" {
		t.Fatalf("diagnostic = %q", got)
	}
	if !errors.Is(err, underlying) {
		t.Fatal("cache diagnostic discarded the underlying error")
	}
	if got := FailureDiagnostic(errors.New("provider-secret-sentinel")); got != "unclassified" {
		t.Fatalf("unknown diagnostic = %q", got)
	}
}

func TestPersistFailureReportsOperationAndOSErrorWithoutPath(t *testing.T) {
	blocked := filepath.Join(t.TempDir(), "private-path-sentinel")
	if err := os.WriteFile(blocked, []byte("not a directory"), 0o600); err != nil {
		t.Fatal(err)
	}
	source := &persistingSource{path: filepath.Join(blocked, "token.json"), generation: "fixture"}
	err := source.persist(token("access-sentinel", "refresh-sentinel"))
	if err == nil {
		t.Fatal("persisted a token through a regular file")
	}
	diagnostic := FailureDiagnostic(err)
	if !strings.HasPrefix(diagnostic, "cache_persist:os_") || strings.Contains(diagnostic, "sentinel") {
		t.Fatalf("unsafe or missing persist diagnostic: %s", diagnostic)
	}
}
