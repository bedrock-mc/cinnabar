// Generated from crates/server-experience/schema/extension.json; do not edit.
package cinnabarext

const (
	MarkerPath              = "cinnabar/extension-offer.json"
	WireVersion             = 1
	MaxWireVersion          = 2
	APIVersion              = 1
	MaxIdentifierBytes      = 96
	MaxURLBytes             = 2048
	MaxOrigins              = 8
	MaxFallbackBytes        = 512
	MaxMarkerBytes          = 64 * 1024
	MaxPayloadBytes         = 16 * 1024
	MaxMessageBytes         = 48 * 1024
	MaxMessagesPerSecond    = 64
	MaxBytesPerSecond       = 64 * 1024
	MaxQueueBytes           = 256 * 1024
	MaxChannels             = 64
	MaxChannelFields        = 64
	MaxFieldDepth           = 4
	InitialBundleGeneration = 1
	MaxBundles              = 4
	MaxBundleBytes          = 64 * 1024 * 1024
	MaxExpandedBytes        = 256 * 1024 * 1024
	MaxGuestMemory          = 32 * 1024 * 1024
	MaxSessionMemory        = 64 * 1024 * 1024
	MaxGPUBytes             = 128 * 1024 * 1024
	MaxOfferLifetimeSecs    = 24 * 60 * 60
	NegotiationTimeoutMs    = 15_000
	OfferDomain             = "Cinnabar/experience/offer/v1\x00"
	AcceptDomain            = "Cinnabar/experience/accept/v1\x00"
	ManifestDomain          = "Cinnabar/experience/manifest/v1\x00"
	ManifestPath            = "manifest.signed.json"
	Carrier                 = "cinnabar:extensions/v1"
	MaxEnvelopeBytes        = 64 * 1024
	maxDepth                = 128
)
