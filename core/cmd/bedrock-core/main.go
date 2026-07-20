package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/signal"
	"syscall"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"golang.org/x/oauth2"
)

func main() {
	signalCtx, stopSignals := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	ctx, stopStdin := contextWithStdinEOF(signalCtx, os.Stdin)
	exitCode := execute(ctx, os.Args[1:], os.Stdout, os.Stderr, authcache.Source, proxy.Serve)
	stopStdin()
	stopSignals()
	if exitCode != 0 {
		os.Exit(exitCode)
	}
}

type options struct {
	socketDir string
	upstream  string
	authCache string
}

func parseFlags(args []string, stderr io.Writer) (options, error) {
	flags := flag.NewFlagSet("bedrock-core", flag.ContinueOnError)
	flags.SetOutput(stderr)
	var opts options
	flags.StringVar(&opts.socketDir, "socket-dir", "", "directory containing the local bridge endpoint")
	flags.StringVar(&opts.upstream, "upstream", "", "upstream Bedrock server address (host:port)")
	flags.StringVar(&opts.authCache, "auth-cache", "", "path to the Microsoft authentication token cache")
	if err := flags.Parse(args); err != nil {
		return options{}, err
	}
	return opts, nil
}

type sourceFunc func(context.Context, authcache.Config) (oauth2.TokenSource, error)
type serveFunc func(context.Context, proxy.Config) error

func execute(ctx context.Context, args []string, stdout, stderr io.Writer, source sourceFunc, serve serveFunc) int {
	if err := run(ctx, args, stdout, stderr, source, serve); err != nil {
		newLifecycleLogger(stderr).Error("core failed", "error", err)
		return 1
	}
	return 0
}

func run(ctx context.Context, args []string, stdout, stderr io.Writer, source sourceFunc, serve serveFunc) error {
	opts, err := parseFlags(args, stderr)
	if err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	logger := newLifecycleLogger(stderr)
	logger.Info("core starting", "endpoint", opts.socketDir, "upstream", opts.upstream)
	authentication := "offline"
	var tokenSource oauth2.TokenSource
	if opts.authCache != "" {
		authentication = "microsoft"
		logger.Info("authentication starting", "mode", authentication)
		tokenSource, err = source(ctx, authcache.Config{Path: opts.authCache, Writer: stdout})
		if err != nil {
			return fmt.Errorf("initialize Microsoft authentication: %w", err)
		}
	}
	logger.Info("authentication ready", "mode", authentication)
	return serve(ctx, proxy.Config{
		SocketDir:   opts.socketDir,
		Upstream:    opts.upstream,
		TokenSource: tokenSource,
		Logger:      logger,
	})
}

func newLifecycleLogger(writer io.Writer) *slog.Logger {
	return slog.New(slog.NewTextHandler(writer, nil))
}

func contextWithStdinEOF(parent context.Context, stdin io.Reader) (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithCancel(parent)
	go func() {
		_, _ = io.Copy(io.Discard, stdin)
		cancel()
	}()
	return ctx, cancel
}
