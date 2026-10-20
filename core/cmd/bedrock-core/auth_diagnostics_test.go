package main

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func TestAuthEventsLogsCacheFailureWithoutCredentialContentsOrPath(t *testing.T) {
	path := filepath.Join(t.TempDir(), "private-path-sentinel.json")
	if err := os.Mkdir(path, 0o700); err != nil {
		t.Fatal(err)
	}
	var output, logs bytes.Buffer
	err := run(context.Background(), []string{"-auth-events", "-auth-cache", path}, &output, &logs, nil, nil)
	if err == nil || !strings.Contains(output.String(), "Could not validate the saved account.") {
		t.Fatalf("did not reproduce cache failure: output=%s err=%v", output.String(), err)
	}
	if !strings.Contains(logs.String(), "diagnostic=cache_load") {
		t.Fatalf("missing cache failure diagnostic: %s", logs.String())
	}
	for _, secret := range []string{"private-path-sentinel", "credential-sentinel"} {
		if strings.Contains(output.String()+logs.String()+err.Error(), secret) {
			t.Fatalf("failure exposed %s", secret)
		}
	}
}
