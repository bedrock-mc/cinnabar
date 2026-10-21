// Command bedrock-local-server hosts saved local worlds.
package main

import (
	"fmt"
	localserver "github.com/hashimthearab/rust-mcbe/tools/localserver"
	"os"
)

// main reports startup errors and leaves server ownership to the library.
func main() {
	if err := localserver.Run(os.Args[1:], os.Stdin, os.Stdout, os.Stderr); err != nil {
		fmt.Fprintln(os.Stderr, "bedrock-local-server:", err)
		os.Exit(1)
	}
}
