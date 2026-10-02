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
	"strconv"
	"testing"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
	"golang.org/x/oauth2"
)

func TestOAuthRefreshAcrossProcesses(t *testing.T) {
	if path := os.Getenv("AUTHCACHE_ROTATION_PATH"); path != "" {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		defer cancel()
		source, err := Source(ctx, Config{Path: path, Refresh: func(cached *oauth2.Token, _ io.Writer) oauth2.TokenSource {
			n, err := strconv.Atoi(cached.RefreshToken)
			if err != nil {
				t.Fatal(err)
			}
			return oauthSourceFunc(func() (*oauth2.Token, error) {
				time.Sleep(30 * time.Millisecond)
				n++
				value := strconv.Itoa(n)
				return token(value, value), nil
			})
		}})
		if err != nil {
			t.Fatal(err)
		}
		if err := os.WriteFile(path+".ready."+os.Getenv("AUTHCACHE_WORKER"), nil, 0o600); err != nil {
			t.Fatal(err)
		}
		waitForCacheTestFile(t, path+".start")
		if _, err := source.Token(); err != nil {
			t.Fatal(err)
		}
		return
	}
	path := filepath.Join(t.TempDir(), "oauth.json")
	writeToken(t, path, token("0", "0"))
	var workers []*exec.Cmd
	var outputs []*bytes.Buffer
	for i := range 2 {
		ctx, cancel := context.WithTimeout(context.Background(), 10*time.Second)
		t.Cleanup(cancel)
		worker := exec.CommandContext(ctx, os.Args[0], "-test.run=^TestOAuthRefreshAcrossProcesses$")
		worker.Env = append(os.Environ(), "AUTHCACHE_ROTATION_PATH="+path, "AUTHCACHE_WORKER="+strconv.Itoa(i))
		output := new(bytes.Buffer)
		worker.Stdout, worker.Stderr = output, output
		if err := worker.Start(); err != nil {
			t.Fatal(err)
		}
		t.Cleanup(func() { _ = worker.Process.Kill(); _ = worker.Wait() })
		workers, outputs = append(workers, worker), append(outputs, output)
	}
	for i := range workers {
		waitForCacheTestFile(t, path+".ready."+strconv.Itoa(i))
	}
	if err := os.WriteFile(path+".start", nil, 0o600); err != nil {
		t.Fatal(err)
	}
	for i, worker := range workers {
		if err := worker.Wait(); err != nil {
			t.Fatalf("worker failed: %v\n%s", err, outputs[i])
		}
	}
	assertCachedToken(t, path, token("4", "4"))
}

// waitForCacheTestFile bounds synchronization with the child test processes.
func waitForCacheTestFile(t *testing.T, path string) {
	t.Helper()
	deadline := time.Now().Add(5 * time.Second)
	for time.Now().Before(deadline) {
		if _, err := os.Stat(path); err == nil {
			return
		} else if !errors.Is(err, os.ErrNotExist) {
			t.Fatal(err)
		}
		time.Sleep(10 * time.Millisecond)
	}
	t.Fatalf("timed out waiting for %s", filepath.Base(path))
}

func TestOAuthLeaseWaitIsCancelled(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oauth.json")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	source, err := Source(ctx, Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
		return token("first", "refresh"), nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	lease, err := lockfile.Acquire(path+".lock", 0)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	time.AfterFunc(50*time.Millisecond, cancel)
	started := time.Now()
	if _, err := source.Token(); !errors.Is(err, context.Canceled) {
		t.Fatalf("Token error = %v, want cancellation", err)
	}
	if time.Since(started) > time.Second {
		t.Fatal("OAuth lease wait ignored cancellation")
	}
}

func TestAccountCancellationInterruptsOAuthLeaseWait(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oauth.json")
	source, err := Source(context.Background(), Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
		return token("first", "refresh"), nil
	}})
	if err != nil {
		t.Fatal(err)
	}
	account := newAccount(context.Background(), "", source, nil, derivedDeps{})
	lease, err := lockfile.Acquire(path+".lock", 0)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	if _, err := account.DeviceToken(ctx); !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("DeviceToken error = %v, want deadline", err)
	}
}

func TestCachePublicationNeverExposesPartialToken(t *testing.T) {
	if runtime.GOOS == "windows" {
		t.Skip("Windows readers may deny rename; OAuth readers use the shared lease")
	}
	path := filepath.Join(t.TempDir(), "oauth.json")
	before, after := token("before", "before-refresh"), token("after", "after-refresh")
	if err := save(path, before, ""); err != nil {
		t.Fatal(err)
	}
	done := make(chan error, 1)
	go func() {
		for range 100 {
			got, err := load(path)
			if err != nil {
				done <- err
				return
			}
			if !sameToken(&got.Token, before) && !sameToken(&got.Token, after) {
				done <- errors.New("reader saw an incomplete token")
				return
			}
		}
		done <- nil
	}()
	for range 20 {
		if err := save(path, after, ""); err != nil {
			t.Fatal(err)
		}
		if err := save(path, before, ""); err != nil {
			t.Fatal(err)
		}
	}
	if err := <-done; err != nil {
		t.Fatal(err)
	}
	assertPrivateFile(t, path)
	temps, err := filepath.Glob(filepath.Join(filepath.Dir(path), ".auth-cache-*.tmp"))
	if err != nil || len(temps) != 0 {
		t.Fatalf("publication left temporary files: %v, %v", temps, err)
	}
}

func TestFailedCachePublicationRemovesTemporaryFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "directory")
	if err := os.Mkdir(path, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := savePrivate(path, []byte("synthetic credential")); err == nil {
		t.Fatal("replacing a directory succeeded")
	}
	entries, err := os.ReadDir(filepath.Dir(path))
	if err != nil || len(entries) != 1 || entries[0].Name() != "directory" {
		t.Fatalf("failed publication left temporary files: %v, %v", entries, err)
	}
}

func TestConcurrentSignInWaitsForFirstDeviceFlow(t *testing.T) {
	path := filepath.Join(t.TempDir(), "oauth.json")
	ctx, cancel := context.WithCancel(context.Background())
	defer cancel()
	started := make(chan struct{})
	first := make(chan error, 1)
	go func() {
		_, err := Source(ctx, Config{Path: path, Refresh: staticRefresh, Request: func(ctx context.Context, _ io.Writer) (*oauth2.Token, error) {
			close(started)
			<-ctx.Done()
			return nil, ctx.Err()
		}})
		first <- err
	}()
	<-started
	wait, stop := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer stop()
	_, err := Source(wait, Config{Path: path, Refresh: staticRefresh, Request: func(context.Context, io.Writer) (*oauth2.Token, error) {
		t.Error("second source started a competing device flow")
		return token("second", "second-refresh"), nil
	}})
	if !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("second sign-in error = %v, want deadline", err)
	}
	cancel()
	if err := <-first; !errors.Is(err, context.Canceled) {
		t.Fatalf("first sign-in error = %v, want cancellation", err)
	}
	if _, err := os.Stat(path); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("cancelled device flow published a token: %v", err)
	}
}
