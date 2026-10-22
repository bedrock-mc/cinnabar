package streamnet

// unixEndpointFallbackDir holds the hashed endpoint of a socket directory too long for sun_path.
// An iOS app container path alone nearly fills sun_path and the sandbox has no shared /tmp, so
// the endpoint is relative: the iOS host makes its private runtime root the working directory.
const unixEndpointFallbackDir = ""
