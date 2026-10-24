// Package cinnabarext runs the server half of the Cinnabar extension wire next to Dragonfly.
// Document declarations, canonical codecs, signing domains and limits are generated from
// crates/server-experience/schema/extension.json. Native signing and connection execution stay
// here. The bidirectional fixtures check signatures and messages against the client boundary.
package cinnabarext
