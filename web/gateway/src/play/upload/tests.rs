//! The actual read owner retains consumed bytes and one timer through borrowed cancellation.

use super::*;
use std::io::Cursor;
use tokio::io::AsyncReadExt;

/// A canceled borrow cannot re-read, re-charge or restart the pending prefix before actual EOF.
#[tokio::test(start_paused = true)]
async fn post_hello_upload_borrowed_cancel_retains_original_wait_and_short_reads() {
    let rate = usize::try_from(RELAY_UPLOAD_BYTES_PER_SECOND).unwrap();
    let first_len = rate / 2;
    let mut source = vec![0x33; RELAY_UPLOAD_BURST_BYTES];
    source.extend(std::iter::repeat_n(0x11, first_len));
    source.extend(std::iter::repeat_n(0x22, rate - first_len));
    let mut reader = Upload::new(Cursor::new(source));
    let started = Instant::now();
    let mut burst = vec![0; RELAY_UPLOAD_BURST_BYTES];
    reader.read_exact(&mut burst).await.unwrap();
    assert!(burst.iter().all(|byte| *byte == 0x33));
    assert_eq!(Instant::now(), started);

    let mut cancelled_output = vec![0; rate];
    {
        let reading = reader.read_exact(&mut cancelled_output);
        tokio::pin!(reading);
        assert!(futures::poll!(&mut reading).is_pending());
    }
    let consumed = u64::try_from(RELAY_UPLOAD_BURST_BYTES.checked_add(rate).unwrap()).unwrap();
    assert_eq!(
        reader.inner.position(),
        consumed,
        "pacing must follow the actual read"
    );
    tokio::time::advance(Duration::from_millis(500)).await;

    let mut first = vec![0; first_len];
    reader.read_exact(&mut first).await.unwrap();
    assert!(first.iter().all(|byte| *byte == 0x11));
    assert_eq!(
        started.elapsed(),
        Duration::from_secs(1),
        "the original read wait was restarted"
    );
    assert_eq!(reader.inner.position(), consumed);
    let mut second = vec![0; rate - first_len];
    reader.read_exact(&mut second).await.unwrap();
    assert!(second.iter().all(|byte| *byte == 0x22));
    assert_eq!(
        started.elapsed(),
        Duration::from_secs(1),
        "paid bytes were charged twice"
    );
    assert_eq!(reader.inner.position(), consumed);
    assert_eq!(reader.read(&mut [0; 1]).await.unwrap(), 0);
    assert_eq!(
        started.elapsed(),
        Duration::from_secs(1),
        "EOF invented another pacing wait"
    );
}
