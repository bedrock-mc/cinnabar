//! Golden documents for the Go server half, from fixed seeds.
//!
//! `marker.json` and every `*_payload.json`, `*_signed.json`, `*_message.json`, `envelope_*.json`
//! and `channel_*.json` file holds the exact bytes the client produces or accepts: compact
//! serde_json with no trailing newline. `*_payload.json` is the signed byte string,
//! `*_signed.json` the SignedDocument over it and `*_message.json` the control message carried in
//! a ScriptMessage. `test_seeds.json`, `constants.json`, `enums.json` and `digests.json` are
//! pretty-printed facts about them.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use anyhow::{Context, Result};
use serde::Serialize;
use server_experience::{
    bundle::MANIFEST_PATH,
    crypto::{self, ACCEPT_DOMAIN, MANIFEST_DOMAIN, OFFER_DOMAIN},
    manifest::{Offer, PackageOffer, Permission, Scope, implemented_permissions},
    negotiation::{Accept, Hello, Marker},
    policy,
    session::Control,
    wire::{Channel, Direction, Envelope, Field, Scalar},
};

use crate::{
    bundle::{self, Source},
    keys,
};

/// Signs the offer and every accept.
const SERVER_SEED: [u8; 32] = pattern(0x00);
/// Signs the manifest.
const PUBLISHER_SEED: [u8; 32] = pattern(0x20);
const PACKAGE: &str = "benergistics";
const AUDIENCE: &str = "127.0.0.1:19132";
const ORIGIN: &str = "https://cxb.example";
const REVISION: u64 = 7;
const EXPIRES_UNIX: u64 = 1_800_000_000;
const WORLD_EPOCH: u64 = 3;
/// Above `i32::MAX`, so a narrower integer anywhere shows up as a mismatch.
const COUNT: i64 = 3_000_000_000;
/// The smallest valid component: the preamble alone.
const COMPONENT: &[u8] = b"\0asm\x0d\0\x01\0";
/// Characters JSON encoders disagree on: HTML, controls, DEL, line separators, BOM, astral.
const TEXT: &str = "quote \" backslash \\ slash / html <b>&amp; nul \0 bell \x07 \
    backspace \x08 tab \t lf \n formfeed \x0c cr \r unit \x1f del \x7f latin \u{e9} \
    line \u{2028} paragraph \u{2029} bom \u{feff} replacement \u{fffd} astral \u{1f600}";

/// A distinct, non-uniform 32-byte value.
const fn pattern(start: u8) -> [u8; 32] {
    let mut out = [0; 32];
    let mut index = 0;
    while index < 32 {
        out[index] = start.wrapping_add(index as u8);
        index += 1;
    }
    out
}

pub struct Fixtures {
    /// `(file name, exact bytes)` for every checked-in fixture.
    pub files: Vec<(&'static str, Vec<u8>)>,
    /// The `.cxb` the offer names. Bundles are never checked in.
    pub bundle: Vec<u8>,
}

#[derive(Serialize)]
struct Seed {
    seed: String,
    public_key: String,
}

#[derive(Serialize)]
struct Seeds {
    server: Seed,
    publisher: Seed,
}

#[derive(Serialize)]
struct Constants {
    carrier: &'static str,
    marker_path: &'static str,
    manifest_path: &'static str,
    offer_domain: &'static str,
    accept_domain: &'static str,
    manifest_domain: &'static str,
    wire_version: u16,
    api_version: u16,
    initial_bundle_generation: u64,
    max_marker_bytes: usize,
    max_payload_bytes: usize,
    max_envelope_bytes: usize,
    max_messages_per_second: u64,
    max_bytes_per_second: u64,
    max_offer_lifetime_secs: u64,
    negotiation_timeout_ms: u64,
    max_bundles: usize,
    max_bundle_bytes: usize,
    max_expanded_bytes: u64,
    max_channels: usize,
    max_channel_fields: usize,
    max_identifier_bytes: usize,
    max_fallback_bytes: usize,
    max_url_bytes: usize,
    max_origins: usize,
    max_guest_memory: u64,
    max_session_memory: u64,
    max_gpu_bytes: u64,
}

#[derive(Serialize)]
struct Enums {
    /// In canonical set order, which is declaration order, not alphabetical.
    permissions: BTreeSet<Permission>,
    /// What a developer client offers in Hello and grants in Ready.
    implemented_permissions: BTreeSet<Permission>,
    directions: Vec<Direction>,
}

/// Lists every variant of a fieldless enum; adding one fails to compile until it is listed.
macro_rules! all_variants {
    ($ty:ident: $($variant:ident),+ $(,)?) => {{
        let _exhaustive = |value: $ty| match value {
            $($ty::$variant)|+ => (),
        };
        [$($ty::$variant),+]
    }};
}

/// Builds every fixture in memory.
pub fn generate() -> Result<Fixtures> {
    let server = keys::pair(&SERVER_SEED)?;
    let publisher = keys::pair(&PUBLISHER_SEED)?;
    let built = bundle::build(source(), COMPONENT, &[], &publisher)?;
    let offer = Offer {
        version: policy::WIRE_VERSION,
        audience: AUDIENCE.to_owned(),
        server_key: keys::public_key(&server),
        revision: REVISION,
        expires_unix: EXPIRES_UNIX,
        scope: Scope {
            permissions: implemented_permissions(),
            origins: BTreeSet::from([ORIGIN.to_owned()]),
            memory_bytes: policy::MAX_GUEST_MEMORY,
            gpu_bytes: 0,
        },
        packages: vec![PackageOffer {
            id: PACKAGE.to_owned(),
            publisher_key: keys::public_key(&publisher),
            digest: crypto::digest(&built.bytes),
            bytes: built.bytes.len() as u64,
            url: format!("{ORIGIN}/{PACKAGE}.cxb"),
        }],
        fallback: "Without the client part, chat still says \"ME Controller interactions: N\" \
            <after each right-click> & nothing else changes"
            .to_owned(),
        carrier: protocol::EXPERIENCE_CHANNEL.to_owned(),
    };
    let offer_document = crypto::sign(&offer, OFFER_DOMAIN, &server)?;
    let offer_payload = crypto::unhex(&offer_document.payload)?;
    let offer_digest = crypto::digest(&offer_payload);
    let hello = Hello {
        version: policy::WIRE_VERSION,
        api: policy::API_VERSION,
        capabilities: implemented_permissions(),
        offer_digest: offer_digest.clone(),
        client_challenge: crypto::hex(&pattern(0x40)),
        connection: crypto::hex(&pattern(0x60)),
        subclient: 0,
    };
    let session = crypto::hex(&pattern(0xa0));
    let accept = Accept {
        hello: hello.clone(),
        server_challenge: crypto::hex(&pattern(0x80)),
        session: session.clone(),
        audience: offer.audience.clone(),
        offer_digest,
        revision: offer.revision,
        expires_unix: offer.expires_unix - 1800,
    };
    let accept_document = crypto::sign(&accept, ACCEPT_DOMAIN, &server)?;
    let accept_payload = crypto::unhex(&accept_document.payload)?;
    let manifest_payload = crypto::unhex(&built.manifest.payload)?;
    let ready = Control::Ready {
        session: session.clone(),
        packages: vec![offer.packages[0].digest.clone()],
        generation: policy::INITIAL_BUNDLE_GENERATION,
        permissions: BTreeMap::from([(PACKAGE.to_owned(), implemented_permissions())]),
        world_epoch: WORLD_EPOCH,
    };
    let envelope = |channel: &str, schema: u16, sequence: u64, payload: Vec<Scalar>| Envelope {
        version: policy::WIRE_VERSION,
        session: session.clone(),
        connection: hello.connection.clone(),
        subclient: hello.subclient,
        bundle: PACKAGE.to_owned(),
        generation: policy::INITIAL_BUNDLE_GENERATION,
        channel: format!("{PACKAGE}.{channel}"),
        schema,
        sequence,
        world_epoch: WORLD_EPOCH,
        payload,
    };
    let all_fields = Channel {
        id: format!("{PACKAGE}.all_fields"),
        schema: 2,
        direction: Direction::ToClient,
        fields: vec![
            Field::Bool,
            Field::Integer {
                min: i64::MIN,
                max: i64::MAX,
            },
            // 2^53 + 1 is not a float64, so a decoder that goes through floats fails.
            Field::Integer {
                min: -1,
                max: 9_007_199_254_740_993,
            },
            Field::Text {
                max_bytes: u16::MAX,
            },
            Field::Choice { variants: u16::MAX },
        ],
    };
    let all_scalars = envelope(
        "all_fields",
        all_fields.schema,
        2,
        vec![
            Scalar::Bool(true),
            Scalar::Integer(i64::MIN),
            Scalar::Integer(9_007_199_254_740_993),
            Scalar::Text(TEXT.to_owned()),
            Scalar::Choice(u16::MAX - 1),
        ],
    );
    let to_client = envelope("controller", 1, 1, vec![Scalar::Integer(COUNT)]);
    let to_server = envelope("ack", 1, 1, vec![Scalar::Integer(COUNT)]);
    let files = vec![
        ("test_seeds.json", pretty(&seeds(&server, &publisher))?),
        ("constants.json", pretty(&constants()?)?),
        ("enums.json", pretty(&enums())?),
        (
            "digests.json",
            pretty(&BTreeMap::from([
                ("offer_payload.json", crypto::digest(&offer_payload)),
                ("accept_payload.json", crypto::digest(&accept_payload)),
                ("manifest_payload.json", crypto::digest(&manifest_payload)),
            ]))?,
        ),
        ("offer_payload.json", offer_payload),
        ("offer_signed.json", serde_json::to_vec(&offer_document)?),
        (
            "marker.json",
            serde_json::to_vec(&Marker {
                server_key: offer.server_key,
                offer: offer_document,
            })?,
        ),
        ("hello_payload.json", serde_json::to_vec(&hello)?),
        (
            "hello_message.json",
            serde_json::to_vec(&Control::Hello(hello))?,
        ),
        ("accept_payload.json", accept_payload),
        ("accept_signed.json", serde_json::to_vec(&accept_document)?),
        (
            "accept_message.json",
            serde_json::to_vec(&Control::Accept(accept_document))?,
        ),
        ("ready_message.json", serde_json::to_vec(&ready)?),
        ("envelope_to_client.json", serde_json::to_vec(&to_client)?),
        ("envelope_to_server.json", serde_json::to_vec(&to_server)?),
        (
            "channel_all_field_types.json",
            serde_json::to_vec(&all_fields)?,
        ),
        (
            "envelope_all_scalar_types.json",
            serde_json::to_vec(&all_scalars)?,
        ),
        ("manifest_payload.json", manifest_payload),
        ("manifest_signed.json", serde_json::to_vec(&built.manifest)?),
    ];
    Ok(Fixtures {
        files,
        bundle: built.bytes,
    })
}

/// Writes every fixture into `dir`, replacing older copies.
pub fn write(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    for (name, bytes) in generate()?.files {
        let path = dir.join(name);
        std::fs::write(&path, bytes).with_context(|| format!("writing {}", path.display()))?;
    }
    Ok(())
}

/// The M0 client part: one counter to the client, its acknowledgement back.
fn source() -> Source {
    let counter = |name: &str, direction| Channel {
        id: format!("{PACKAGE}.{name}"),
        schema: 1,
        direction,
        fields: vec![Field::Integer {
            min: 0,
            max: u32::MAX.into(),
        }],
    };
    Source {
        id: PACKAGE.to_owned(),
        package_version: "0.1.0".to_owned(),
        permissions: implemented_permissions(),
        channels: vec![
            counter("controller", Direction::ToClient),
            counter("ack", Direction::ToServer),
        ],
        actions: BTreeSet::new(),
    }
}

fn seeds(server: &crypto::Ed25519KeyPair, publisher: &crypto::Ed25519KeyPair) -> Seeds {
    Seeds {
        server: Seed {
            seed: crypto::hex(&SERVER_SEED),
            public_key: keys::public_key(server),
        },
        publisher: Seed {
            seed: crypto::hex(&PUBLISHER_SEED),
            public_key: keys::public_key(publisher),
        },
    }
}

fn constants() -> Result<Constants> {
    let domain = |bytes: &'static [u8]| std::str::from_utf8(bytes);
    Ok(Constants {
        carrier: protocol::EXPERIENCE_CHANNEL,
        marker_path: policy::MARKER_PATH,
        manifest_path: MANIFEST_PATH,
        offer_domain: domain(OFFER_DOMAIN)?,
        accept_domain: domain(ACCEPT_DOMAIN)?,
        manifest_domain: domain(MANIFEST_DOMAIN)?,
        wire_version: policy::WIRE_VERSION,
        api_version: policy::API_VERSION,
        initial_bundle_generation: policy::INITIAL_BUNDLE_GENERATION,
        max_marker_bytes: policy::MAX_MARKER_BYTES,
        max_payload_bytes: policy::MAX_PAYLOAD_BYTES,
        max_envelope_bytes: protocol::MAX_EXPERIENCE_ENVELOPE_BYTES,
        max_messages_per_second: policy::MAX_MESSAGES_PER_SECOND,
        max_bytes_per_second: policy::MAX_BYTES_PER_SECOND,
        max_offer_lifetime_secs: policy::MAX_OFFER_LIFETIME_SECS,
        negotiation_timeout_ms: policy::NEGOTIATION_TIMEOUT_MS,
        max_bundles: policy::MAX_BUNDLES,
        max_bundle_bytes: policy::MAX_BUNDLE_BYTES,
        max_expanded_bytes: policy::MAX_EXPANDED_BYTES,
        max_channels: policy::MAX_CHANNELS,
        max_channel_fields: policy::MAX_CHANNEL_FIELDS,
        max_identifier_bytes: policy::MAX_IDENTIFIER_BYTES,
        max_fallback_bytes: policy::MAX_FALLBACK_BYTES,
        max_url_bytes: policy::MAX_URL_BYTES,
        max_origins: policy::MAX_ORIGINS,
        max_guest_memory: policy::MAX_GUEST_MEMORY,
        max_session_memory: policy::MAX_SESSION_MEMORY,
        max_gpu_bytes: policy::MAX_GPU_BYTES,
    })
}

fn enums() -> Enums {
    Enums {
        permissions: BTreeSet::from(all_variants!(
            Permission: Ui,
            ModalUi,
            Input,
            Messaging,
            Scene,
            Media,
        )),
        implemented_permissions: implemented_permissions(),
        directions: all_variants!(Direction: ToClient, ToServer).to_vec(),
    }
}

/// Descriptive files are pretty-printed with a final newline.
fn pretty(value: &impl Serialize) -> Result<Vec<u8>> {
    let mut bytes = serde_json::to_vec_pretty(value)?;
    bytes.push(b'\n');
    Ok(bytes)
}
