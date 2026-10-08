package main

import (
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

func fixture(t *testing.T) string {
	t.Helper()
	root := t.TempDir()
	for name, body := range map[string]string{"index.html": "<title>Cinnabar</title>", "app.js": "console.log('ok');", ".private": "secret"} {
		if err := os.WriteFile(filepath.Join(root, name), []byte(body), 0600); err != nil {
			t.Fatal(err)
		}
	}
	if err := os.Mkdir(filepath.Join(root, "directory"), 0700); err != nil {
		t.Fatal(err)
	}
	return root
}

func TestStaticHTTP(t *testing.T) {
	server := httptest.NewServer(handler(fixture(t), "release-one"))
	defer server.Close()
	for _, test := range []struct {
		method, path string
		status       int
		body, mime   string
	}{
		{"GET", "/", 200, "<title>Cinnabar</title>", "text/html"},
		{"GET", "/app.js?release=one", 200, "console.log('ok');", "javascript"},
		{"HEAD", "/app.js", 200, "", "javascript"},
		{"GET", "/missing", 404, "404 page not found\n", "text/plain"},
		{"GET", "/directory/", 404, "404 page not found\n", "text/plain"},
		{"GET", "/.private", 404, "404 page not found\n", "text/plain"},
		{"GET", "/%2e%2e/.ssh/authorized_keys", 404, "404 page not found\n", "text/plain"},
		{"GET", "/server", 404, "404 page not found\n", "text/plain"},
		{"POST", "/", 405, "Method not allowed\n", "text/plain"},
		{"GET", "/healthz", 200, "release-one\n", "text/plain"},
	} {
		t.Run(test.method+test.path, func(t *testing.T) {
			request, err := http.NewRequest(test.method, server.URL+test.path, nil)
			if err != nil {
				t.Fatal(err)
			}
			response, err := server.Client().Do(request)
			if err != nil {
				t.Fatal(err)
			}
			defer response.Body.Close()
			body, err := io.ReadAll(response.Body)
			if err != nil || response.StatusCode != test.status || string(body) != test.body {
				t.Fatalf("status=%d body=%q error=%v", response.StatusCode, body, err)
			}
			if !strings.Contains(response.Header.Get("Content-Type"), test.mime) || response.Header.Get("Cache-Control") != "no-cache" || response.Header.Get("X-Content-Type-Options") != "nosniff" {
				t.Fatalf("unexpected headers: %v", response.Header)
			}
			if test.method == "HEAD" && response.ContentLength <= 0 {
				t.Fatal("HEAD lost the static file content length")
			}
		})
	}
}

func TestSymlinkCannotExposeFilesOutsidePublicRoot(t *testing.T) {
	root := fixture(t)
	secret := filepath.Join(t.TempDir(), "secret")
	if err := os.WriteFile(secret, []byte("private"), 0600); err != nil {
		t.Fatal(err)
	}
	if err := os.Symlink(secret, filepath.Join(root, "leak")); err != nil {
		t.Fatal(err)
	}
	response := httptest.NewRecorder()
	handler(root, "release").ServeHTTP(response, httptest.NewRequest("GET", "/leak", nil))
	if response.Code != http.StatusNotFound {
		t.Fatalf("private symlink returned %d", response.Code)
	}
}

func TestRootFollowsAtomicReleasePointer(t *testing.T) {
	parent := t.TempDir()
	first, second := fixture(t), fixture(t)
	if err := os.WriteFile(filepath.Join(second, "index.html"), []byte("second"), 0600); err != nil {
		t.Fatal(err)
	}
	current := filepath.Join(parent, "current")
	if err := os.Symlink(first, current); err != nil {
		t.Fatal(err)
	}
	serve := handler(current, "release")
	response := httptest.NewRecorder()
	serve.ServeHTTP(response, httptest.NewRequest("GET", "/", nil))
	if response.Body.String() != "<title>Cinnabar</title>" {
		t.Fatal(response.Body.String())
	}
	next := filepath.Join(parent, "next")
	if err := os.Symlink(second, next); err != nil {
		t.Fatal(err)
	}
	if err := os.Rename(next, current); err != nil {
		t.Fatal(err)
	}
	response = httptest.NewRecorder()
	serve.ServeHTTP(response, httptest.NewRequest("GET", "/", nil))
	if response.Body.String() != "second" {
		t.Fatal(response.Body.String())
	}
}
