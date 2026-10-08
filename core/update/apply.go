package update

import (
	"context"
	"crypto/sha256"
	"errors"
	"fmt"
	"io"
	"os"
	"os/exec"
	"path/filepath"
	"strings"

	"github.com/hashimthearab/rust-mcbe/core/internal/lockfile"
)

// ApplyConfig identifies a staged release and the installed file or app bundle.
type ApplyConfig struct {
	Config  Config
	Stage   string
	Target  string
	Restart bool
}

// ApplyAfterExit prevents verification, extraction and replacement until the caller
// confirms the client process has ended. The helper provides the real process wait.
func ApplyAfterExit(ctx context.Context, cfg ApplyConfig, wait func(context.Context) error) error {
	if wait == nil {
		return errors.New("update requires a parent-exit barrier")
	}
	if err := wait(ctx); err != nil {
		return err
	}
	return apply(cfg)
}

// apply revalidates the staged envelope and artifact before preparing an installation.
func apply(cfg ApplyConfig) error {
	body, err := os.ReadFile(filepath.Join(cfg.Stage, "manifest.json"))
	if err != nil {
		return err
	}
	if len(body) > maxEnvelopeBytes {
		return errors.New("staged manifest exceeds size limit")
	}
	_, artifact, err := selected(body, cfg.Config)
	if err != nil {
		return err
	}
	source := filepath.Join(cfg.Stage, "artifact")
	if err := verifyFile(source, artifact); err != nil {
		return err
	}
	target, err := filepath.Abs(cfg.Target)
	if err != nil || cfg.Target == "" {
		return errors.New("installed target is required")
	}
	if _, err := os.Lstat(target); err != nil {
		return fmt.Errorf("installed target: %w", err)
	}
	platform := strings.Split(cfg.Config.Platform, "-")[0]
	lockTarget := target
	if platform == "windows" {
		lockTarget = strings.ToLower(lockTarget)
	}
	lockPath := filepath.Join(filepath.Dir(cfg.Stage), fmt.Sprintf("apply-%x.lock", sha256.Sum256([]byte(lockTarget))))
	lease, err := lockfile.Acquire(lockPath, 0)
	if err != nil {
		return fmt.Errorf("another update may be applying: %w", err)
	}
	defer lease.Close()
	if platform == "windows" {
		return installMSI(source, artifact, target, cfg.Restart)
	}
	if platform != "linux" && platform != "macos" {
		return errors.New("unsupported update platform")
	}
	work, err := os.MkdirTemp(filepath.Dir(target), ".cinnabar-update-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(work)
	localArtifact := filepath.Join(work, "artifact")
	if err := copyFile(source, localArtifact, 0o700); err != nil {
		return err
	}
	if err := verifyFile(localArtifact, artifact); err != nil {
		return err
	}
	candidate := localArtifact
	if platform == "macos" {
		if filepath.Ext(target) != ".app" {
			return errors.New("macOS target must be an app bundle")
		}
		candidate, err = extractApp(localArtifact, work)
		if err != nil {
			return err
		}
	}
	if err := replaceWithRollback(target, candidate, os.Rename); err != nil {
		return err
	}
	if cfg.Restart {
		var command *exec.Cmd
		if platform == "macos" {
			command = exec.Command("/usr/bin/open", target)
		} else {
			command = exec.Command(target, "--appimage-extract-and-run")
			command.Env = cleanAppImageEnvironment(os.Environ())
		}
		if err := command.Start(); err != nil {
			return fmt.Errorf("updated successfully but restart failed: %w", err)
		}
		_ = command.Process.Release()
	}
	return nil
}

// replaceWithRollback retains the previous installation and restores it if promotion fails.
func replaceWithRollback(target, candidate string, rename func(string, string) error) error {
	backup := target + ".previous"
	if err := os.RemoveAll(backup); err != nil {
		return fmt.Errorf("remove older backup: %w", err)
	}
	if err := rename(target, backup); err != nil {
		return fmt.Errorf("save previous installation: %w", err)
	}
	if err := rename(candidate, target); err != nil {
		restoreErr := rename(backup, target)
		return errors.Join(fmt.Errorf("install update: %w", err), restoreErr)
	}
	return nil
}

// copyFile creates and flushes a private complete copy on the destination filesystem.
func copyFile(source, target string, mode os.FileMode) error {
	in, err := os.Open(source)
	if err != nil {
		return err
	}
	defer in.Close()
	out, err := os.OpenFile(target, os.O_CREATE|os.O_EXCL|os.O_WRONLY, mode)
	if err != nil {
		return err
	}
	_, copyErr := io.Copy(out, in)
	syncErr := out.Sync()
	return errors.Join(copyErr, syncErr, out.Close())
}

// installMSI uses Windows Installer's transaction and rollback support after client exit.
func installMSI(source string, artifact Artifact, target string, restart bool) error {
	work, err := os.MkdirTemp(filepath.Dir(source), "installer-")
	if err != nil {
		return err
	}
	defer os.RemoveAll(work)
	installer := filepath.Join(work, "update.msi")
	if err := copyFile(source, installer, 0o600); err != nil {
		return err
	}
	if err := verifyFile(installer, artifact); err != nil {
		return err
	}
	err = exec.Command("msiexec.exe", "/i", installer, "/passive", "/norestart").Run()
	if err != nil {
		var exit *exec.ExitError
		if !errors.As(err, &exit) || (exit.ExitCode() != 3010 && exit.ExitCode() != 1641) {
			return fmt.Errorf("Windows Installer failed (transaction rolled back): %w", err)
		}
	}
	if restart {
		command := exec.Command(target)
		if err := command.Start(); err != nil {
			return err
		}
		_ = command.Process.Release()
	}
	return nil
}

// cleanAppImageEnvironment prevents a restarted image from inheriting its deleted mount.
func cleanAppImageEnvironment(environment []string) []string {
	clean := make([]string, 0, len(environment))
	for _, entry := range environment {
		key, _, _ := strings.Cut(entry, "=")
		switch key {
		case "APPIMAGE", "APPDIR", "OWD", "LD_LIBRARY_PATH", "LD_PRELOAD":
			continue
		}
		clean = append(clean, entry)
	}
	return clean
}
