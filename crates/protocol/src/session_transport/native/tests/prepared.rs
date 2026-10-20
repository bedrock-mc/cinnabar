use super::*;

#[tokio::test]
async fn graceful_send_seal_drains_accepted_work_and_rejects_an_old_permit() {
    let (sent, mut observed) = mpsc::channel(1);
    let (closed, retired) = oneshot::channel();
    let PreparedPump {
        sender,
        incoming: _incoming,
        stop,
        driver,
    } = prepare(ClosingConnection {
        sent,
        closed: Some(closed),
    });
    let receipt = sender
        .accept(Bytes::from_static(b"accepted"))
        .await
        .unwrap();
    let old_permit = sender.send.reserve().await.unwrap();
    sender.close_admission();
    assert_eq!(
        sender
            .commit(old_permit, Bytes::from_static(b"uncommitted"))
            .unwrap_err()
            .kind(),
        io::ErrorKind::BrokenPipe,
    );
    stop.send(()).unwrap();
    let end = driver.await;
    end.close.unwrap();
    assert!(end.pending.is_none());
    assert_eq!(observed.recv().await.unwrap(), b"accepted"[..]);
    receipt.await.unwrap().unwrap();
    retired.await.unwrap();
    assert!(observed.recv().await.is_none());
}

/// Observes the actual prepared driver's accepted sends and transport-close boundary.
struct ClosingConnection {
    sent: mpsc::Sender<Bytes>,
    closed: Option<oneshot::Sender<()>>,
}

impl PacketConnection for ClosingConnection {
    async fn send_body(&mut self, body: Bytes) -> io::Result<()> {
        self.sent.send(body).await.map_err(io::Error::other)
    }

    async fn recv_body(&mut self) -> io::Result<Bytes> {
        std::future::pending().await
    }

    async fn close(&mut self) -> io::Result<()> {
        if let Some(closed) = self.closed.take() {
            let _ = closed.send(());
        }
        Ok(())
    }
}
