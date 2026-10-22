package redact

import (
	"bytes"
	"encoding/json"
	"errors"
	"fmt"
	"log/slog"
	"strings"
	"testing"
)

// The vectors in the shared rule file give the same output here as in crash_report::redact.
func TestConformanceVectors(t *testing.T) {
	var file struct {
		Vectors []struct {
			Name, Input, Output string
			FileName            bool `json:"file_name"`
		} `json:"vectors"`
	}
	if err := json.Unmarshal(rulesJSON, &file); err != nil {
		t.Fatal(err)
	}
	if len(file.Vectors) == 0 {
		t.Fatal("the rule file has no vectors")
	}
	for _, vector := range file.Vectors {
		apply := Text
		if vector.FileName {
			apply = FileName
		}
		if got := apply(vector.Input); got != vector.Output {
			t.Errorf("%s:\n got  %q\n want %q", vector.Name, got, vector.Output)
		}
		if got := apply(vector.Output); got != vector.Output {
			t.Errorf("%s redacts its own output again: %q", vector.Name, got)
		}
	}
}

func TestUnknownChecksAndFieldListsAreRejected(t *testing.T) {
	for _, data := range []string{
		`{"rules":[{"name":"x","pattern":"a","check":"later"}],"source_location":"b"}`,
		`{"rules":[{"name":"x","pattern":"{fields}","fields":"other"}],"source_location":"b"}`,
	} {
		if _, err := compile([]byte(data)); err == nil {
			t.Errorf("compile(%s) succeeded", data)
		}
	}
}

// Logged service errors keep their status but never a token or address, within the bound.
func TestErrorHidesSecretsAndBoundsLength(t *testing.T) {
	err := errors.New(`POST gatherings: 401 Unauthorized: {"token":"abc123","Authorization":"MCToken eyJhbGciOi.eyJzdWIi.c2ln"} XBL3.0 x=123;eyJraWQ via 203.0.113.7:19132`)
	got := Error(err)
	for _, secret := range []string{"abc123", "eyJhbGciOi", "x=123;", "203.0.113"} {
		if strings.Contains(got, secret) {
			t.Fatalf("redacted error %q still holds %q", got, secret)
		}
	}
	if !strings.Contains(got, "401 Unauthorized") {
		t.Fatalf("redacted error %q lost its status", got)
	}
	if Error(nil) != "" {
		t.Fatal("a nil error is not empty")
	}
	long := Error(errors.New(strings.Repeat("é", maxLoggedError)))
	if !strings.HasSuffix(long, "…") || len(long) > maxLoggedError+len("…") || !strings.HasPrefix(long, "é") || strings.ContainsRune(long, '\uFFFD') {
		t.Fatalf("bounded error %q", long)
	}
}

func TestSecretAndPrivateNeverLogTheirValue(t *testing.T) {
	var output bytes.Buffer
	logger := slog.New(slog.NewTextHandler(&output, nil))
	logger.Info("signed in", "token", Secret("tok-123"), "target", Private("Play.Example.com:19132"), "xuid", Private("2535400000000000"))
	line := output.String()
	for _, private := range []string{"tok-123", "example", "19132", "2535400000000000"} {
		if strings.Contains(line, private) {
			t.Fatalf("log line %q holds %q", line, private)
		}
	}
	if fmt.Sprint(Secret("tok-123")) != rules.placeholder {
		t.Fatal("a formatted Secret is not the placeholder")
	}
	address := Private("play.example.com:19132").String()
	if address != Private("PLAY.example.com:19132").String() || address == Private("other.example.com:19132").String() {
		t.Fatalf("Private is not a stable per-value hash: %q", address)
	}
	if !strings.Contains(line, "target="+address) {
		t.Fatalf("log line %q lacks the hashed target %q", line, address)
	}
	if shared := Text(line); shared != line {
		t.Fatalf("share-time rules changed hashed values:\n got  %q\n want %q", shared, line)
	}
}

func TestPackIdentityKeys(t *testing.T) {
	for key, want := range map[string]bool{"uuid": true, "pack_URL": true, "content_key": true, "pack.size": false, "urls": false} {
		if got := PackIdentityKey(key); got != want {
			t.Errorf("PackIdentityKey(%q) = %v, want %v", key, got, want)
		}
	}
}

// TestFilenamePorts keeps every shared filename exemption from exposing explicit endpoints.
func TestFilenamePorts(t *testing.T) {
	for _, extension := range rules.extensions {
		file := "fixture." + extension
		if got := Text(file); got != file {
			t.Fatalf("filename %s: %s", file, got)
		}
		for _, port := range []string{"1", "9", "80", "19132", "65535"} {
			if got := Text(file + ":" + port); got != rules.placeholder {
				t.Errorf("endpoint %s:%s: %s", file, port, got)
			}
		}
	}
}
