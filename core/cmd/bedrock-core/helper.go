package main

import (
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"os"
	"runtime"

	"github.com/hashimthearab/rust-mcbe/core/update"
)

// releaseVersion is injected by release builds. Public keys live in core/update/trusted-keys.txt.
var releaseVersion = "0.0.0-dev"

// helperMode reports whether args select a client-driven helper subcommand and, if so, runs it.
func helperMode(ctx context.Context, args []string, stdout, stderr io.Writer) (bool, int) {
	if len(args) == 0 {
		return false, 0
	}
	var err error
	switch args[0] {
	case "check-update", "download-update":
		err = runUpdate(ctx, args[1:], stdout, args[0] == "download-update")
	case "apply-update":
		err = runApplyUpdate(ctx, args[1:], os.Stdin)
	default:
		return false, 0
	}
	if err != nil {
		fmt.Fprintf(stderr, "%s: %v\n", args[0], err)
		return true, 1
	}
	return true, 0
}

// runUpdate shares channel and trust selection between checks and background downloads.
func runUpdate(ctx context.Context, args []string, stdout io.Writer, download bool) error {
	flags := flag.NewFlagSet("check-update", flag.ContinueOnError)
	manifestURL := flags.String("manifest-url", "", "signed manifest URL")
	channel := flags.String("channel", "stable", "release channel")
	platform := flags.String("platform", runtime.GOOS+"-"+runtime.GOARCH, "artifact platform key")
	current := flags.String("current", releaseVersion, "running client version")
	cache := flags.String("cache-dir", "", "private updater cache directory")
	if err := flags.Parse(args); err != nil {
		return err
	}
	keys, err := update.TrustedKeys()
	if err != nil {
		return err
	}
	if len(keys) == 0 {
		return errors.New("this build has no trusted update keys")
	}
	cfg := update.Config{
		ManifestURL: *manifestURL, Channel: *channel, Platform: *platform, Current: *current, Keys: keys,
	}
	encoder := json.NewEncoder(stdout)
	if download {
		if *cache == "" {
			return errors.New("cache-dir is required")
		}
		result, err := update.Download(ctx, cfg, *cache, func(p update.Progress) { _ = encoder.Encode(p) })
		if err != nil {
			return err
		}
		return encoder.Encode(result)
	}
	result, err := update.Check(ctx, cfg)
	if err != nil {
		return err
	}
	return json.NewEncoder(stdout).Encode(result)
}
