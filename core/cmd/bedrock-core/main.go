package main

import (
	"context"
	"flag"
	"log"
	"os/signal"
	"syscall"

	"github.com/hashimthearab/rust-mcbe/core/proxy"
)

func main() {
	socketDir := flag.String("socket-dir", "", "directory containing the local bridge endpoint")
	upstream := flag.String("upstream", "", "upstream Bedrock server address (host:port)")
	flag.Parse()

	ctx, stop := signal.NotifyContext(context.Background(), syscall.SIGINT, syscall.SIGTERM)
	defer stop()
	if err := proxy.Serve(ctx, proxy.Config{SocketDir: *socketDir, Upstream: *upstream}); err != nil {
		log.Fatal(err)
	}
}
