package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"io"
	"log/slog"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/authflow"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/lifeline"
	"github.com/hashimthearab/rust-mcbe/core/internal/locale"
	"github.com/hashimthearab/rust-mcbe/core/launcher"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/packcache"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/sandertv/gophertunnel/minecraft"
	"golang.org/x/oauth2"
)

const (
	// Past this, a shutdown still waiting on work that ignores its context hard-exits.
	shutdownGrace      = 2 * time.Second
	parentPollInterval = 250 * time.Millisecond
)

func main() {
	args := os.Args[1:]
	var stdin io.Reader
	if bindsStdin(args) {
		stdin = os.Stdin
	}
	ctx, stop := lifeline.Start(context.Background(), lifeline.Config{
		Stdin:      stdin,
		ParentGone: lifeline.WatchParent(lifeline.ParentFromEnv(), parentPollInterval),
		Grace:      shutdownGrace,
	})
	if handled, code := helperMode(ctx, args, os.Stdout, os.Stderr); handled {
		stop()
		os.Exit(code)
	}
	exitCode := execute(ctx, args, os.Stdout, os.Stderr, authcache.Source, proxy.Serve)
	stop()
	if exitCode != 0 {
		os.Exit(exitCode)
	}
}

// bindsStdin reports whether the client pipes stdin, whose EOF then ends the core; the sign-in and
// update helpers run with a null stdin.
func bindsStdin(args []string) bool {
	if len(args) > 0 && args[0] == "check-update" {
		return false
	}
	for _, arg := range args {
		if arg == "-auth-events" || strings.HasPrefix(arg, "-auth-events=") {
			return false
		}
	}
	return true
}

type options struct {
	socketDir                 string
	upstream                  string
	authCache                 string
	language                  string
	catalogFile               string
	authEvents                bool
	resourcePackCacheDir      string
	resourcePackCacheQuota    uint64
	resourcePackCacheQuotaSet bool
	controlStatus             bool
	upstreamClientCache       bool
	localWorldsDir            string
	localServerBin            string
	localBackend              string
	bdsDir                    string
	bdsVersion                string
	bdsImage                  string
	docker                    string
}

func parseFlags(args []string, stderr io.Writer) (options, error) {
	flags := flag.NewFlagSet("bedrock-core", flag.ContinueOnError)
	flags.SetOutput(stderr)
	var opts options
	flags.StringVar(&opts.socketDir, "socket-dir", "", "directory containing the local bridge endpoint")
	flags.StringVar(&opts.upstream, "upstream", "", "upstream Bedrock server address (host:port)")
	flags.StringVar(&opts.authCache, "auth-cache", "", "path to the Microsoft authentication token cache")
	flags.StringVar(&opts.language, "language", locale.Default, "active UI language (BCP 47)")
	flags.StringVar(&opts.catalogFile, "catalog-file", "", "write the authenticated launcher catalog and exit")
	flags.BoolVar(&opts.authEvents, "auth-events", false, "perform one-shot authentication and emit bounded JSONL events")
	flags.StringVar(&opts.resourcePackCacheDir, "resource-pack-cache-dir", "", "enable the persistent verified resource-pack cache in this directory")
	flags.Uint64Var(&opts.resourcePackCacheQuota, "resource-pack-cache-quota-bytes", packcache.DefaultQuota, "maximum resource-pack cache bytes (requires -resource-pack-cache-dir)")
	flags.BoolVar(&opts.controlStatus, "control-status", false, "enable the local read-only Status v1 control endpoint")
	flags.BoolVar(&opts.upstreamClientCache, "upstream-client-cache", false, "advertise client-cache capability upstream; enable only when the connecting client owns a verified blob cache")
	flags.StringVar(&opts.localWorldsDir, "local-worlds-dir", "", "enable local single-player worlds stored in this directory (requires -control-status)")
	flags.StringVar(&opts.localServerBin, "local-server-bin", "", "local world server binary (default: bedrock-local-server beside the core)")
	flags.StringVar(&opts.localBackend, "local-backend", "auto", "default backend for new local worlds: auto (BDS where available, else dragonfly), bds or dragonfly")
	flags.StringVar(&opts.bdsDir, "bds-dir", "", "directory for downloaded Bedrock Dedicated Server builds (default: bds beside the worlds directory)")
	flags.StringVar(&opts.bdsVersion, "bds-version", "", "exact Bedrock Dedicated Server build to download (the client passes its target manifest's server_version)")
	flags.StringVar(&opts.bdsImage, "bds-image", "", "digest-pinned container image that runs the Linux Bedrock Dedicated Server where no native build exists")
	flags.StringVar(&opts.docker, "docker", "docker", "Docker-compatible CLI used to run the Linux Bedrock Dedicated Server where no native build exists")
	if err := flags.Parse(args); err != nil {
		return options{}, err
	}
	switch opts.localBackend {
	case "auto", "bds", "dragonfly":
	default:
		return options{}, errors.New("local-backend must be auto, bds or dragonfly")
	}
	if opts.localWorldsDir != "" && !opts.controlStatus {
		return options{}, errors.New("local-worlds-dir requires -control-status")
	}
	if opts.localServerBin != "" && opts.localWorldsDir == "" {
		return options{}, errors.New("local-server-bin requires -local-worlds-dir")
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
	var statusStore *control.Store
	var controlServer *control.Server
	if opts.controlStatus && opts.catalogFile == "" {
		// Bound before authentication so a launcher can poll the device code.
		statusStore = control.NewStore()
		controlServer, err = control.Start(opts.socketDir, statusStore)
		if err != nil {
			return fmt.Errorf("start control endpoint: %w", err)
		}
		defer func() { _ = controlServer.Close() }()
	}
	authentication := "offline"
	var tokenSource oauth2.TokenSource
	var account *authcache.Account
	if statusStore != nil {
		statusStore.SetAuth(control.AuthV1{State: control.AuthOffline})
	}
	if opts.authCache != "" {
		authentication = "microsoft"
		logger.Info("authentication starting", "mode", authentication)
		authConfig := authcache.Config{Path: opts.authCache, Writer: stdout}
		if statusStore != nil {
			authConfig.Request = launcher.DeviceRequest(statusStore)
		}
		tokenSource, err = source(ctx, authConfig)
		if err != nil {
			if statusStore != nil && statusStore.Auth().State != control.AuthFailed {
				statusStore.SetAuth(control.AuthV1{State: control.AuthFailed, Reason: "Could not validate the saved account."})
			}
			return fmt.Errorf("initialize Microsoft authentication: %w", err)
		}
		if account = authcache.NewAccount(ctx, authcache.DerivedCachePath(opts.authCache), tokenSource, stderr); account != nil {
			tokenSource = account
			defer func() { _ = account.Close() }()
		}
		if statusStore != nil {
			statusStore.SetAuth(control.AuthV1{State: control.AuthSignedIn})
		}
	}
	logger.Info("authentication ready", "mode", authentication)
	if opts.catalogFile != "" {
		if opts.resourcePackCacheDir != "" {
			return errors.New("catalog mode cannot be combined with resource-pack cache options")
		}
		if account == nil {
			return errors.New("catalog mode requires -auth-cache")
		}
		if err := catalog.Write(ctx, opts.catalogFile, account); err != nil {
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
			// Persistence is optional. Keep the cache's permission and integrity
			// checks fail-closed without turning a cache miss into a join failure.
			// The underlying error may contain private paths; do not log it.
			logger.Warn("resource pack cache unavailable; continuing without persistent cache")
		} else {
			resourcePackCache = cache
			closeResourcePackCache = cache.Close
		}
	}
	var localWorlds *localworld.Manager
	var localTarget proxy.LocalTargetFunc
	if opts.localWorldsDir != "" {
		localWorlds, err = openLocalWorlds(opts, logger)
		if err != nil {
			if closeResourcePackCache != nil {
				_ = closeResourcePackCache()
			}
			return err
		}
		localTarget = localWorlds.Target
	}
	var resourcePackAdmissionUpdate func(proxy.ResourcePackAdmissionSnapshot)
	var connectProgress func(proxy.ConnectProgress)
	transfers := new(proxy.TransferState)
	selector := new(proxy.UpstreamSelector)
	var onDisconnect func(proxy.DisconnectInfo)
	if statusStore != nil {
		if localWorlds != nil {
			// Opening a local world supersedes any pending transfer or selected upstream.
			controlServer.SetWorlds(control.WithOpenHook(localWorlds, func() {
				transfers.Clear()
				selector.Set("")
				statusStore.ClearTransfer()
			}))
		}
		artworkDir, cacheFile := filepath.Join(opts.socketDir, "artwork"), ""
		if dir := authSibling(opts.authCache, "catalog-cache"); dir != "" {
			artworkDir, cacheFile = filepath.Join(dir, "artwork"), filepath.Join(dir, "catalog.json")
		}
		service := launcher.New(launcher.Config{
			Account: account, AuthCache: opts.authCache, Language: opts.language,
			Store: statusStore, Selector: selector, Transfers: transfers,
			ArtworkDir: artworkDir, CacheFile: cacheFile, Logger: logger,
			StoreImageDir: authSibling(opts.authCache, "store-images"),
		})
		controlServer.SetLogger(logger)
		controlServer.SetServices(service)
		controlServer.SetMarketplace(service.Marketplace())
		if account != nil {
			go service.PublishSignedIn(ctx)
			service.Prefetch()
		}
		statusStore.SetLifecycle(control.LifecycleRunning)
		resourcePackAdmissionUpdate = statusStore.Observe
		connectProgress = statusStore.ObserveConnectProgress
		transfers.OnTransfer = statusStore.ObserveTransfer
		onDisconnect = statusStore.ObserveDisconnect
	}
	serveErr := serve(ctx, proxy.Config{
		SocketDir:           opts.socketDir,
		Upstream:            opts.upstream,
		Account:             account,
		Logger:              logger,
		UpstreamClientCache: opts.upstreamClientCache,
		Transfers:           transfers,
		Selector:            selector,
		OnDisconnect:        onDisconnect,
		LocalTarget:         localTarget,
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
		ConnectProgress:             connectProgress,
	})
	if controlServer != nil {
		serveErr = errors.Join(serveErr, controlServer.Close())
	}
	if localWorlds != nil {
		localWorlds.Shutdown()
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

// authSibling is the persistent per-install directory called name beside the auth cache; empty without one.
func authSibling(authCache, name string) string {
	if authCache == "" {
		return ""
	}
	return filepath.Join(filepath.Dir(authCache), name)
}
