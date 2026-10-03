use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

use cinnabar_cxb::fixtures;
use serde::de::DeserializeOwned;
use server_experience::{
    bundle::VerifiedBundle,
    crypto::{self, KeyPair, SignedDocument},
    manifest::{Manifest, Offer, implemented_permissions},
    negotiation::{Accept, Grant, Marker, VerifiedOffer},
    policy::{
        INITIAL_BUNDLE_GENERATION, MAX_EXPANDED_BYTES, MAX_MARKER_BYTES, MAX_PAYLOAD_BYTES,
        WIRE_VERSION,
    },
    runtime::Capabilities,
    session::Control,
    wire::{Channel, Direction, Envelope, Ingress},
};

const REGENERATE: &str = "regenerate with `cargo run -p cinnabar-cxb --locked -- write-fixtures tools/localserver/extension/testdata`";
/// The server half writes `go/`: Go's own marker, Accept and envelope for the client's verifiers.
const REGENERATE_GO: &str = "regenerate with `go test ./extension -run TestGoFixturesAreCurrent -update-go-fixtures` in tools/localserver";

fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../localserver/extension/testdata")
}

fn read(name: &str) -> Vec<u8> {
    let path = fixture_dir().join(name);
    let regenerate = if name.starts_with("go/") {
        REGENERATE_GO
    } else {
        REGENERATE
    };
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}; {regenerate}", path.display()))
}

/// Decodes a checked-in document and proves it is already in the client's canonical form.
fn canonical<T: DeserializeOwned + serde::Serialize>(name: &str) -> T {
    let bytes = read(name);
    let value: T = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(
        serde_json::to_vec(&value).unwrap(),
        bytes,
        "{name} is not canonical"
    );
    value
}

#[test]
fn checked_in_fixtures_match() {
    let generated = fixtures::generate().unwrap();
    for (name, bytes) in &generated.files {
        assert!(read(name) == *bytes, "{name} is stale; {REGENERATE}");
    }
    // Subdirectories are free for fixtures other languages produce.
    let mut checked_in: Vec<String> = std::fs::read_dir(fixture_dir())
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_type().unwrap().is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    checked_in.sort();
    let mut expected: Vec<String> = generated
        .files
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect();
    expected.sort();
    assert_eq!(
        checked_in, expected,
        "unexpected fixture files; {REGENERATE}"
    );
}

#[test]
fn fixtures_pass_the_client_verifiers() {
    let seeds: serde_json::Value = serde_json::from_slice(&read("test_seeds.json")).unwrap();
    let digests: BTreeMap<String, String> = serde_json::from_slice(&read("digests.json")).unwrap();
    for role in ["server", "publisher"] {
        let seed = crypto::fixed_hex::<32>(seeds[role]["seed"].as_str().unwrap()).unwrap();
        let key = crypto::Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
        assert_eq!(
            seeds[role]["public_key"],
            crypto::hex(key.public_key().as_ref())
        );
    }

    // Marker → offer, exactly as the client admits it from the resource pack.
    let marker_bytes = read("marker.json");
    let marker: Marker = canonical("marker.json");
    let offer: Offer = canonical("offer_payload.json");
    // One clock for the whole handshake: the accept arrives seconds after the offer is read.
    let now = offer.expires_unix - 3600;
    let verified = VerifiedOffer::read(&marker_bytes, &offer.audience, now).unwrap();
    assert_eq!(verified.offer, offer);
    assert_eq!(verified.digest, digests["offer_payload.json"]);
    assert_eq!(offer.server_key, seeds["server"]["public_key"]);
    assert_eq!(
        serde_json::to_vec(&marker.offer).unwrap(),
        read("offer_signed.json")
    );
    let package = &offer.packages[0];
    assert_eq!(package.publisher_key, seeds["publisher"]["public_key"]);

    // Hello → Accept, checked by the client's signature and binding rules.
    let Control::Hello(hello) = canonical("hello_message.json") else {
        panic!("hello_message.json is not a hello");
    };
    assert_eq!(
        serde_json::to_vec(&hello).unwrap(),
        read("hello_payload.json")
    );
    assert_eq!(hello.offer_digest, verified.digest);
    let Control::Accept(document) = canonical("accept_message.json") else {
        panic!("accept_message.json is not an accept");
    };
    assert_eq!(
        serde_json::to_vec(&document).unwrap(),
        read("accept_signed.json")
    );
    let (accept, digest): (Accept, _) = document
        .verify(&offer.server_key, crypto::ACCEPT_DOMAIN, MAX_PAYLOAD_BYTES)
        .unwrap();
    assert!(
        document
            .verify::<Accept>(&offer.server_key, crypto::OFFER_DOMAIN, MAX_PAYLOAD_BYTES)
            .is_err()
    );
    assert_eq!(digest, digests["accept_payload.json"]);
    assert_eq!(
        serde_json::to_vec(&accept).unwrap(),
        read("accept_payload.json")
    );
    assert_eq!(accept.hello, hello);
    assert_eq!(accept.audience, offer.audience);
    assert_eq!(accept.offer_digest, verified.digest);
    assert_eq!(accept.revision, offer.revision);
    assert!(accept.expires_unix > now && accept.expires_unix <= offer.expires_unix);

    // Manifest and bundle, through the client's own bundle verifier.
    let manifest_document: SignedDocument = canonical("manifest_signed.json");
    let (manifest, digest): (Manifest, _) = manifest_document
        .verify(
            &package.publisher_key,
            crypto::MANIFEST_DOMAIN,
            MAX_MARKER_BYTES / 2,
        )
        .unwrap();
    assert_eq!(digest, digests["manifest_payload.json"]);
    assert_eq!(
        serde_json::to_vec(&manifest).unwrap(),
        read("manifest_payload.json")
    );
    let generated = fixtures::generate().unwrap();
    let bundle =
        VerifiedBundle::read(&generated.bundle, package, &offer.scope, MAX_EXPANDED_BYTES).unwrap();
    assert_eq!(
        serde_json::to_vec(&bundle.manifest).unwrap(),
        read("manifest_payload.json")
    );

    // Ready, as the client's live runtime reports it.
    let Control::Ready {
        session,
        packages,
        generation,
        permissions,
        world_epoch,
    } = canonical("ready_message.json")
    else {
        panic!("ready_message.json is not a ready");
    };
    assert_eq!(session, accept.session);
    assert_eq!(packages, std::slice::from_ref(&package.digest));
    assert_eq!(generation, INITIAL_BUNDLE_GENERATION);
    let granted: BTreeSet<_> = manifest
        .permissions
        .intersection(&implemented_permissions())
        .copied()
        .collect();
    assert_eq!(permissions, BTreeMap::from([(package.id.clone(), granted)]));

    // Server → client envelope, through the client's ingress for this grant.
    let grant = Grant {
        offer: verified,
        session: accept.session.clone(),
        connection: hello.connection.clone(),
        subclient: hello.subclient,
        expires_unix: accept.expires_unix,
    };
    let capabilities = Capabilities {
        scope: offer.scope.clone(),
        assets: BTreeSet::new(),
        channels: manifest.channels.clone(),
        actions: manifest.actions.clone(),
    };
    let to_client: Envelope = canonical("envelope_to_client.json");
    let mut ingress = Ingress::new(0);
    ingress
        .receive(&read("envelope_to_client.json"), 0, 1, &grant, |id| {
            (id == package.id).then_some(&capabilities)
        })
        .unwrap();
    assert_eq!(ingress.skipped, 0);
    let delivered = ingress.pop(1, world_epoch).unwrap();
    assert_eq!(delivered.payload, to_client.payload);

    // Client → server envelope, as the client's live runtime would send it.
    let to_server: Envelope = canonical("envelope_to_server.json");
    assert_eq!(
        (
            to_server.version,
            &to_server.session,
            &to_server.connection,
            to_server.subclient
        ),
        (
            WIRE_VERSION,
            &grant.session,
            &grant.connection,
            grant.subclient
        )
    );
    assert_eq!(to_server.bundle, package.id);
    assert_eq!(to_server.generation, INITIAL_BUNDLE_GENERATION);
    assert_eq!(to_server.world_epoch, world_epoch);
    let channel = manifest
        .channels
        .iter()
        .find(|c| c.id == to_server.channel && c.schema == to_server.schema)
        .unwrap();
    channel
        .validate(&to_server.payload, Direction::ToServer)
        .unwrap();

    // Every scalar and field type, including text that JSON encoders escape differently.
    let channel: Channel = canonical("channel_all_field_types.json");
    let scalars: Envelope = canonical("envelope_all_scalar_types.json");
    assert_eq!(
        (&scalars.channel, scalars.schema),
        (&channel.id, channel.schema)
    );
    channel
        .validate(&scalars.payload, channel.direction)
        .unwrap();
}

/// What the Go server half itself produced passes the client's verifiers: its marker, the real
/// `.cxb` against the package Go offers for it, its Accept for a developer client's Hello under
/// the binding rules of `Pending::accept`, and its first envelope after Ready through the client's
/// ingress. `Pending` makes its Hello's nonces itself, so the Accept is checked by the same
/// verification and rules rather than through a `Pending` holding Go's Hello.
#[test]
fn go_server_half_passes_the_client_verifiers() {
    let marker: Marker = canonical("go/marker.json");
    let (offer, _): (Offer, _) = marker
        .offer
        .verify(
            &marker.server_key,
            crypto::OFFER_DOMAIN,
            MAX_MARKER_BYTES / 2,
        )
        .unwrap();
    // One clock for the whole handshake, within the offer's lifetime.
    let now = offer.expires_unix - 3600;
    let verified = VerifiedOffer::read(&read("go/marker.json"), &offer.audience, now).unwrap();
    let package = verified.offer.packages[0].clone();
    let generated = fixtures::generate().unwrap();
    let bundle = VerifiedBundle::read(
        &generated.bundle,
        &package,
        &verified.offer.scope,
        MAX_EXPANDED_BYTES,
    )
    .unwrap();

    let Control::Hello(hello) = canonical("go/hello_message.json") else {
        panic!("go/hello_message.json is not a hello");
    };
    assert_eq!(hello.offer_digest, verified.digest);
    let Control::Accept(document) = canonical("go/accept_message.json") else {
        panic!("go/accept_message.json is not an accept");
    };
    let (accept, _): (Accept, _) = document
        .verify(
            &verified.offer.server_key,
            crypto::ACCEPT_DOMAIN,
            MAX_PAYLOAD_BYTES,
        )
        .unwrap();
    assert_eq!(accept.hello, hello);
    assert_eq!(accept.audience, verified.offer.audience);
    assert_eq!(accept.offer_digest, verified.digest);
    assert_eq!(accept.revision, verified.offer.revision);
    assert!(accept.expires_unix > now && accept.expires_unix <= verified.offer.expires_unix);
    crypto::fixed_hex::<32>(&accept.server_challenge).unwrap();
    crypto::fixed_hex::<32>(&accept.session).unwrap();

    let Control::Ready {
        session,
        permissions,
        world_epoch,
        ..
    } = canonical("go/ready_message.json")
    else {
        panic!("go/ready_message.json is not a ready");
    };
    assert_eq!(session, accept.session);
    let mut scope = verified.offer.scope.clone();
    scope.permissions = permissions[&package.id].clone();
    let capabilities = Capabilities {
        scope,
        assets: BTreeSet::new(),
        channels: bundle.manifest.channels.clone(),
        actions: bundle.manifest.actions.clone(),
    };
    let grant = Grant {
        offer: verified,
        session: accept.session,
        connection: hello.connection,
        subclient: hello.subclient,
        expires_unix: accept.expires_unix,
    };
    let mut ingress = Ingress::new(0);
    ingress
        .receive(&read("go/envelope_to_client.json"), 0, 1, &grant, |id| {
            (id == package.id).then_some(&capabilities)
        })
        .unwrap();
    assert_eq!(ingress.skipped, 0);
    let envelope: Envelope = canonical("go/envelope_to_client.json");
    let delivered = ingress.pop(1, world_epoch).unwrap();
    assert_eq!(delivered.payload, envelope.payload);
}

#[test]
fn bundle_metadata_is_independent_of_the_build_host() {
    let generated = fixtures::generate().unwrap();
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(&generated.bundle)).unwrap();
    for index in 0..archive.len() {
        let file = archive.by_index(index).unwrap();
        let header = usize::try_from(file.central_header_start()).unwrap();
        // The creator OS occupies the high byte of ZIP's version-made-by field.
        assert_eq!(generated.bundle[header + 5], 0, "creator OS must be DOS");
        assert_eq!(file.compression(), zip::CompressionMethod::Stored);
        assert_eq!(file.last_modified(), Some(zip::DateTime::default()));
    }
    let offer: Offer = canonical("offer_payload.json");
    assert_eq!(crypto::digest(&generated.bundle), offer.packages[0].digest);
}
