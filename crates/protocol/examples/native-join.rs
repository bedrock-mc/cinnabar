//! Runs the application's actual native login/spawn/play path without a renderer.

use protocol::{
    ClientRandomId, ClientSkin, ClientSkinGeometry, LoginSequence, LoginSettings,
    NativeConnectionTarget, NativeTransport, SessionLogin,
};
use std::{error::Error, io, io::Write, time::Duration};

/// Joins an explicitly supplied signed-out test server and receives its world stream.
/// Local HTTP assumes the caller owns/admitted that loopback process; address syntax grants no account authority.
#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args().skip(1);
    let transport = match args.next().as_deref() {
        Some("raknet") => NativeTransport::RakNet,
        Some("nethernet-lan") => NativeTransport::NetherNetLan,
        Some("local-nethernet-http") => NativeTransport::LocalNetherNetHttp,
        _ => {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: native-join <raknet|nethernet-lan|local-nethernet-http> HOST:PORT",
            )
            .into());
        }
    };
    let address = args
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "missing server address"))?;
    if args.next().is_some() {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "unexpected argument").into());
    }
    // Original solid-color skin and cuboid geometry, independent of game assets.
    let skin = ClientSkin {
        id: "cinnabar.native.example".into(), rgba8: [0x80, 0x80, 0x80, 0xff].repeat(64 * 64),
        width: 64, height: 64, arm_size: "wide".into(), cape: None,
        geometry: Some(ClientSkinGeometry {
            resource_patch: r#"{"geometry":{"default":"geometry.cinnabar.example"}}"#.into(),
            geometry_data: r#"{"format_version":"1.12.0","minecraft:geometry":[{"description":{"identifier":"geometry.cinnabar.example","texture_width":64,"texture_height":64},"bones":[{"name":"root","pivot":[0,0,0],"cubes":[{"origin":[-4,0,-2],"size":[8,24,4],"uv":[0,0]}]}]}]}"#.into(),
            engine_version: "1.21.0".into(),
        }),
    };
    let settings = LoginSettings {
        view_distance_chunks: 4,
        ..LoginSettings::default()
    };
    let (mut session, packs) = LoginSequence::connect_native(
        &NativeConnectionTarget::new(address, transport),
        SessionLogin {
            display_name: "NativeExample",
            client_random_id: ClientRandomId::new(1).expect("nonzero constant"),
            cache: None,
            skin: Some(skin),
            settings: &settings,
            tap: None,
            #[cfg(not(target_arch = "wasm32"))]
            relay: None,
            target: None,
        },
        |packs, _game| packs.len(),
    )
    .await?;
    writeln!(io::stdout().lock(), "native world spawned; packs={packs}")?;
    session.finish_loading().await?;
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            if matches!(
                session.recv_world_event(0).await?,
                protocol::WorldEvent::LevelChunk(_)
            ) {
                break Ok::<(), protocol::ProtocolError>(());
            }
        }
    })
    .await??;
    writeln!(io::stdout().lock(), "native terrain received")?;
    session.close().await?;
    Ok(())
}
