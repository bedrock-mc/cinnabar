package authflow

import (
	"bytes"
	"context"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"golang.org/x/oauth2"
)

func TestInvalidSavedAccountCompletesDeviceSignIn(t *testing.T) {
	path := filepath.Join(t.TempDir(), "token.json")
	before := []byte(`{"refresh_token":"old-credential-sentinel"`)
	if err := os.WriteFile(path, before, 0o600); err != nil {
		t.Fatal(err)
	}
	var output bytes.Buffer
	err := Run(context.Background(), Config{
		Path: path, Writer: &output, Refresh: staticRefresh,
		DeviceAuth: func(context.Context) (*oauth2.DeviceAuthResponse, error) {
			return &oauth2.DeviceAuthResponse{VerificationURI: "https://login.example.test/device", UserCode: "ABCD-1234"}, nil
		},
		DeviceToken: func(context.Context, *oauth2.DeviceAuthResponse) (*oauth2.Token, error) {
			contents, err := os.ReadFile(path)
			if err != nil || !bytes.Equal(contents, before) {
				t.Fatal("saved cache changed before device sign-in completed")
			}
			return validToken("new-access-sentinel", "new-refresh-sentinel"), nil
		},
	})
	if err != nil {
		t.Fatalf("saved-account recovery failed: %v", err)
	}
	events := decodeEvents(t, output.Bytes())
	if len(events) != 3 || events[0].Kind != "checking_cache" || events[1].Kind != "device_code" || events[2].Kind != "authenticated" || events[2].Method != "device_code" {
		t.Fatalf("unexpected recovery events: %#v", events)
	}
	for _, secret := range []string{"old-credential-sentinel", "new-access-sentinel", "new-refresh-sentinel"} {
		if strings.Contains(output.String(), secret) {
			t.Fatal("recovery event exposed credential contents")
		}
	}
	contents, err := os.ReadFile(path)
	if err != nil || !bytes.Contains(contents, []byte("new-refresh-sentinel")) {
		t.Fatal("completed device sign-in did not replace the invalid cache")
	}
}
