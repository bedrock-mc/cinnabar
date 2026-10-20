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
	"runtime"
	"runtime/debug"
	"strings"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/authcache"
	"github.com/hashimthearab/rust-mcbe/core/authflow"
	"github.com/hashimthearab/rust-mcbe/core/catalog"
	"github.com/hashimthearab/rust-mcbe/core/cinnabarapi"
	"github.com/hashimthearab/rust-mcbe/core/control"
	"github.com/hashimthearab/rust-mcbe/core/internal/corelog"
	"github.com/hashimthearab/rust-mcbe/core/internal/latencytrace"
	"github.com/hashimthearab/rust-mcbe/core/internal/lifeline"
	"github.com/hashimthearab/rust-mcbe/core/internal/locale"
	"github.com/hashimthearab/rust-mcbe/core/internal/redact"
	"github.com/hashimthearab/rust-mcbe/core/launcher"
	"github.com/hashimthearab/rust-mcbe/core/localworld"
	"github.com/hashimthearab/rust-mcbe/core/packcache"
	"github.com/hashimthearab/rust-mcbe/core/proxy"
	"github.com/sandertv/gophertunnel/minecraft"
	"golang.org/x/oauth2"
)

const (
	// The core relays one session beside a game client that owns the remaining cores.
	coreMaxProcs = 2
	// Soft heap target; GC works harder near it instead of growing past it.
	coreMemoryLimit = 512 << 20
	// Past this, a shutdown still waiting on work that ignores its context hard-exits.
	shutdownGrace      = 2 * time.Second
	parentPollInterval = 250 * time.Millisecond
)

// The build stamps these with -ldflags "-X main.version=<workspace version> -X main.commit=<commit>",
// the release and commit the Rust client reports, so a core crash can be matched to its client.
var (
	version = "unknown"
	commit  = "unknown"
)

func main() {
	code := runMain()
	if code != 0 {
		os.Exit(code)
	}
}

// runMain owns the core's runtime settings, lifeline and services.
func runMain() int {
	configureRuntime(os.Getenv)
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
	exitCode := execute(ctx, args, os.Stdout, os.Stderr, authcache.Source, proxy.Serve)
	latencytrace.Finish()
	stop()
	return exitCode
}

// configureRuntime caps scheduler threads and sets a soft memory limit unless GOMAXPROCS or
// GOMEMLIMIT already chose them.
func configureRuntime(getenv func(string) string) {
	if getenv("GOMAXPROCS") == "" {
		runtime.GOMAXPROCS(min(coreMaxProcs, runtime.NumCPU()))
	}
	if getenv("GOMEMLIMIT") == "" {
		debug.SetMemoryLimit(coreMemoryLimit)
	}
}

// bindsStdin reports whether the client pipes stdin, whose EOF then ends the core; the sign-in
// helper runs with a null stdin.
func bindsStdin(args []string) bool {
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
	deviceFile                string
	language                  string
	catalogFile               string
	authEvents                bool
	resourcePackCacheDir      string
	resourcePackCacheQuota    uint64
	resourcePackCacheQuotaSet bool
	controlStatus             bool
	cinnabarAPI               bool
	xboxPresence              bool
	upstreamClientCache       bool
	serverTrustFile           string
	logLevel                  slog.Level
	crashOutput               string
}

// process names the core's role in every log record, so interleaved cores stay distinguishable.
func (opts options) process() string {
	switch {
	case opts.authEvents:
		return "auth-events"
	case opts.catalogFile != "":
		return "catalog"
	case opts.xboxPresence:
		return "account"
	case opts.upstream != "":
		return "game"
	}
	return "bedrock-core"
}

func parseFlags(args []string, stderr io.Writer) (options, error) {
	flags := flag.NewFlagSet("bedrock-core", flag.ContinueOnError)
	flags.SetOutput(stderr)
	var opts options
	flags.StringVar(&opts.socketDir, "socket-dir", "", "directory containing the local bridge endpoint")
	flags.StringVar(&opts.upstream, "upstream", "", "upstream Bedrock server address (host:port)")
	flags.StringVar(&opts.authCache, "auth-cache", "", "path to the Microsoft authentication token cache")
	flags.StringVar(&opts.deviceFile, "device-file", "", "the install's persisted login device profile (default: device.json beside -auth-cache)")
	flags.StringVar(&opts.language, "language", locale.Default, "active UI language (BCP 47)")
	flags.StringVar(&opts.catalogFile, "catalog-file", "", "write the authenticated launcher catalog and exit")
	flags.BoolVar(&opts.authEvents, "auth-events", false, "perform one-shot authentication and emit bounded JSONL events")
	flags.StringVar(&opts.resourcePackCacheDir, "resource-pack-cache-dir", "", "enable the persistent verified resource-pack cache in this directory")
	flags.Uint64Var(&opts.resourcePackCacheQuota, "resource-pack-cache-quota-bytes", packcache.DefaultQuota, "maximum resource-pack cache bytes (requires -resource-pack-cache-dir)")
	flags.BoolVar(&opts.controlStatus, "control-status", false, "enable the local read-only Status v1 control endpoint")
	flags.BoolVar(&opts.xboxPresence, "xbox-presence", false, "own Xbox title presence for the account (requires -control-status)")
	flags.BoolVar(&opts.cinnabarAPI, "cinnabar-api", false, "connect the account to the Cinnabar API once the control client configures it (requires -control-status)")
	flags.StringVar(&opts.serverTrustFile, "server-trust-file", "", "ask the control client before joining an unknown http NetherNet server, remembering trusted ones in this file (requires -control-status)")
	flags.BoolVar(&opts.upstreamClientCache, "upstream-client-cache", false, "advertise client-cache capability upstream; enable only when the connecting client owns a verified blob cache")
	corelog.LevelFlag(flags, &opts.logLevel)
	flags.StringVar(&opts.crashOutput, "crash-output", "", "also append unhandled panics and fatal runtime errors, with every goroutine's stack, to this file (created owner-only); unset leaves them on stderr only")
	if err := flags.Parse(args); err != nil {
		return options{}, err
	}
	if opts.xboxPresence && !opts.controlStatus {
		return options{}, errors.New("xbox-presence requires -control-status")
	}
	if opts.cinnabarAPI && !opts.controlStatus {
		return options{}, errors.New("cinnabar-api requires -control-status")
	}
	if opts.serverTrustFile != "" && !opts.controlStatus {
		return options{}, errors.New("server-trust-file requires -control-status")
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

// Replaced in tests that must not reach the network or change process-wide state.
var (
	startVerifierPreload = proxy.StartVerifierPreload
	keepAccountFresh     = (*authcache.Account).KeepFresh
	installDiagnostics   = installProcessDiagnostics
)

// installProcessDiagnostics makes logger the process default, so library and lifeline records share
// its format, and arms -crash-output. A crash output that cannot be opened leaves crashes on stderr.
func installProcessDiagnostics(opts options, logger *slog.Logger) {
	slog.SetDefault(logger)
	if opts.crashOutput == "" {
		return
	}
	if err := corelog.SetCrashOutput(opts.crashOutput); err != nil {
		// The error names the private path; the level is enough to act on.
		logger.Warn("crash output unavailable; crashes reach stderr only")
	}
}

type ownedResourcePackCache interface {
	minecraft.ResourcePackCache
	Close() error
}
type resourcePackCacheFactory func(string, ...packcache.Option) (ownedResourcePackCache, error)

func execute(ctx context.Context, args []string, stdout, stderr io.Writer, source sourceFunc, serve serveFunc) int {
	if err := run(ctx, args, stdout, stderr, source, serve); err != nil {
		return 1
	}
	return 0
}

// run parses args, installs the process's root logger on stderr and runs the core; a failure is also
// logged as "core failed".
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
) (err error) {
	opts, err := parseFlags(args, stderr)
	if errors.Is(err, flag.ErrHelp) {
		return nil
	}
	// Invalid arguments leave opts zero: the failure is still logged, as bedrock-core at info.
	logger := corelog.New(stderr, opts.process(), opts.logLevel)
	installDiagnostics(opts, logger)
	defer func() {
		if err != nil {
			logger.Error("core failed", "error", redact.Error(err))
		}
	}()
	if err != nil {
		return err
	}
	logger.Info("core build", "version", version, "commit", commit, "go", runtime.Version())
	if opts.authEvents {
		if opts.authCache == "" {
			return errors.New("auth-events mode requires -auth-cache")
		}
		if opts.socketDir != "" || opts.upstream != "" || opts.catalogFile != "" || opts.resourcePackCacheDir != "" || opts.controlStatus {
			return errors.New("auth-events mode cannot be combined with proxy or catalog options")
		}
		return authflow.Run(ctx, authflow.Config{
			Path: opts.authCache, Writer: stdout, Logger: logger,
			CompleteSignIn: func(ctx context.Context, path string, source oauth2.TokenSource) error {
				return authcache.CompleteSignIn(ctx, path, source, logger)
			},
		})
	}
	logger.Info("core starting", "endpoint", opts.socketDir, "upstream", redact.Private(opts.upstream))
	if opts.catalogFile == "" {
		defer startVerifierPreload(ctx, logger)()
	}
	var statusStore *control.Store
	var controlServer *control.Server
	packetDelay := new(proxy.PacketDelay)
	if opts.controlStatus && opts.catalogFile == "" {
		// Bound before authentication so a launcher can poll the device code.
		statusStore = control.NewStore()
		controlServer, err = control.Start(opts.socketDir, statusStore)
		if err != nil {
			return fmt.Errorf("start control endpoint: %w", err)
		}
		defer func() { _ = controlServer.Close() }()
		controlServer.SetPacketDelay(packetDelayControl{packetDelay})
	}
	authentication := "offline"
	var tokenSource oauth2.TokenSource
	var account *authcache.Account
	closeCredentials := context.CancelFunc(func() {})
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
		credentialCtx, cancelCredentials := context.WithCancel(context.WithoutCancel(ctx))
		closeCredentials = cancelCredentials
		cancelAuthentication := context.AfterFunc(ctx, closeCredentials)
		defer func() { cancelAuthentication(); closeCredentials() }()
		tokenSource, err = source(credentialCtx, authConfig)
		if err != nil {
			if statusStore != nil && statusStore.Auth().State != control.AuthFailed {
				statusStore.SetAuth(control.AuthV1{State: control.AuthFailed, Reason: "Could not validate the saved account."})
			}
			return fmt.Errorf("initialize Microsoft authentication: %w", err)
		}
		if err := ctx.Err(); err != nil {
			return err
		}
		if account = authcache.NewAccount(credentialCtx, authcache.DerivedCachePath(opts.authCache), tokenSource, logger); account != nil {
			tokenSource = account
			defer func() { closeCredentials(); _ = account.Close() }()
		}
		// Initialization follows shutdown; afterward credentials survive title cleanup.
		cancelAuthentication()
		if err := ctx.Err(); err != nil {
			return err
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
	if account != nil {
		// Sign-out or an account change closes the account, which ends the refresher.
		refreshed := make(chan struct{})
		go func() {
			defer close(refreshed)
			keepAccountFresh(account, ctx)
		}()
		defer func() {
			closeCredentials()
			_ = account.Close()
			<-refreshed
		}()
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
	// The launcher runs local worlds and reports the open one over local_world.v1.
	var localWorlds *localworld.Local
	var localTarget proxy.LocalTargetFunc
	if statusStore != nil {
		localWorlds = localworld.NewLocal()
		localTarget = localWorlds.ConnectionTarget
	}
	var resourcePackAdmissionUpdate func(proxy.ResourcePackAdmissionSnapshot)
	var connectProgress func(proxy.ConnectProgress)
	transfers := new(proxy.TransferState)
	selector := new(proxy.UpstreamSelector)
	var onDisconnect func(proxy.DisconnectInfo)
	var networkObservation func(proxy.NetworkObservation)
	var sessionTarget func(context.Context, string, string) (string, error)
	var serverTrust minecraft.ServerTrust
	if statusStore != nil {
		// Opening a local world supersedes any pending transfer or selected upstream.
		worlds := controlWorlds{local: localWorlds, onOpen: func() {
			transfers.Clear()
			selector.Set("")
			statusStore.ClearTransfer()
		}}
		if account != nil {
			hosting := &friendHosting{account: account, worlds: localWorlds, log: logger}
			hostingCtx, stopHosting := context.WithCancel(ctx)
			hostingDone := make(chan struct{})
			go func() {
				defer close(hostingDone)
				hosting.run(hostingCtx)
			}()
			defer func() {
				stopHosting()
				<-hostingDone
			}()
			worlds.invite = hosting.Invite
		}
		controlServer.SetWorlds(worlds)
		artworkDir, cacheFile := filepath.Join(opts.socketDir, "artwork"), ""
		if dir := authSibling(opts.authCache, "catalog-cache"); dir != "" {
			artworkDir, cacheFile = filepath.Join(dir, "artwork"), filepath.Join(dir, "catalog.json")
		}
		service := launcher.New(launcher.Config{
			Account: account, AuthCache: opts.authCache, Language: opts.language,
			Store: statusStore, Selector: selector, Transfers: transfers,
			ArtworkDir: artworkDir, CacheFile: cacheFile, Logger: logger,
		})
		if opts.xboxPresence {
			presence := service.StartPresence(ctx)
			defer presence.Close()
			controlServer.SetPresence(presence.Set)
		}
		if opts.cinnabarAPI {
			api := service.StartCinnabarAPI(ctx, func(release cinnabarapi.Release) {
				statusStore.ObserveCinnabarRelease(release)
			})
			defer api.Close()
			controlServer.SetCinnabarAPI(api)
		}
		controlServer.SetLogger(logger)
		controlServer.SetServices(launcherControl{service})
		sessionTarget = service.SessionTarget
		if account != nil {
			go service.PublishSignedIn(ctx)
			service.Prefetch()
		}
		statusStore.SetLifecycle(control.LifecycleRunning)
		resourcePackAdmissionUpdate = func(snapshot proxy.ResourcePackAdmissionSnapshot) {
			statusStore.Observe(wirePackAdmission(snapshot))
		}
		connectProgress = func(progress proxy.ConnectProgress) {
			statusStore.ObserveConnectProgress(wireConnectProgress(progress))
		}
		transfers.OnTransfer = func(target proxy.TransferTarget) { statusStore.ObserveTransfer(target.Host, target.Port) }
		onDisconnect = func(info proxy.DisconnectInfo) { statusStore.ObserveDisconnect(info.Reason, info.Message) }
		networkObservation = func(observation proxy.NetworkObservation) {
			statusStore.ObserveNetwork(control.NetworkObservation{AttemptID: observation.AttemptID, LocalRuntimeID: observation.LocalRuntimeID, RoundTripMillis: observation.RoundTripMillis})
		}

		if opts.serverTrustFile != "" {
			prompts := proxy.NewServerTrustPrompts(func(prompt proxy.ServerTrustPrompt, pending bool) {
				statusStore.ObserveServerTrust(control.ServerTrustPrompt(prompt), pending)
			})
			statusStore.SetServerTrustAnswer(prompts.Answer)
			serverTrust = &minecraft.FirstUseTrust{
				Store:   proxy.ServerTrustFile(opts.serverTrustFile),
				Confirm: prompts.Confirm,
				Log:     logger,
			}
		}
	}
	// One device per install, consistent with the platform the account signs in as.
	deviceProfile, deviceErr := authcache.LoadDevice(opts.devicePath())
	if deviceErr != nil {
		logger.Warn("device profile not saved; using it for this run only")
	}
	serveErr := serve(ctx, proxy.Config{
		PacketDelay:         packetDelay,
		SocketDir:           opts.socketDir,
		Upstream:            opts.upstream,
		Account:             account,
		Logger:              logger,
		UpstreamClientCache: opts.upstreamClientCache,
		Transfers:           transfers,
		Selector:            selector,
		OnDisconnect:        onDisconnect,
		NetworkObservation:  networkObservation,
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
		ServerTrust:                 serverTrust,
		SessionTarget:               sessionTarget,
		Device:                      &deviceProfile,
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

// devicePath is the install's device profile, independent of which account's token the core holds.
func (opts options) devicePath() string {
	if opts.deviceFile != "" {
		return opts.deviceFile
	}
	return authSibling(opts.authCache, "device.json")
}

// authSibling is the persistent per-install directory called name beside the auth cache; empty without one.
func authSibling(authCache, name string) string {
	if authCache == "" {
		return ""
	}
	return filepath.Join(filepath.Dir(authCache), name)
}
