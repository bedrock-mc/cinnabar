//! Actual private TCP owners, duplex control flow and retained partial framing.

use super::{contract::*, *};
use std::{
    io,
    time::{Duration, Instant},
};
use tokio::{
    io::AsyncWriteExt,
    net::{TcpListener, TcpStream},
};

/// Supplies only synthetic process records; no SDK member/Login/transport capability is fabricated.
fn snapshot() -> Snapshot {
    Snapshot {
        protocol_version: 1,
        max_players: MAX_PLAYERS,
        max_chunk_radius: 4,
        required: false,
        packs: Vec::new(),
    }
}

/// Writes generated private-process metadata through the actual retained writer.
async fn json(writer: &mut Writer, kind: Kind, record: &impl serde::Serialize) {
    writer
        .admit(kind, &serde_json::to_vec(record).unwrap())
        .unwrap();
    writer.flush().await.unwrap();
}

/// Connects as this fixture's actual child and completes the mutual capability exchange.
async fn child(bootstrap: &Bootstrap, peer: u64) -> (Reader, Writer) {
    let stream = TcpStream::connect(&bootstrap.address).await.unwrap();
    let (mut reader, mut writer) = split(stream);
    json(
        &mut writer,
        Kind::Hello,
        &Hello {
            version: VERSION,
            opening: bootstrap.opening.clone(),
            capability: bootstrap.capability.clone(),
            peer,
        },
    )
    .await;
    let frame = reader.recv().await.unwrap().unwrap();
    assert_eq!(frame.kind, Kind::Hello);
    let hello: Hello = serde_json::from_slice(&frame.body).unwrap();
    assert_eq!(hello.peer, peer);
    (reader, writer)
}

#[tokio::test]
async fn host_process_duplex_receipts_and_final_close_keep_the_same_payload_slot() {
    let deadline = Instant::now() + Duration::from_secs(5);
    let pending = PendingHostProcess::bind(
        &[7; CAPABILITY_BYTES as usize],
        &[8; OPENING_BYTES as usize],
    )
    .await
    .unwrap();
    let bootstrap = Bootstrap {
        address: pending.bootstrap().address.clone(),
        capability: pending.bootstrap().capability.clone(),
        opening: pending.bootstrap().opening.clone(),
    };
    let child = async {
        let (mut control, mut control_out) = child(&bootstrap, 0).await;
        json(&mut control_out, Kind::Snapshot, &snapshot()).await;
        let frame = control.recv().await.unwrap().unwrap();
        assert_eq!(frame.kind, Kind::Open);
        let open: Open = serde_json::from_slice(&frame.body).unwrap();
        assert_eq!(open.client_data.get(), r#"{ "opaque": 1e+2 }"#);
        let (mut peer, mut out) = child(&bootstrap, open.peer).await;
        json(
            &mut control_out,
            Kind::Opened,
            &PeerResult {
                peer: open.peer,
                code: 0,
            },
        )
        .await;
        let range = peer.recv().await.unwrap().unwrap();
        assert_eq!(range.kind, Kind::ReadPack);
        let range: Range = serde_json::from_slice(&range.body).unwrap();
        assert_eq!((range.archive, range.offset, range.length), (0, 3, 2));
        out.admit(Kind::PackBytes, b"ef").unwrap();
        out.flush().await.unwrap();
        assert_eq!(peer.recv().await.unwrap().unwrap().kind, Kind::Attach);
        out.admit(Kind::World, b"original-world").unwrap();
        out.flush().await.unwrap();
        assert_eq!(peer.recv().await.unwrap().unwrap().kind, Kind::Initialized);
        out.admit(Kind::Packet, b"world-output").unwrap();
        out.flush().await.unwrap();
        out.admit(Kind::Flush, &1_u64.to_be_bytes()).unwrap();
        out.flush().await.unwrap();
        assert_eq!(
            peer.recv().await.unwrap().unwrap().body,
            b"client-input"[..]
        );
        let flush = peer.recv().await.unwrap().unwrap();
        assert_eq!(flush.kind, Kind::Flush);
        assert_eq!(flush.body, 1_u64.to_be_bytes()[..]);
        out.admit(Kind::Flushed, &1_u64.to_be_bytes()).unwrap();
        out.flush().await.unwrap();
        let receipt = peer.recv().await.unwrap().unwrap();
        assert_eq!(receipt.kind, Kind::Flushed);
        assert_eq!(receipt.body, 1_u64.to_be_bytes()[..]);
        assert_eq!(peer.recv().await.unwrap().unwrap().kind, Kind::Close);
        out.admit(Kind::Closed, &[]).unwrap();
        out.flush().await.unwrap();
        assert_eq!(control.recv().await.unwrap().unwrap().kind, Kind::Close);
        control_out.admit(Kind::Closed, &[]).unwrap();
        control_out.flush().await.unwrap();
    };
    let parent = async {
        let mut process = pending.establish(deadline).await.unwrap();
        let data =
            serde_json::value::RawValue::from_string(r#"{ "opaque": 1e+2 }"#.into()).unwrap();
        let mut peer = process
            .prepare_open(
                Identity {
                    xuid: "1".into(),
                    uuid: "fixture".into(),
                    display_name: "fixture".into(),
                    playfab_id: String::new(),
                    playfab_title_id: String::new(),
                },
                data,
                deadline,
            )
            .await
            .unwrap()
            .begin_open()
            .unwrap()
            .finish()
            .await
            .unwrap();
        assert_eq!(peer.read_pack(0, 3, 2, deadline).await.unwrap(), b"ef"[..]);
        let attaching = peer.begin_attach(deadline).unwrap();
        let finishing = attaching.finish();
        tokio::pin!(finishing);
        assert!(futures::poll!(finishing.as_mut()).is_pending());
        let peer = finishing.await.unwrap();
        assert_eq!(peer.world(), b"original-world");
        let mut peer = peer
            .begin_initialized(true, 2, deadline)
            .unwrap()
            .finish()
            .await
            .unwrap();
        peer.queue_send(b"client-input", deadline).unwrap();
        assert_eq!(
            peer.flush_send().await.unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );
        assert_eq!(peer.recv().await.unwrap().unwrap(), b"world-output"[..]);
        peer.received(deadline).await.unwrap();
        peer.flush_send().await.unwrap();
        peer.flush_send().await.unwrap(); // A completed operation is not polled again.
        let mut closing = peer.begin_close(deadline);
        closing.finish_close().await.unwrap();
        closing.finish_close().await.unwrap();
        process.close(deadline).await.unwrap();
        process.close(deadline).await.unwrap();
    };
    tokio::time::timeout_at(deadline.into(), async {
        tokio::join!(parent, child);
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn host_process_reader_retains_the_original_partial_frame_after_cancel() {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let address = listener.local_addr().unwrap();
    let (connection, accepted) = tokio::join!(TcpStream::connect(address), listener.accept());
    let mut connection = connection.unwrap();
    let (mut reader, _) = split(accepted.unwrap().0);
    let body = b"original";
    let length = u32::try_from(body.len() + 1).unwrap().to_be_bytes();
    connection.write_all(&length[..2]).await.unwrap();
    {
        let receive = reader.recv();
        tokio::pin!(receive);
        futures::future::poll_fn(|cx| {
            assert!(receive.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
    }
    connection.write_all(&length[2..]).await.unwrap();
    connection.write_all(&[Kind::Packet as u8]).await.unwrap();
    connection.write_all(&body[..2]).await.unwrap();
    {
        let receive = reader.recv();
        tokio::pin!(receive);
        futures::future::poll_fn(|cx| {
            assert!(receive.as_mut().poll(cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
    }
    connection.write_all(&body[2..]).await.unwrap();
    let frame = reader.recv().await.unwrap().unwrap();
    assert_eq!(frame.kind, Kind::Packet);
    assert_eq!(frame.body, body[..]);
}
