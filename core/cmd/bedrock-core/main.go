// Command bedrock-core is the core process the client starts beside itself.
package main

import "github.com/hashimthearab/rust-mcbe/core/coreapp"

// The build stamps the executable identity for matching client and core diagnostics.
var (
	version = "unknown"
	commit  = "unknown"
)

func main() {
	coreapp.Main(coreapp.Options{Version: version, Commit: commit})
}
