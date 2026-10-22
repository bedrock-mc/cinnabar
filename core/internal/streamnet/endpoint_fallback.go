//go:build !windows && !ios

package streamnet

// unixEndpointFallbackDir holds the hashed endpoint of a socket directory too long for sun_path.
const unixEndpointFallbackDir = "/tmp"
