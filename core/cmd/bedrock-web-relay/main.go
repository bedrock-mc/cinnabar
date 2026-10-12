// Command bedrock-web-relay serves Cinnabar's browser build: Microsoft sign-in and Bedrock joins
// for each browser session, over WebSockets, and optionally the built site itself.
package main

import (
	"context"
	"crypto/tls"
	"errors"
	"flag"
	"fmt"
	"log/slog"
	"net"
	"net/http"
	"net/netip"
	"os"
	"os/signal"
	"strings"
	"syscall"
	"time"

	"github.com/hashimthearab/rust-mcbe/core/webrelay"
)

type options struct {
	listen               string
	allowedOrigins       string
	vanillaSource        string
	siteDir              string
	devAssets            string
	tlsCert, tlsKey      string
	devSelfSigned        bool
	allowPrivateUpstream bool
	trustForwardedFor    bool
}

func main() {
	var opts options
	flag.StringVar(&opts.listen, "listen", "127.0.0.1:8787", "HTTP listen address")
	flag.StringVar(&opts.allowedOrigins, "allowed-origin", "", "comma-separated page origin hosts allowed to open WebSockets (default: same host only)")
	flag.StringVar(&opts.vanillaSource, "vanilla-source", "assets/vanilla-source.json", "vanilla pack manifest to stream through; empty disables it")
	flag.StringVar(&opts.siteDir, "site", "", "directory of the built browser site to serve at /")
	flag.StringVar(&opts.tlsCert, "tls-cert", "", "PEM certificate to serve HTTPS with; needs -tls-key")
	flag.StringVar(&opts.tlsKey, "tls-key", "", "PEM private key for -tls-cert")
	flag.BoolVar(&opts.devSelfSigned, "dev-self-signed", false, "serve HTTPS with a generated certificate for this machine's addresses, which browsers must accept once")
	flag.StringVar(&opts.devAssets, "dev-assets", "", "compiled carrier directory to serve so pages need no import; Mojang-derived, private relays only")
	flag.BoolVar(&opts.allowPrivateUpstream, "dev-allow-private-upstream", false, "let sessions join private addresses; for development relays only")
	flag.BoolVar(&opts.trustForwardedFor, "trust-forwarded-for", false, "read client addresses from X-Forwarded-For; loopback listeners only")
	flag.Parse()
	logger := slog.New(slog.NewTextHandler(os.Stderr, nil))
	if err := run(opts, logger); err != nil {
		fmt.Fprintln(os.Stderr, "bedrock-web-relay:", err)
		os.Exit(1)
	}
}

func run(opts options, logger *slog.Logger) error {
	loopback, err := loopbackListen(opts.listen)
	if err != nil {
		return err
	}
	if opts.trustForwardedFor && !loopback {
		return errors.New("-trust-forwarded-for needs a loopback -listen address")
	}
	if opts.allowPrivateUpstream && !loopback {
		logger.Warn("every client that reaches this relay can join RakNet servers on its private networks")
	}
	cfg := webrelay.Config{
		AllowPrivateUpstream: opts.allowPrivateUpstream,
		TrustForwardedFor:    opts.trustForwardedFor,
		Logger:               logger,
	}
	for _, origin := range strings.Split(opts.allowedOrigins, ",") {
		if origin = strings.TrimSpace(origin); origin != "" {
			cfg.AllowedOrigins = append(cfg.AllowedOrigins, origin)
		}
	}
	if opts.vanillaSource != "" {
		if cfg.Vanilla, err = webrelay.LoadVanillaSource(opts.vanillaSource); err != nil {
			return err
		}
	}
	relay := webrelay.New(cfg)
	mux := http.NewServeMux()
	mux.Handle("/v1/", relay.Handler())
	if opts.siteDir != "" {
		site := webrelay.StaticFiles(opts.siteDir)
		go site.Warm(0)
		mux.Handle("/", site)
	}
	if opts.devAssets != "" {
		mux.Handle(webrelay.DevAssetsPath, webrelay.DevAssets(opts.devAssets))
		logger.Warn("serving Mojang-derived compiled carriers to every client that reaches this relay", "dir", opts.devAssets)
	}
	server := &http.Server{
		Addr:              opts.listen,
		Handler:           mux,
		ReadHeaderTimeout: 10 * time.Second,
	}
	scheme := "http"
	switch {
	case opts.devSelfSigned && opts.tlsCert != "":
		return errors.New("-dev-self-signed and -tls-cert are exclusive")
	case opts.devSelfSigned:
		certificate, err := selfSignedCertificate()
		if err != nil {
			return fmt.Errorf("generate a self-signed certificate: %w", err)
		}
		server.TLSConfig = &tls.Config{Certificates: []tls.Certificate{certificate}, MinVersion: tls.VersionTLS12}
		scheme = "https"
	case opts.tlsCert != "" || opts.tlsKey != "":
		if opts.tlsCert == "" || opts.tlsKey == "" {
			return errors.New("-tls-cert and -tls-key go together")
		}
		scheme = "https"
	}

	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	go relay.Run(ctx)
	serveErr := make(chan error, 1)
	go func() {
		switch {
		case server.TLSConfig != nil:
			serveErr <- server.ListenAndServeTLS("", "")
		case scheme == "https":
			serveErr <- server.ListenAndServeTLS(opts.tlsCert, opts.tlsKey)
		default:
			serveErr <- server.ListenAndServe()
		}
	}()
	logger.Info("web relay listening", "address", opts.listen, "scheme", scheme, "site", opts.siteDir != "")
	select {
	case err = <-serveErr:
	case <-ctx.Done():
		shutdown, cancel := context.WithTimeout(context.Background(), 5*time.Second)
		defer cancel()
		err = server.Shutdown(shutdown)
	}
	relay.Close()
	if errors.Is(err, http.ErrServerClosed) {
		return nil
	}
	return err
}

// loopbackListen reports whether address listens on loopback only.
func loopbackListen(address string) (bool, error) {
	host, _, err := net.SplitHostPort(address)
	if err != nil {
		return false, fmt.Errorf("-listen: %w", err)
	}
	if host == "localhost" {
		return true, nil
	}
	addr, err := netip.ParseAddr(host)
	return err == nil && addr.IsLoopback(), nil
}
