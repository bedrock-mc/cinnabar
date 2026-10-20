//! Actual backend delivery/receipt state survives a contended directory ready-result postcheck.

use super::*;
use crate::native_account::Operation;
use crate::native_account::hosted_world::serve::play::output::Output;
use bridge::host_world::{
    ActiveBackendPeer, HostProcess, PendingHostProcess, Reader, Writer,
    contract::{
        Bootstrap, CAPABILITY_BYTES, Hello, Identity, Kind, MAX_PLAYERS, OPENING_BYTES, PeerResult,
        Snapshot, VERSION,
    },
    split,
};
use std::{cell::RefCell, future::Future};
use tokio::net::TcpStream;

#[derive(Clone, Copy)]
enum Boundary {
    Delivered,
    Acknowledged,
}

#[tokio::test]
async fn delivered_backend_output_resumes_after_the_real_account_checkout() {
    exercise(Boundary::Delivered).await;
}

#[tokio::test]
async fn acknowledged_backend_output_is_not_replayed_after_the_real_account_checkout() {
    exercise(Boundary::Acknowledged).await;
}

/// Retains one actual completed effect while a real account checkout delays its authority postcheck.
async fn exercise(boundary: Boundary) {
    tokio::time::timeout(ACQUISITION_TIMEOUT, async {
        let fixture = NativeHostFixture::new();
        let namespace = tempfile::tempdir().unwrap();
        let config = minecraft::Config::desktop(
            protocol::GAME_VERSION,
            u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
            metadata::service_device_id("123").unwrap(),
        );
        let HostedAuthority {
            account: services,
            identity,
            mut peer,
        } = HostedAuthority::acquire_revocation(authentication_policy(), config).await;
        let store = AccountStore::new(namespace.path().join("synthetic-oauth.json"));
        let credentials = services.credentials();
        let expiry = oauth_expiry(credentials.expires_at()).unwrap();
        let checkpoint = services.checkpoint().unwrap();
        let pending = transaction("reserve external send fixture sign-in", || {
            store.try_begin_native_sign_in("host-external-send-generation", None)
        });
        transaction("publish external send fixture OAuth", || {
            pending.try_publish(
                credentials.access_token(),
                credentials.refresh_token(),
                &expiry,
            )
        });
        transaction("publish external send fixture checkpoint", || {
            pending.try_publish_checkpoint(checkpoint.as_bytes())
        });
        transaction("commit external send fixture sign-in", || {
            pending.try_commit(
                &AccountProfile::from_identity(identity.xuid(), identity.gamertag()).unwrap(),
            )
        });
        let account = Arc::new(NativeAccount::new(
            store,
            namespace.path().join("synthetic-device.json"),
            Some(identity.xuid().into()),
        ));
        install(&account, services, &identity);
        let deadline = Instant::now() + ACQUISITION_TIMEOUT;
        let mut host = fixture
            .manager
            .native_hosted_world(Arc::clone(&account), deadline)
            .unwrap();
        {
            let directory = host.directory.as_mut().unwrap();
            let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
            let HostedMembership {
                socket,
                subscription,
                binding,
                session,
            } = peer
                .host(
                    operation.minecraft().unwrap(),
                    &directory.reference,
                    directory.session_subscription,
                    directory.scope.as_ref().unwrap().rta_generation().unwrap(),
                    "456",
                )
                .await;
            directory.socket = Some(socket);
            directory.subscription = Some(subscription);
            directory.subscribed = true;
            directory.binding = Some(binding);
            directory.session = Some(session);
            directory.host_name = identity.gamertag().into();
            directory.approvals = Some(
                Approvals::new(identity.xuid().into(), directory.opening.max_players()).unwrap(),
            );
            directory.observe().unwrap();
            let approvals = directory.approvals.as_mut().unwrap();
            for owner in approvals.missing() {
                approvals.issue(&owner, &[7; NONCE_BYTES]).unwrap();
            }
            let revision = approvals.revision();
            directory.pending = Some(ApprovalWrite {
                revision,
                document: directory.properties().unwrap(),
            });
            peer.set_properties_bound(
                operation.minecraft().unwrap(),
                directory.session.as_mut().unwrap(),
                binding,
                &directory.pending.as_ref().unwrap().document,
            )
            .await;
            directory.observe().unwrap();
            directory.confirm_pending().unwrap();
            operation.persist().unwrap();
        }
        let nonce = host
            .directory
            .as_ref()
            .unwrap()
            .approvals
            .as_ref()
            .unwrap()
            .document()
            .unwrap()["456"]
            .as_str()
            .unwrap()
            .to_owned();
        let approval = host.check_approval("456", &nonce).unwrap();
        let SignedPair {
            mut wire,
            sender,
            mut client,
            mut listener,
            signaling,
        } = signed_pair(deadline).await;
        let Backend {
            mut process,
            mut backend,
            mut input,
            mut output,
            mut control,
            mut control_output,
        } = backend(&mut host, &approval, deadline).await;
        let first = raw_packet(1);
        let second = raw_packet(2);
        let mut obligation = Output::default();
        let reserved_account = RefCell::new(None);
        let mut postcheck_waited = false;
        let mut original_restored = true;
        let operation: io::Result<()> = async {
            for (index, packet) in [first.clone(), second.clone()].into_iter().enumerate() {
                output.admit(Kind::Packet, &packet)?;
                output.flush().await?;
                let sequence = u64::try_from(index + 1).unwrap();
                output.admit(Kind::Flush, &sequence.to_be_bytes())?;
                output.flush().await?;
                let delivered = host.peer_io(Some(deadline), async {
                    let packet = backend.recv().await?.ok_or_else(ended)?;
                    obligation.stage(&mut wire, packet, deadline)?;
                    if index == 0 && matches!(boundary, Boundary::Delivered) {
                        *reserved_account.borrow_mut() = Some(account.begin()?);
                    }
                    Ok(())
                });
                if index == 0 && matches!(boundary, Boundary::Delivered) {
                    let (delivered, waited) = finish_after_checkout(delivered, &reserved_account).await;
                    postcheck_waited = waited;
                    delivered?;
                    original_restored &= host.recheck_approval(&approval).is_ok();
                } else {
                    delivered.await?;
                }
                // This exact production owner resumes the already delivered packet, not backend.recv.
                obligation.forward(&mut host, &approval, &mut wire).await?;
                let acknowledged = host.peer_io(Some(deadline), async {
                    backend.received(deadline).await?;
                    obligation.acknowledged(&mut wire)?;
                    if index == 0 && matches!(boundary, Boundary::Acknowledged) {
                        *reserved_account.borrow_mut() = Some(account.begin()?);
                    }
                    Ok(())
                });
                if index == 0 && matches!(boundary, Boundary::Acknowledged) {
                    let (acknowledged, waited) = finish_after_checkout(acknowledged, &reserved_account).await;
                    postcheck_waited = waited;
                    acknowledged?;
                    original_restored &= host.recheck_approval(&approval).is_ok();
                } else {
                    acknowledged.await?;
                }
            }
            Ok(())
        }.await;
        drop(reserved_account.borrow_mut().take());

        // Observe the actual ordered stream through its marker even if an earlier operation failed.
        sender.send_before(Bytes::from_static(MARKER), deadline).await.unwrap();
        let mut packets = Vec::new();
        loop {
            let message = client.recv().await.unwrap().unwrap();
            if message.as_ref() == MARKER { break; }
            packets.extend(decode(message));
        }
        let (backend_closed, receipts) = if operation.is_ok() {
            let closing = async {
                let mut closing = backend.begin_close(deadline);
                let result = closing.finish_close().await;
                drop(closing);
                result.is_ok()
            };
            let child = async {
                let mut receipts = Vec::new();
                while let Some(frame) = input.recv().await.unwrap() {
                    if frame.kind == Kind::Flushed {
                        receipts.push(frame.body);
                    } else if frame.kind == Kind::Close {
                        output.admit(Kind::Closed, &[]).unwrap();
                        output.flush().await.unwrap();
                        break;
                    } else {
                        break;
                    }
                }
                receipts
            };
            tokio::join!(closing, child)
        } else {
            drop(backend);
            (false, Vec::new())
        };
        drop(input);
        drop(output);
        let (process_closed, control_closed) = tokio::join!(
            async {
                let result = process.close(deadline).await.is_ok();
                drop(process);
                result
            },
            async {
                let closed = matches!(control.recv().await.unwrap(), Some(frame) if frame.kind == Kind::Close);
                if closed {
                    control_output.admit(Kind::Closed, &[]).unwrap();
                    control_output.flush().await.unwrap();
                }
                closed
            },
        );
        drop(control);
        drop(control_output);
        let mut retired = wire.begin_close();
        let mut client = client.close();
        retired.finish_close(deadline).await.unwrap();
        let server_drained = matches!(retired.receive_raw(deadline).await,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof);
        let reliable_drained = drain(&mut client.reliable).await;
        let unreliable_drained = drain(&mut client.unreliable).await;
        drop(retired);
        drop(client);
        drop(sender);
        listener.close().await.unwrap();
        drop(listener);
        let signaling_joined = match Arc::try_unwrap(signaling) {
            Ok(signaling) => { signaling.shutdown().await; true }
            Err(signaling) => { drop(signaling); false }
        };

        // Complete this original finite signed service scenario after the positive transport controls.
        peer.revoke_member_ready(host.directory.as_mut().unwrap().socket.as_mut().unwrap()).await;
        host.directory.as_mut().unwrap().service_barrier(Some(deadline)).await.unwrap();
        let revoked = host.recheck_approval(&approval).is_err();
        {
            let directory = host.directory.as_mut().unwrap();
            let mut operation = Arc::clone(&account).acquire_verified().await.unwrap();
            peer.leave_bound(
                operation.minecraft().unwrap(),
                directory.session.as_mut().unwrap(),
                directory.binding.unwrap(),
            ).await;
            operation.persist().unwrap();
            peer.finish(directory.socket.as_mut().unwrap()).await;
        }
        host.close(deadline).await.unwrap();
        let cleanup_empty = fixture.manager.native_host_cleanup().unwrap().is_none();
        let world_running = !fixture.runner.last().stopped();
        assert!(operation.is_ok(), "actual retained backend output failed: {operation:?}");
        assert!(postcheck_waited && original_restored,
            "actual ready-result postcheck did not retain its result until the original account checkout ended");
        assert_eq!(packets, [first, second], "external output was lost, repeated or reordered");
        assert_eq!(receipts, [Bytes::copy_from_slice(&1_u64.to_be_bytes()), Bytes::copy_from_slice(&2_u64.to_be_bytes())],
            "backend receipt was lost, repeated or assigned to the wrong delivered packet");
        assert!(backend_closed && process_closed && control_closed && server_drained
            && reliable_drained && unreliable_drained && signaling_joined && revoked
            && cleanup_empty && world_running, "original owned cleanup did not finish");
    }).await.expect("finite original backend output control stalled");
}

/// Observes Pending only after the real effect checks out its account, then resumes that same future.
async fn finish_after_checkout(
    operation: impl Future<Output = io::Result<()>>,
    reserved_account: &RefCell<Option<Operation>>,
) -> (io::Result<()>, bool) {
    tokio::pin!(operation);
    let first = poll_fn(|cx| {
        let result = operation.as_mut().poll(cx);
        if result.is_ready() || reserved_account.borrow().is_some() {
            Poll::Ready(result)
        } else {
            Poll::Pending
        }
    })
    .await;
    let waited = first.is_pending() && reserved_account.borrow().is_some();
    drop(reserved_account.borrow_mut().take());
    let result = match first {
        Poll::Ready(result) => result,
        Poll::Pending => operation.await,
    };
    (result, waited)
}

struct Backend {
    process: HostProcess,
    backend: ActiveBackendPeer,
    input: Reader,
    output: Writer,
    control: Reader,
    control_output: Writer,
}

/// Completes actual capability/Open/Attach/Initialized phases; supplies no fabricated accepted Login.
async fn backend(
    host: &mut NativeHostedWorld,
    approval: &HostedApproval,
    deadline: Instant,
) -> Backend {
    let pending = PendingHostProcess::bind(
        &[7; CAPABILITY_BYTES as usize],
        &[8; OPENING_BYTES as usize],
    )
    .await
    .unwrap();
    let bootstrap = Bootstrap {
        address: pending.bootstrap().address.clone(),
        opening: pending.bootstrap().opening.clone(),
        capability: pending.bootstrap().capability.clone(),
    };
    let (process, (mut control, mut control_output)) =
        tokio::join!(pending.establish(deadline), async {
            let (input, mut output) = child(&bootstrap, 0).await;
            json(
                &mut output,
                Kind::Snapshot,
                &Snapshot {
                    protocol_version: u32::try_from(protocol::PROTOCOL_VERSION).unwrap(),
                    max_players: MAX_PLAYERS,
                    max_chunk_radius: 1,
                    required: false,
                    packs: Vec::new(),
                },
            )
            .await;
            (input, output)
        },);
    let mut process = process.unwrap();
    let (backend, (mut input, mut output)) = tokio::join!(
        async {
            host.open_backend(
                approval,
                &mut process,
                Identity {
                    xuid: approval.xuid.clone(),
                    uuid: Uuid::from_u128(1).to_string(),
                    display_name: "Fixture".into(),
                    playfab_id: String::new(),
                    playfab_title_id: String::new(),
                },
                serde_json::value::RawValue::from_string("{}".into()).unwrap(),
                deadline,
            )
            .await
            .unwrap()
        },
        async {
            let frame = control.recv().await.unwrap().unwrap();
            assert_eq!(frame.kind, Kind::Open);
            let open: bridge::host_world::contract::Open =
                serde_json::from_slice(&frame.body).unwrap();
            let peer = child(&bootstrap, open.peer).await;
            json(
                &mut control_output,
                Kind::Opened,
                &PeerResult {
                    peer: open.peer,
                    code: 0,
                },
            )
            .await;
            peer
        },
    );
    let (backend, ()) = tokio::join!(host.attach_backend(approval, backend, deadline), async {
        assert_eq!(input.recv().await.unwrap().unwrap().kind, Kind::Attach);
        output.admit(Kind::World, b"bounded-world-output").unwrap();
        output.flush().await.unwrap();
    },);
    let (backend, ()) = tokio::join!(
        host.initialize_backend(approval, backend.unwrap(), false, 1, deadline),
        async {
            assert_eq!(input.recv().await.unwrap().unwrap().kind, Kind::Initialized);
        },
    );
    Backend {
        process,
        backend: backend.unwrap(),
        input,
        output,
        control,
        control_output,
    }
}

/// Uses the actual original private-process mutual handshake on numeric loopback TCP.
async fn child(bootstrap: &Bootstrap, peer: u64) -> (Reader, Writer) {
    let (mut input, mut output) = split(TcpStream::connect(&bootstrap.address).await.unwrap());
    json(
        &mut output,
        Kind::Hello,
        &Hello {
            version: VERSION,
            opening: bootstrap.opening.clone(),
            capability: bootstrap.capability.clone(),
            peer,
        },
    )
    .await;
    assert_eq!(input.recv().await.unwrap().unwrap().kind, Kind::Hello);
    (input, output)
}

/// Writes one generated bounded fixture record through the actual concrete Writer.
async fn json(writer: &mut Writer, kind: Kind, record: &impl serde::Serialize) {
    writer
        .admit(kind, &serde_json::to_vec(record).unwrap())
        .unwrap();
    writer.flush().await.unwrap();
}

/// Keeps failed fixture dispatch explicit while retaining all real cleanup owners.
fn ended() -> io::Error {
    io::Error::new(io::ErrorKind::UnexpectedEof, "owned backend fixture ended")
}
