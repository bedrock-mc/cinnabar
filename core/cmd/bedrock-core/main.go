package main

import (
	"context"
	"flag"
	"io"
	"log"
	"os"
	"os/signal"
	"syscall"

	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

func main() {
	socketDir := flag.String("socket-dir", "", "directory containing the local bridge endpoint")
	upstream := flag.String("upstream", "", "upstream Bedrock server address (host:port)")
	flag.Parse()

	signalCtx, stopSignals := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stopSignals()
	ctx, stopStdin := contextWithStdinEOF(signalCtx, os.Stdin)
	defer stopStdin()
	if err := proxy.Serve(ctx, proxy.Config{SocketDir: *socketDir, Upstream: *upstream}); err != nil {
		log.Fatal(err)
	}
}

func contextWithStdinEOF(parent context.Context, stdin io.Reader) (context.Context, context.CancelFunc) {
	ctx, cancel := context.WithCancel(parent)
	go func() {
		_, _ = io.Copy(io.Discard, stdin)
		cancel()
	}()
	return ctx, cancel
}
