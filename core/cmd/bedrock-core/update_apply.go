package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/update"
)

// runApplyUpdate waits for the pipe and parent process before touching an installation.
func runApplyUpdate(ctx context.Context, args []string, stdin io.Reader) error {
	flags := flag.NewFlagSet("apply-update", flag.ContinueOnError)
	stage := flags.String("stage", "", "verified staging directory")
	target := flags.String("target", "", "installed app bundle or executable")
	platform := flags.String("platform", "", "signed artifact platform")
	channel := flags.String("channel", "stable", "release channel")
	parent := flags.Int("parent-pid", 0, "client PID that must exit before applying")
	restart := flags.Bool("restart", false, "restart the client after installation")
	if err := flags.Parse(args); err != nil {
		return err
	}
	if *parent <= 0 || *parent == os.Getpid() {
		return errors.New("valid parent-pid is required")
	}
	keys, err := update.TrustedKeys()
	if err != nil {
		return err
	}
	cfg := update.ApplyConfig{Config: update.Config{Keys: keys, Channel: *channel, Platform: *platform}, Stage: *stage, Target: *target, Restart: *restart}
	err = update.ApplyAfterExit(ctx, cfg, func(ctx context.Context) error {
		if err := waitForInputClose(ctx, stdin); err != nil {
			return err
		}
		return waitForParent(ctx, *parent)
	})
	if err != nil && *stage != "" {
		_ = os.WriteFile(filepath.Join(*stage, "apply-error.txt"), []byte(err.Error()), 0o600)
	}
	if err == nil {
		_ = os.RemoveAll(*stage)
	}
	return err
}

// waitForParent requires observed termination, failing closed on ambiguous process errors.
func waitForParent(ctx context.Context, pid int) error {
	for {
		alive, err := parentAlive(pid)
		if err != nil {
			return fmt.Errorf("wait for client exit: %w", err)
		}
		if !alive {
			return nil
		}
		select {
		case <-ctx.Done():
			return ctx.Err()
		case <-time.After(100 * time.Millisecond):
		}
	}
}

// waitForInputClose lets cancellation stop a helper even while its parent holds the pipe.
func waitForInputClose(ctx context.Context, input io.Reader) error {
	done := make(chan error, 1)
	go func() { _, err := io.Copy(io.Discard, input); done <- err }()
	select {
	case <-ctx.Done():
		return ctx.Err()
	case err := <-done:
		return err
	}
}
