//go:build linux

package packcache

import (
	"context"
	"errors"
	"os"
	"os/exec"
	"path/filepath"
	"runtime"
	"testing"
	"unsafe"

	"github.com/google/uuid"
	"golang.org/x/sys/unix"
)

// TestPublicationWithoutHardLinks models Android's app-level denial in a disposable process.
func TestPublicationWithoutHardLinks(t *testing.T) {
	const child = "CINNABAR_PACKCACHE_NO_LINKS"
	if os.Getenv(child) != "1" {
		command := exec.Command(os.Args[0], "-test.run=^TestPublicationWithoutHardLinks$", "-test.v")
		command.Env = append(os.Environ(), child+"=1")
		if output, err := command.CombinedOutput(); err != nil {
			t.Fatalf("publication with hard links denied: %v\n%s", err, output)
		}
		return
	}
	// Keep the filtered thread locked until this child test exits so the runtime cannot reuse it.
	runtime.LockOSThread()
	denySyscall(t, unix.SYS_LINKAT, unix.EACCES)
	root := t.TempDir()
	source, target := filepath.Join(root, "staging"), filepath.Join(root, "pack.mcpack")
	if err := os.WriteFile(source, []byte("complete archive"), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Link(source, target); !errors.Is(err, unix.EACCES) {
		t.Fatalf("hard-link fixture = %v, want permission denied", err)
	}
	if err := publishNoReplace(source, target); err != nil {
		t.Fatal(err)
	}
	if bytes, err := os.ReadFile(target); err != nil || string(bytes) != "complete archive" {
		t.Fatalf("published bytes = %q, %v", bytes, err)
	}
	if _, err := os.Stat(source); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("staging entry remains: %v", err)
	}

	cache := newTestCache(t, 1<<20)
	pack, key, _ := testPack(t, uuid.New(), "1.0.0", "persistent without links")
	if err := cache.Store(context.Background(), key, pack); err != nil {
		t.Fatal(err)
	}
	if err := cache.Close(); err != nil {
		t.Fatal(err)
	}
	reopened, err := New(cache.root)
	if err != nil {
		t.Fatal(err)
	}
	defer reopened.Close()
	if got, err := reopened.Load(context.Background(), key); err != nil || got == nil || got.Checksum() != pack.Checksum() {
		t.Fatalf("reopened cache Load = %v, %v; want original archive", got, err)
	}

	// An unavailable rename must leave the archive staged rather than use an overwriting fallback.
	denySyscall(t, unix.SYS_RENAMEAT2, unix.ENOSYS)
	if err := os.WriteFile(source, []byte("staged archive"), 0o600); err != nil {
		t.Fatal(err)
	}
	missing := filepath.Join(root, "unsupported.mcpack")
	if err := publishNoReplace(source, missing); !errors.Is(err, unix.ENOSYS) {
		t.Fatalf("unsupported publication = %v, want ENOSYS", err)
	}
	if data, err := os.ReadFile(source); err != nil || string(data) != "staged archive" {
		t.Fatalf("staged archive = %q, %v", data, err)
	}
	if _, err := os.Lstat(missing); !errors.Is(err, os.ErrNotExist) {
		t.Fatalf("unsupported publication created destination: %v", err)
	}
}

// denySyscall returns errno for one native syscall on the caller's locked child-process thread.
func denySyscall(t *testing.T, number uint32, errno unix.Errno) {
	t.Helper()
	filter := []unix.SockFilter{
		{Code: unix.BPF_LD | unix.BPF_W | unix.BPF_ABS},
		{Code: unix.BPF_JMP | unix.BPF_JEQ | unix.BPF_K, K: number, Jf: 1},
		{Code: unix.BPF_RET | unix.BPF_K, K: unix.SECCOMP_RET_ERRNO | uint32(errno)},
		{Code: unix.BPF_RET | unix.BPF_K, K: unix.SECCOMP_RET_ALLOW},
	}
	program := unix.SockFprog{Len: uint16(len(filter)), Filter: &filter[0]}
	if err := unix.Prctl(unix.PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0); err != nil {
		t.Fatalf("enable syscall fixture: %v", err)
	}
	if _, _, err := unix.Syscall6(unix.SYS_PRCTL, unix.PR_SET_SECCOMP, unix.SECCOMP_MODE_FILTER, uintptr(unsafe.Pointer(&program)), 0, 0, 0); err != 0 {
		t.Fatalf("deny syscall %d: %v", number, err)
	}
	runtime.KeepAlive(&program)
	runtime.KeepAlive(filter)
}

// TestPublicationPreservesExistingEntries checks collisions without following destination links.
func TestPublicationPreservesExistingEntries(t *testing.T) {
	for _, kind := range []string{"file", "directory", "symlink", "dangling symlink"} {
		t.Run(kind, func(t *testing.T) {
			root := t.TempDir()
			source, dest := filepath.Join(root, "staging"), filepath.Join(root, "destination")
			referent := filepath.Join(root, "referent")
			if err := os.WriteFile(source, []byte("new archive"), 0o600); err != nil {
				t.Fatal(err)
			}
			switch kind {
			case "file":
				if err := os.WriteFile(dest, []byte("existing archive"), 0o600); err != nil {
					t.Fatal(err)
				}
			case "directory":
				if err := os.Mkdir(dest, 0o700); err != nil {
					t.Fatal(err)
				}
			case "symlink", "dangling symlink":
				if kind == "symlink" {
					if err := os.WriteFile(referent, []byte("existing archive"), 0o600); err != nil {
						t.Fatal(err)
					}
				}
				if err := os.Symlink(referent, dest); err != nil {
					t.Fatal(err)
				}
			}
			before, err := os.Lstat(dest)
			if err != nil {
				t.Fatal(err)
			}
			if err := publishNoReplace(source, dest); !errors.Is(err, os.ErrExist) {
				t.Fatalf("publication = %v, want existing entry", err)
			}
			after, err := os.Lstat(dest)
			if err != nil || !os.SameFile(before, after) {
				t.Fatalf("destination changed: %v", err)
			}
			if data, err := os.ReadFile(source); err != nil || string(data) != "new archive" {
				t.Fatalf("staged archive = %q, %v", data, err)
			}
			switch kind {
			case "file", "symlink":
				if data, err := os.ReadFile(dest); err != nil || string(data) != "existing archive" {
					t.Fatalf("existing archive = %q, %v", data, err)
				}
			case "dangling symlink":
				if _, err := os.Lstat(referent); !errors.Is(err, os.ErrNotExist) {
					t.Fatalf("publication created symlink target: %v", err)
				}
			}
			if before.Mode()&os.ModeSymlink != 0 {
				if target, err := os.Readlink(dest); err != nil || target != referent {
					t.Fatalf("destination link = %q, %v", target, err)
				}
			}
		})
	}
}
