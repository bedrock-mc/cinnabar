// Command bedrock-web-relay runs the browser relay with operator-supplied TLS and network binds.
package main

import (
	"context"
	"flag"
	"log/slog"
	"os"
	"os/signal"
	"strings"
	"syscall"

	"github.com/hashimthearab/rust-mcbe/core/webrelay"
)

// main composes the relay without changing the native core command or serving assets.
func main() {
	var cfg webrelay.ListenConfig
	flag.StringVar(&cfg.Hostname, "hostname", "", "relay TLS hostname")
	flag.StringVar(&cfg.UDPAddress, "udp", "", "HTTP/3 UDP listen address")
	flag.StringVar(&cfg.TCPAddress, "tcp", "", "HTTPS WebSocket TCP listen address")
	flag.StringVar(&cfg.CertificateFile, "tls-cert", "", "PEM certificate chain path")
	flag.StringVar(&cfg.KeyFile, "tls-key", "", "PEM private key path")
	origins := flag.String("origins", "", "comma-separated allowed page origins; empty requires relay host")
	flag.Parse()
	var allowed []string
	for _, origin := range strings.Split(*origins, ",") {
		if origin = strings.TrimSpace(origin); origin != "" {
			allowed = append(allowed, origin)
		}
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	relay := webrelay.New(webrelay.Config{AllowedOrigins: allowed})
	defer relay.Close()
	go relay.Run(ctx)
	if err := relay.Serve(ctx, cfg); err != nil {
		slog.Error("relay listener failed", "error", err)
		os.Exit(1)
	}
}
