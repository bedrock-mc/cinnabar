package main

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"net/http"
	"net/netip"
	"net/url"
	"os"
	"os/signal"
	"strings"
	"sync/atomic"
	"syscall"
	"time"

	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/ingress"
	"github.com/bedrock-mc/cinnabar/tools/web-spectator/server/internal/spectator"
	"github.com/nats-io/nats.go"
)

func main() {
	log := slog.New(slog.NewJSONHandler(os.Stderr, nil))
	if err := run(log); err != nil {
		log.Error("spectator server stopped", "error", err)
		os.Exit(1)
	}
}

func run(log *slog.Logger) error {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	upstream, err := httpURL(env("SPECTATOR_WEBSITE_URL", "http://127.0.0.1:3001"), false)
	if err != nil {
		return fmt.Errorf("SPECTATOR_WEBSITE_URL: %w", err)
	}
	origin, err := httpURL(env("SPECTATOR_PUBLIC_ORIGIN", "https://dev.zenomc.org"), true)
	if err != nil {
		return fmt.Errorf("SPECTATOR_PUBLIC_ORIGIN: %w", err)
	}
	trusted, err := trustedProxies(env("SPECTATOR_TRUSTED_PROXIES", "127.0.0.1/32,::1/128"))
	if err != nil {
		return err
	}
	store := spectator.NewStore()
	options := []nats.Option{nats.Name("cinnabar-web-spectator"), nats.Timeout(5 * time.Second), nats.MaxReconnects(-1), nats.ReconnectWait(time.Second), nats.ReconnectBufSize(0), nats.DisconnectErrHandler(func(_ *nats.Conn, _ error) { store.CloseAll(time.Now()); log.Warn("spectator bus disconnected") }), nats.ErrorHandler(func(_ *nats.Conn, _ *nats.Subscription, _ error) {
		store.CloseAll(time.Now())
		log.Warn("spectator subscription lost messages; active views closed")
	})}
	if credentials := os.Getenv("SPECTATOR_NATS_CREDENTIALS_FILE"); credentials != "" {
		options = append(options, nats.UserCredentials(credentials))
	}
	connection, err := nats.Connect(env("SPECTATOR_NATS_URL", nats.DefaultURL), options...)
	if err != nil {
		return errors.New("could not connect to spectator event bus")
	}
	defer connection.Close()
	var rejected atomic.Uint64
	subscription, err := connection.Subscribe("practice.spectator.v1.>", func(message *nats.Msg) {
		if err := store.Accept(message.Subject, message.Data, time.Now()); err != nil {
			rejected.Add(1)
		}
	})
	if err != nil {
		return errors.New("could not subscribe to spectator events")
	}
	if err := subscription.SetPendingLimits(512, 8<<20); err != nil {
		return err
	}
	if err := connection.FlushTimeout(5 * time.Second); err != nil {
		return errors.New("spectator event subscription did not become ready")
	}
	server := &http.Server{Addr: env("SPECTATOR_LISTEN_ADDRESS", "127.0.0.1:3002"), Handler: ingress.New(store, upstream, origin, trusted), ReadHeaderTimeout: 5 * time.Second, ReadTimeout: 15 * time.Second, WriteTimeout: 60 * time.Second, IdleTimeout: 60 * time.Second, MaxHeaderBytes: 16 << 10}
	go func() {
		ticker := time.NewTicker(time.Second)
		defer ticker.Stop()
		for {
			select {
			case <-ctx.Done():
				store.CloseAll(time.Now())
				shutdown, cancel := context.WithTimeout(context.Background(), 10*time.Second)
				defer cancel()
				if err := server.Shutdown(shutdown); err != nil {
					_ = server.Close()
				}
				return
			case now := <-ticker.C:
				store.Sweep(now)
				if count := rejected.Swap(0); count > 0 {
					log.Warn("invalid or unavailable spectator exports skipped", "count", count)
				}
			}
		}
	}()
	log.Info("starting read-only spectator ingress", "address", server.Addr, "origin", origin.String())
	if err := server.ListenAndServe(); err != nil && !errors.Is(err, http.ErrServerClosed) {
		return err
	}
	return nil
}

func env(name, fallback string) string {
	if value := os.Getenv(name); value != "" {
		return value
	}
	return fallback
}

func httpURL(raw string, origin bool) (*url.URL, error) {
	value, err := url.Parse(raw)
	if err != nil || value.Host == "" || (value.Scheme != "http" && value.Scheme != "https") || value.User != nil || value.RawQuery != "" || value.Fragment != "" || (value.Path != "" && value.Path != "/") {
		return nil, errors.New("expected an HTTP origin without credentials or a path")
	}
	value.Path = ""
	if origin && value.Scheme != "https" && value.Hostname() != "localhost" && value.Hostname() != "127.0.0.1" {
		return nil, errors.New("public origin requires HTTPS")
	}
	return value, nil
}

func trustedProxies(raw string) ([]netip.Prefix, error) {
	var prefixes []netip.Prefix
	for _, entry := range strings.Split(raw, ",") {
		entry = strings.TrimSpace(entry)
		if entry == "" {
			continue
		}
		prefix, err := netip.ParsePrefix(entry)
		if err != nil || prefix.Bits() == 0 {
			return nil, errors.New("SPECTATOR_TRUSTED_PROXIES requires explicit trusted network prefixes")
		}
		prefixes = append(prefixes, prefix.Masked())
	}
	return prefixes, nil
}
