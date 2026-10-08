package lockfile

import (
	"bufio"
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"path/filepath"
	"testing"
	"time"
)

func TestAcquireContextCancellationDoesNotCreateLock(t *testing.T) {
	path := filepath.Join(t.TempDir(), "missing", "lease")
	ctx, cancel := context.WithCancel(context.Background())
	cancel()
	if lease, err := AcquireContext(ctx, path); lease != nil || !errors.Is(err, context.Canceled) {
		t.Fatalf("cancelled lease = %v, %v", lease, err)
	}
	if _, err := os.Stat(filepath.Dir(path)); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("cancelled acquisition created a directory: %v", err)
	}
}

func TestAcquireContextCancellationInterruptsContention(t *testing.T) {
	path := filepath.Join(t.TempDir(), "lease")
	lease, err := Acquire(path, 0)
	if err != nil {
		t.Fatal(err)
	}
	defer lease.Close()
	ctx, cancel := context.WithTimeout(context.Background(), 50*time.Millisecond)
	defer cancel()
	started := time.Now()
	if next, err := AcquireContext(ctx, path); next != nil || !errors.Is(err, context.DeadlineExceeded) {
		t.Fatalf("contended lease = %v, %v", next, err)
	}
	if time.Since(started) > time.Second {
		t.Fatal("cancellation did not interrupt the lease wait")
	}
}

func TestAcquireStillCreatesMissingParentAndFile(t *testing.T) {
	path := filepath.Join(t.TempDir(), "missing", "lease")
	lease, err := Acquire(path, 0)
	if err != nil {
		t.Fatal(err)
	}
	if err := lease.Close(); err != nil {
		t.Fatal(err)
	}
	if info, err := os.Lstat(path); err != nil || !info.Mode().IsRegular() {
		t.Fatalf("Acquire did not create a regular lease file: info=%v error=%v", info, err)
	}
}

// The auth-cache lease is an existing lock file; a holder that dies without releasing it (the
// kernel drops the lock with its descriptors) must not block the next core.
func TestLeaseOfADeadHolderIsFree(t *testing.T) {
	if path := os.Getenv("LOCKFILE_HOLDER"); path != "" {
		if _, err := Acquire(path, 0); err != nil {
			os.Exit(2)
		}
		fmt.Println("held")
		time.Sleep(time.Minute)
		os.Exit(3)
	}
	path := filepath.Join(t.TempDir(), "lease")
	if err := os.WriteFile(path, nil, 0o600); err != nil {
		t.Fatal(err)
	}
	holder := exec.Command(os.Args[0], "-test.run=^TestLeaseOfADeadHolderIsFree$")
	holder.Env = append(os.Environ(), "LOCKFILE_HOLDER="+path)
	stdout, err := holder.StdoutPipe()
	if err != nil {
		t.Fatal(err)
	}
	if err := holder.Start(); err != nil {
		t.Fatal(err)
	}
	defer func() { _ = holder.Process.Kill(); _ = holder.Wait() }()
	if line, _ := bufio.NewReader(stdout).ReadString('\n'); line != "held\n" {
		t.Fatalf("holder reported %q", line)
	}
	if _, err := Acquire(path, 0); !errors.Is(err, ErrBusy) {
		t.Fatalf("Acquire beside a live holder = %v, want ErrBusy", err)
	}
	_ = holder.Process.Kill()
	_ = holder.Wait()
	lease, err := Acquire(path, 0)
	if err != nil {
		t.Fatalf("Acquire after the holder died = %v", err)
	}
	_ = lease.Close()
}
