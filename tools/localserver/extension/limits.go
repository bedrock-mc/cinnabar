package extension

// The protocol constants Go shares with the client. Each equals its Rust constant, which
// TestConstantsMatchRust checks against testdata/constants.json.
const (
	// Carrier is the ScriptMessage identifier: protocol::EXPERIENCE_CHANNEL.
	Carrier = "cinnabar:extensions/v1"
	// MarkerPath is where the admitted resource pack holds the marker: MARKER_PATH.
	MarkerPath = "cinnabar/extension-offer.json"
	// ManifestPath is the signed manifest entry of a .cxb: bundle::MANIFEST_PATH.
	ManifestPath = "manifest.signed.json"

	// OfferDomain prefixes the signed bytes of a deployment offer: OFFER_DOMAIN.
	OfferDomain = "Cinnabar/experience/offer/v1\x00"
	// AcceptDomain prefixes the signed bytes of a live acceptance: ACCEPT_DOMAIN.
	AcceptDomain = "Cinnabar/experience/accept/v1\x00"
	// ManifestDomain prefixes the signed bytes of a bundle manifest: MANIFEST_DOMAIN.
	ManifestDomain = "Cinnabar/experience/manifest/v1\x00"

	// WireVersion is the version of offers, Hello and envelopes: WIRE_VERSION.
	WireVersion = 1
	// APIVersion is the client component API in Hello and manifests: API_VERSION.
	APIVersion = 1
	// InitialBundleGeneration is the only bundle generation: INITIAL_BUNDLE_GENERATION.
	InitialBundleGeneration = 1

	// MaxMarkerBytes bounds the marker file: MAX_MARKER_BYTES.
	MaxMarkerBytes = 64 << 10
	// MaxPayloadBytes bounds an encoded record and an Accept payload: MAX_PAYLOAD_BYTES.
	MaxPayloadBytes = 16 << 10
	// MaxEnvelopeBytes bounds one carrier message: protocol::MAX_EXPERIENCE_ENVELOPE_BYTES.
	MaxEnvelopeBytes = 64 << 10
	// MaxMessagesPerSecond is the message rate of one direction: MAX_MESSAGES_PER_SECOND.
	MaxMessagesPerSecond = 64
	// MaxBytesPerSecond is the byte rate of one direction: MAX_BYTES_PER_SECOND.
	MaxBytesPerSecond = 64 << 10
	// MaxOfferLifetimeSecs bounds how far ahead an offer may expire: MAX_OFFER_LIFETIME_SECS.
	MaxOfferLifetimeSecs = 24 * 60 * 60
	// NegotiationTimeoutMs is how long the client waits for Accept: NEGOTIATION_TIMEOUT_MS.
	NegotiationTimeoutMs = 15_000

	// MaxBundles bounds the packages of one offer: MAX_BUNDLES.
	MaxBundles = 4
	// MaxBundleBytes bounds one .cxb: MAX_BUNDLE_BYTES.
	MaxBundleBytes = 64 << 20
	// MaxExpandedBytes bounds the offered bundles together: MAX_EXPANDED_BYTES.
	MaxExpandedBytes = 256 << 20
	// MaxChannels bounds the channels of one bundle: MAX_CHANNELS.
	MaxChannels = 64
	// MaxChannelFields bounds the fields of one channel: MAX_CHANNEL_FIELDS.
	MaxChannelFields = 64
	// MaxIdentifierBytes bounds a package, channel or action identifier: MAX_IDENTIFIER_BYTES.
	MaxIdentifierBytes = 96
	// MaxFallbackBytes bounds an offer's fallback text: MAX_FALLBACK_BYTES.
	MaxFallbackBytes = 512
	// MaxURLBytes bounds a package URL: MAX_URL_BYTES.
	MaxURLBytes = 2048
	// MaxOrigins bounds the origins of an offer's scope: MAX_ORIGINS.
	MaxOrigins = 8
	// MaxGuestMemory bounds one guest's memory: MAX_GUEST_MEMORY.
	MaxGuestMemory = 32 << 20
	// MaxSessionMemory bounds an offer scope's memory_bytes: MAX_SESSION_MEMORY.
	MaxSessionMemory = 64 << 20
	// MaxGPUBytes bounds an offer scope's gpu_bytes: MAX_GPU_BYTES.
	MaxGPUBytes = 128 << 20
)
