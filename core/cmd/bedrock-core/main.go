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
	"github.com/hashimthearab/rust-mcbe/core/authflow"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/packcache"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/sandertv/gophertunnel/minecraft"
	"golang.org/x/oauth2"
)

func main() {
	signalCtx, stopSignals := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	ctx := signalCtx
	stopStdin := func() {}
	if !catalogMode(os.Args[1:]) {
		ctx, stopStdin = contextWithStdinEOF(signalCtx, os.Stdin)
	}
	exitCode := execute(ctx, os.Args[1:], os.Stdout, os.Stderr, authcache.Source, proxy.Serve)
	stopStdin()
	stopSignals()
	if exitCode != 0 {
		os.Exit(exitCode)
	}
}

func catalogMode(args []string) bool {
	for _, arg := range args {
		if arg == "-auth-events" ||
			arg == "-catalog-file" || len(arg) > len("-catalog-file=") && arg[:len("-catalog-file=")] == "-catalog-file=" {
			return true
		}
	}
	return false
}

type options struct {
	socketDir                 string
	upstream                  string
	authCache                 string
	catalogFile               string
	authEvents                bool
	resourcePackCacheDir      string
	resourcePackCacheQuota    uint64
	resourcePackCacheQuotaSet bool
	controlStatus             bool
	upstreamClientCache       bool
}

func parseFlags(args []string, stderr io.Writer) (options, error) {
	flags := flag.NewFlagSet("bedrock-core", flag.ContinueOnError)
	flags.SetOutput(stderr)
	var opts options
	flags.StringVar(&opts.socketDir, "socket-dir", "", "directory containing the local bridge endpoint")
	flags.StringVar(&opts.upstream, "upstream", "", "upstream Bedrock server address (host:port)")
	flags.StringVar(&opts.authCache, "auth-cache", "", "path to the Microsoft authentication token cache")
	flags.StringVar(&opts.catalogFile, "catalog-file", "", "write the authenticated launcher catalog and exit")
	flags.BoolVar(&opts.authEvents, "auth-events", false, "perform one-shot authentication and emit bounded JSONL events")
	flags.StringVar(&opts.resourcePackCacheDir, "resource-pack-cache-dir", "", "enable the persistent verified resource-pack cache in this directory")
	flags.Uint64Var(&opts.resourcePackCacheQuota, "resource-pack-cache-quota-bytes", packcache.DefaultQuota, "maximum resource-pack cache bytes (requires -resource-pack-cache-dir)")
	flags.BoolVar(&opts.controlStatus, "control-status", false, "enable the local read-only Status v1 control endpoint")
	flags.BoolVar(&opts.upstreamClientCache, "upstream-client-cache", false, "advertise client-cache capability upstream; enable only when the connecting client owns a verified blob cache")
	if err := flags.Parse(args); err != nil {
		return options{}, err
	}
	flags.Visit(func(value *flag.Flag) {
		if value.Name == "resource-pack-cache-quota-bytes" {
			opts.resourcePackCacheQuotaSet = true
		}
	})
	if opts.resourcePackCacheQuotaSet && opts.resourcePackCacheDir == "" {
		return options{}, errors.New("resource-pack-cache-quota-bytes requires -resource-pack-cache-dir")
	}
	if opts.resourcePackCacheDir != "" && opts.resourcePackCacheQuota == 0 {
		return options{}, errors.New("resource-pack-cache-quota-bytes must be greater than zero")
	}
	if opts.authEvents && flags.NArg() != 0 {
		return options{}, errors.New("auth-events mode does not accept positional arguments")
	}
	return opts, nil
}

type sourceFunc func(context.Context, authcache.Config) (oauth2.TokenSource, error)
type serveFunc func(context.Context, proxy.Config) error
type ownedResourcePackCache interface {
	minecraft.ResourcePackCache
	Close() error
}
type resourcePackCacheFactory func(string, ...packcache.Option) (ownedResourcePackCache, error)

func execute(ctx context.Context, args []string, stdout, stderr io.Writer, source sourceFunc, serve serveFunc) int {
	if err := run(ctx, args, stdout, stderr, source, serve); err != nil {
		newLifecycleLogger(stderr).Error("core failed", "error", err)
		return 1
	}
	return 0
}

func run(ctx context.Context, args []string, stdout, stderr io.Writer, source sourceFunc, serve serveFunc) error {
	return runWithResourcePackCacheFactory(ctx, args, stdout, stderr, source, serve, func(root string, options ...packcache.Option) (ownedResourcePackCache, error) {
		return packcache.New(root, options...)
	})
}

func runWithResourcePackCacheFactory(
	ctx context.Context,
	args []string,
	stdout, stderr io.Writer,
	source sourceFunc,
	serve serveFunc,
	openCache resourcePackCacheFactory,
) error {
	opts, err := parseFlags(args, stderr)
	if err != nil {
		if errors.Is(err, flag.ErrHelp) {
			return nil
		}
		return err
	}
	logger := newLifecycleLogger(stderr)
	if opts.authEvents {
		if opts.authCache == "" {
			return errors.New("auth-events mode requires -auth-cache")
		}
		if opts.socketDir != "" || opts.upstream != "" || opts.catalogFile != "" || opts.resourcePackCacheDir != "" || opts.controlStatus {
			return errors.New("auth-events mode cannot be combined with proxy or catalog options")
		}
		return authflow.Run(ctx, authflow.Config{Path: opts.authCache, Writer: stdout})
	}
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
	if opts.catalogFile != "" {
		if opts.resourcePackCacheDir != "" {
			return errors.New("catalog mode cannot be combined with resource-pack cache options")
		}
		if tokenSource == nil {
			return errors.New("catalog mode requires -auth-cache")
		}
		if err := catalog.Write(ctx, opts.catalogFile, tokenSource); err != nil {
			return fmt.Errorf("write launcher catalog: %w", err)
		}
		logger.Info("launcher catalog written", "path", opts.catalogFile)
		return nil
	}
	var resourcePackCache minecraft.ResourcePackCache
	var closeResourcePackCache func() error
	if opts.resourcePackCacheDir != "" {
		cache, cacheErr := openCache(opts.resourcePackCacheDir, packcache.WithQuota(opts.resourcePackCacheQuota))
		if cacheErr != nil {
			return errors.New("initialize resource pack cache: unavailable")
		}
		resourcePackCache = cache
		closeResourcePackCache = cache.Close
	}
	var statusStore *control.Store
	var controlServer *control.Server
	var resourcePackAdmissionUpdate func(proxy.ResourcePackAdmissionSnapshot)
	if opts.controlStatus {
		statusStore = control.NewStore()
		controlServer, err = control.Start(opts.socketDir, statusStore)
		if err != nil {
			if closeResourcePackCache != nil {
				_ = closeResourcePackCache()
			}
			return fmt.Errorf("start control endpoint: %w", err)
		}
		statusStore.SetLifecycle(control.LifecycleRunning)
		resourcePackAdmissionUpdate = statusStore.Observe
	}
	serveErr := serve(ctx, proxy.Config{
		SocketDir:           opts.socketDir,
		Upstream:            opts.upstream,
		TokenSource:         tokenSource,
		Logger:              logger,
		UpstreamClientCache: opts.upstreamClientCache,
		ResourcePackCache:   resourcePackCache,
		ResourcePackAdmission: func(snapshot proxy.ResourcePackAdmissionSnapshot) {
			logger.Info("RESOURCE_PACK_ADMISSION",
				"attempt_id", snapshot.AttemptID,
				"offer", snapshot.Offer,
				"pack_count", snapshot.PackCount,
				"total_bytes", snapshot.TotalBytes,
				"acquisition", snapshot.Acquisition,
				"cache_loads", snapshot.CacheLoads,
				"cache_hits", snapshot.CacheHits,
				"cache_misses", snapshot.CacheMisses,
				"cache_stores", snapshot.CacheStores,
				"cache_errors", snapshot.CacheErrors,
				"downstream_outcome", snapshot.DownstreamOutcome,
				"application", snapshot.Application,
			)
		},
		ResourcePackAdmissionUpdate: resourcePackAdmissionUpdate,
	})
	if controlServer != nil {
		serveErr = errors.Join(serveErr, controlServer.Close())
	}
	if closeResourcePackCache == nil {
		return serveErr
	}
	if closeErr := closeResourcePackCache(); closeErr != nil {
		return errors.Join(serveErr, errors.New("close resource pack cache: unavailable"))
	}
	return serveErr
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
